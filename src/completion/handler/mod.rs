//! Completion request orchestration.
//!
//! Coordinates the various completion strategies (PHPDoc tags, named
//! arguments, array shape keys, member access, variable names,
//! class/constant/function names) and returns the first successful
//! result.
//!
//! Each strategy is a private method, grouped into sibling modules by
//! concern:
//! - `phpdoc` — `complete_phpdoc_tag` (`@tag` completion inside docblocks)
//!   and `complete_docblock_type_or_variable` (type/variable after
//!   `@param`, `@return`, etc.)
//! - `class_constant` — `complete_type_hint` (type completion in
//!   parameter lists, return types, properties) and
//!   `try_class_constant_function_completion` (bare class/constant/
//!   function names, including `new` and `throw new`)
//! - `named_args` — `try_named_arg_completion`-style collection of
//!   `name:` argument completion inside call parens
//! - `member_access` — `try_member_access_completion` (`->` and `::`
//!   member completion) and the related override-suggestion strategy
//! - `patching` — source-text patches shared by `member_access` and
//!   `named_args` to recover a parseable AST mid-keystroke
//!
//! Strategies not big enough to warrant their own module (array shape
//! completion, variable name completion, catch clause completion, and
//! the PHPStan-ignore-code completion) stay here alongside the
//! orchestrator.
//!
//! Methods prefixed with `complete_` always short-circuit: the caller
//! unconditionally returns their result.  Methods prefixed with `try_`
//! return `Option<CompletionResponse>` where `None` means "not applicable,
//! try the next strategy."
use std::collections::BTreeSet;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::class_completion::{ClassCompletionParams, ClassNameContext};
use crate::text_position::position_to_byte_offset;
use crate::types::FileContext;

mod blade_component;
mod blade_directive;
mod class_constant;
mod member_access;
mod named_args;
mod patching;
mod phpdoc;

/// Append named-argument items into an existing [`CompletionResponse`].
///
/// If `named_arg_items` is empty the response is returned unchanged.
/// Otherwise the items are appended to the response's item list,
/// preserving the `is_incomplete` flag when the response is a
/// [`CompletionList`].
fn merge_named_args_into_response(
    response: CompletionResponse,
    named_arg_items: Vec<CompletionItem>,
) -> CompletionResponse {
    if named_arg_items.is_empty() {
        return response;
    }
    match response {
        CompletionResponse::Array(mut items) => {
            items.extend(named_arg_items);
            CompletionResponse::Array(items)
        }
        CompletionResponse::List(mut list) => {
            list.items.extend(named_arg_items);
            CompletionResponse::List(list)
        }
    }
}

/// Check whether a `(` immediately follows the cursor position (past any
/// partial identifier the user has already typed).
///
/// When the user is renaming an existing call — `$obj->oldName|()`,
/// `functionNa|()`, `new ClassNa|()` — the opening paren is already
/// present and inserting a snippet with its own `()` would produce
/// double parentheses like `method()()`.
fn paren_follows_cursor(content: &str, position: Position) -> bool {
    let byte_off = position_to_byte_offset(content, position);
    let rest = &content[byte_off..];
    // Skip past any partial identifier the user has typed
    // (ASCII letters, digits, underscore, backslash for namespaced names).
    let after_ident =
        rest.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '\\');
    after_ident.starts_with('(')
}

/// Downgrade callable snippet items to plain-name insertions.
///
/// When `(` already follows the cursor, snippets that insert their own
/// parentheses would produce duplicates.  This strips the snippet
/// format and replaces the insert text with just the name from
/// `filter_text`.
///
/// Applies to methods, functions, and class names (for `new` / `throw new`).
fn strip_snippet_parens(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    items
        .into_iter()
        .map(|mut item| {
            if item.insert_text_format == Some(InsertTextFormat::SNIPPET)
                && matches!(
                    item.kind,
                    Some(CompletionItemKind::METHOD)
                        | Some(CompletionItemKind::FUNCTION)
                        | Some(CompletionItemKind::CLASS)
                )
            {
                // Replace the snippet with just the name
                // (the filter_text already holds it).
                if let Some(ref name) = item.filter_text {
                    item.insert_text = Some(name.clone());
                }
                // Also clear any text_edit that carries the snippet text.
                if let Some(CompletionTextEdit::Edit(ref mut te)) = item.text_edit
                    && let Some(ref name) = item.filter_text
                {
                    te.new_text = name.clone();
                }
                item.insert_text_format = None;
            }
            item
        })
        .collect()
}

impl Backend {
    /// Main completion handler — called by `LanguageServer::completion`.
    ///
    /// Tries each completion strategy in priority order and returns the
    /// first one that produces results.  Falls back to no completions
    /// when nothing matches.
    pub(crate) fn handle_completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri.to_string();
        let mut position = params.text_document_position.position;
        let completion_context = params.context.clone();

        // ── Blade directive-name completion ─────────────────────────────
        // Runs before the virtual-PHP content/position swap below: an `@`
        // the user is still typing a directive name after doesn't survive
        // Blade preprocessing (an unrecognised directive is masked as
        // inert HTML), so this reads the raw buffer at the untranslated
        // position instead. Always short-circuits once the cursor is
        // confirmed to be in an HTML/directive position — even a prefix
        // matching no known directive returns an empty list rather than
        // falling through to class/variable completion.
        if self.is_blade_file(&uri)
            && let Some(prefix) = self.blade_directive_prefix_at(&uri, position)
        {
            return Ok(Some(self.complete_blade_directive(&prefix)));
        }

        // ── Blade component tag and attribute completion ────────────────
        // A `<x-` the user is still typing a component name after names no
        // component yet, so the preprocessor degrades it to a comment and
        // nothing of it survives into the virtual PHP — this too reads the
        // raw buffer at the untranslated position. Runs after the
        // directive check so a Blade directive written inside a tag
        // (`<x-alert @if(…)`) still completes as one.
        if self.is_blade_file(&uri)
            && let Some(response) = self.blade_component_completion(&uri, position)
        {
            return Ok(Some(response));
        }

        // ── Blade section / stack name completion ───────────────────────
        // Inside `@yield('|')`, `@section('|')`, `@push('|')` and their
        // helpers, the names come from the templates that render this one
        // (or the ones it renders), and the edit is in Blade coordinates —
        // so this too runs before the virtual-PHP swap below.
        if self.is_blade_file(&uri)
            && let Some(response) = self.blade_block_name_completion(&uri, position)
        {
            return Ok(Some(response));
        }

        // Get file content for offset calculation.  For Blade files,
        // use the virtual PHP content and translate the cursor position
        // so that variable resolution walks the preprocessed AST.
        let content = if self.is_blade_file(&uri) {
            let vc = self.blade_virtual_content.read();
            if let Some(virtual_php) = vc.get(&uri) {
                position = self.translate_blade_to_php(&uri, position);
                Some(virtual_php.clone())
            } else {
                self.get_file_content(&uri)
            }
        } else {
            self.get_file_content(&uri)
        };

        if let Some(content) = content {
            let response = (|| -> Result<Option<CompletionResponse>> {
                // Activate the chain resolution cache so that shared chain
                // prefixes are resolved once and reused within this completion
                // request.  The guard is re-entrant safe.
                let _chain_guard = crate::type_engine::resolver::with_chain_resolution_cache();
                let _resolver_guard =
                    crate::type_engine::call_resolution::activate_type_engine_caches();
                let _cache_guard = crate::virtual_members::with_active_resolved_class_cache(
                    &self.resolved_class_cache,
                );

                // Gather per-file context (classes, use-map, namespace) in one
                // call instead of three separate lock-and-unwrap blocks.
                // Use the cursor offset for position-aware namespace resolution
                // so that multi-namespace files resolve to the correct namespace.
                let cursor_offset = crate::text_position::position_to_offset(&content, position);
                let ctx = self.file_context_at(&uri, cursor_offset);

                // Scanning for a comment costs a pass over the file, so the answer
                // is taken once and reused by the check below it.
                let in_non_doc_comment =
                    crate::completion::comment_position::is_inside_non_doc_comment(
                        &content, position,
                    );

                if (in_non_doc_comment
                    || crate::completion::comment_position::is_inside_docblock(&content, position))
                    && let Some(prefix) =
                        crate::phpstan_ignore::phpstan_ignore_code_prefix(&content, position)
                {
                    return Ok(Some(self.complete_phpstan_ignore_code(&uri, &prefix)));
                }

                // ── Suppress completion inside non-doc comments ─────────
                if in_non_doc_comment {
                    return Ok(None);
                }

                // ── PHPDoc block generation on `/**` ────────────────────
                // When the user types `/**` above a declaration, generate
                // a complete docblock skeleton as a single snippet item.
                // Must run before the docblock-interior checks below.
                {
                    let class_loader = self.class_loader(&ctx);
                    let function_loader = self.function_loader(&ctx);
                    if let Some(response) =
                        crate::completion::phpdoc::generation::try_generate_docblock(
                            &content,
                            position,
                            &ctx.use_map,
                            &ctx.namespace,
                            &ctx.classes,
                            &class_loader,
                            Some(self),
                            Some(&function_loader),
                        )
                    {
                        return Ok(Some(response));
                    }
                }

                // ── PHPDoc tag completion ────────────────────────────────
                // Always short-circuits when an `@` prefix is detected
                // inside a docblock — even when the item list is empty.
                if let Some(prefix) =
                    crate::completion::phpdoc::extract_phpdoc_prefix(&content, position)
                {
                    return Ok(Some(
                        self.complete_phpdoc_tag(&content, &prefix, position, &ctx),
                    ));
                }

                // ── Docblock type / variable completion ─────────────────
                // Always short-circuits when inside a docblock.
                if crate::completion::comment_position::is_inside_docblock(&content, position) {
                    return Ok(
                        self.complete_docblock_type_or_variable(&content, position, &ctx, &uri)
                    );
                }

                // ── Type hint completion in definitions ─────────────────
                // Always short-circuits when a type-hint position is detected.
                if let Some(th_ctx) =
                    crate::completion::type_hint_completion::detect_type_hint_context(
                        &content, position,
                    )
                {
                    return Ok(self.complete_type_hint(&content, &th_ctx, &ctx, position, &uri));
                }

                // ── Class-body root member completion ───────────────────
                // At a new-member position with no modifier typed yet
                // (e.g. `o` right inside a class body), offer overridable
                // parent/interface/trait members with their full
                // declarations plus member keywords, and suppress
                // class/function/constant completions, which are invalid
                // at this position.
                //
                // Runs before the modifier-anchored check below because that
                // one scans backwards for `function`/`const` without skipping
                // comments, so a docblock mentioning `const` above the cursor
                // claims the position.  This check is lexical and only matches
                // a bare identifier, so `public function ge|` and `const FO|`
                // still fall through to it.
                if let Some(items) =
                    self.try_class_root_member_completion(&uri, &content, position, &ctx)
                {
                    if items.is_empty() {
                        return Ok(None);
                    }
                    return Ok(Some(CompletionResponse::Array(items)));
                }

                if crate::completion::context::override_completion::is_member_declaration_name_position_at_offset(
                &content,
                cursor_offset as usize,
            ) {
                if let Some(items) = self.try_method_override_completion(&content, position, &ctx)
                    && !items.is_empty()
                {
                    return Ok(Some(CompletionResponse::Array(items)));
                }
                return Ok(None);
            }

                // ── Named argument completion (collected, not short-circuited) ──
                // Named arg items are always valid alongside normal
                // completions, so collect them here and merge them into
                // whatever strategy wins below.
                let named_arg_items = self.collect_named_arg_items(&uri, &content, position, &ctx);

                // ── String context detection ────────────────────────────
                // Classify once and use throughout the remaining pipeline.
                let string_ctx = crate::completion::comment_position::classify_string_context(
                    &content, position,
                );
                use crate::completion::comment_position::StringContext;

                // ── Cursor's lexical position ───────────────────────────
                // Resolving it is a forward pass over the file, and every
                // string-argument strategy below needs the same answer (the
                // literal being typed in, and the brackets around it), so the
                // scan runs once here and is shared rather than repeated per
                // strategy on every keystroke.
                let code_ctx = crate::completion::source::code_context::code_context_at(
                    &content,
                    cursor_offset as usize,
                );
                // ── Array shape key completion ───────────────────────────
                // Runs before `InStringLiteral` suppression because in
                // normal code `$arr['` puts the scanner inside a
                // single-quoted string, yet array shape completion is
                // designed to work there.  Skip only in simple
                // interpolation: `"$arr['key']"` does NOT perform array
                // access in PHP (only `"{$arr['key']}"` does).
                if !matches!(string_ctx, StringContext::SimpleInterpolation)
                    && let Some(response) =
                        self.try_array_shape_completion(&content, position, &ctx)
                {
                    return Ok(Some(response));
                }

                // ── Laravel string key completion (route/config/view/trans) ──
                // Inside `route('|')`, `config('|')`, `view('|')`, `__('|')`,
                // etc., offer matching key names from the project.
                // NB: `is_laravel` is extracted to a `let` so the read lock
                // on `resolved_class_cache` is dropped before calling
                // `try_laravel_string_key_completion`, which may trigger
                // `ensure_workspace_indexed` → `update_ast` → write lock.
                let is_laravel = self.resolved_class_cache.read().is_laravel();
                if is_laravel
                    && matches!(
                        string_ctx,
                        StringContext::InStringLiteral | StringContext::NotInString
                    )
                    && let Some(response) =
                        self.try_laravel_string_key_completion_in_file(&content, position, &ctx)
                {
                    return Ok(Some(response));
                }

                // ── Path helper completion ──────────────────────────────
                // `base_path('|')` and friends complete a segment at a time
                // from the directory the argument has reached.
                if is_laravel
                    && matches!(string_ctx, StringContext::InStringLiteral)
                    && let Some(code) = code_ctx.as_ref()
                    && let Some(response) =
                        self.try_path_helper_completion(&content, position, code)
                {
                    return Ok(Some(response));
                }

                // ── Route parameter completion ──────────────────────────
                // The keys of `route('users.show', ['|' => 1])` are the URI
                // parameters of the named route.
                if is_laravel
                    && matches!(
                        string_ctx,
                        StringContext::InStringLiteral | StringContext::NotInString
                    )
                    && let Some(code) = code_ctx.as_ref()
                    && let Some(response) =
                        self.try_route_param_completion(&content, position, code)
                {
                    return Ok(Some(response));
                }

                // ── Artisan command parameter completion ────────────────
                // `$this->argument('|')` / `$this->option('|')` against the
                // enclosing command's own signature, and array keys of
                // `Artisan::call('cmd', ['|' => ...])`.
                if is_laravel
                    && matches!(
                        string_ctx,
                        StringContext::InStringLiteral | StringContext::NotInString
                    )
                    && let Some(code) = code_ctx.as_ref()
                    && let Some(response) =
                        self.try_command_param_completion(&content, position, code)
                {
                    return Ok(Some(response));
                }

                // ── Eloquent relation/column string completion ──────────
                // Like array shape completion, this triggers inside string
                // literals where the cursor is in a method argument position
                // for an Eloquent method that accepts relation or column names.
                if matches!(
                    string_ctx,
                    StringContext::InStringLiteral | StringContext::NotInString
                ) && let Some(code) = code_ctx.as_ref()
                    && let Some(response) =
                        self.try_eloquent_string_completion(&content, position, &ctx, code)
                {
                    return Ok(Some(response));
                }

                // ── model-property<Model> string completion ────────────
                // When the cursor is inside a string argument whose
                // parameter is typed as `model-property<Model>`, suggest
                // the model's known property names.
                if matches!(
                    string_ctx,
                    StringContext::InStringLiteral | StringContext::NotInString
                ) && let Some(code) = code_ctx.as_ref()
                    && let Some(response) =
                        self.try_model_property_completion(&content, position, &ctx, code)
                {
                    return Ok(Some(response));
                }

                // ── Request input key completion ────────────────────────
                // `$request->input('|')`, `->has('|')`, `$request['|']`, …
                // offer the field names the validation rules in scope define.
                // Runs after the Eloquent strategies because `has()` is also a
                // relation method: a Builder subject resolves there first, and
                // only a request-typed subject falls through to here.
                if is_laravel
                    && matches!(
                        string_ctx,
                        StringContext::InStringLiteral | StringContext::NotInString
                    )
                    && let Some(code) = code_ctx.as_ref()
                    && let Some(response) =
                        self.try_request_input_key_completion(&uri, &content, position, &ctx, code)
                {
                    return Ok(Some(response));
                }

                // ── Laravel route controller method completion ─────────
                // Inside `Route::controller(X::class)->group(fn(){…})`,
                // the 2nd argument string of Route::get/post/patch/… is a
                // controller method name.
                if matches!(
                    string_ctx,
                    StringContext::InStringLiteral | StringContext::NotInString
                ) && let Some(response) =
                    self.try_laravel_route_controller_completion(&uri, &content, position, &ctx)
                {
                    return Ok(Some(response));
                }

                // ── Array callable method completion ────────────────────
                // Like array shape and Eloquent string completion, this
                // triggers inside string literals — specifically the
                // method-name string in `[Class::class, 'method']`.
                if matches!(
                    string_ctx,
                    StringContext::InStringLiteral | StringContext::NotInString
                ) && let Some(response) =
                    self.try_array_callable_completion(&uri, &content, position, &ctx)
                {
                    return Ok(Some(response));
                }

                if matches!(string_ctx, StringContext::InStringLiteral) {
                    return Ok(None);
                }

                // ── Member access completion (-> or ::) ─────────────────
                if let Some(response) = self.try_member_access_completion(
                    &uri,
                    &content,
                    position,
                    &ctx,
                    completion_context.as_ref(),
                ) {
                    // In simple interpolation (`"$var->"`), PHP only allows
                    // property access — method calls and constants are
                    // syntax errors.  Filter to properties only.
                    if matches!(string_ctx, StringContext::SimpleInterpolation) {
                        let filtered = match response {
                            CompletionResponse::Array(items) => items
                                .into_iter()
                                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                                .collect(),
                            CompletionResponse::List(list) => list
                                .items
                                .into_iter()
                                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                                .collect(),
                        };
                        return Ok(Some(CompletionResponse::Array(filtered)));
                    }
                    return Ok(Some(response));
                }

                // ── Variable name completion ────────────────────────────
                // Placed before the interpolation guard so that `"$`
                // and `"{$` both offer variable suggestions.
                if let Some(response) = self.try_variable_name_completion(&content, position, &uri)
                {
                    return Ok(Some(response));
                }

                // Inside any interpolation context the only useful
                // completions are variable names and member access (handled
                // above).  Suppress the remaining completion strategies so
                // class names, catch clauses, etc. don't leak into strings.
                if matches!(
                    string_ctx,
                    StringContext::SimpleInterpolation | StringContext::BraceInterpolation
                ) {
                    return Ok(None);
                }

                // ── Smart catch clause completion ───────────────────────
                if let Some(response) = self.try_catch_completion(&content, position, &ctx, &uri) {
                    return Ok(Some(merge_named_args_into_response(
                        response,
                        named_arg_items,
                    )));
                }

                // ── Class declaration name completion ───────────────────
                // When declaring a new class/interface/trait/enum, suggest
                // the filename (without extension) as the class name.
                if let Some(response) =
                    self.try_class_declaration_completion(&uri, &content, position)
                {
                    return Ok(Some(response));
                }

                // ── Class name + constant + function completion ─────────
                if let Some(response) =
                    self.try_class_constant_function_completion(&content, position, &ctx, &uri)
                {
                    return Ok(Some(merge_named_args_into_response(
                        response,
                        named_arg_items,
                    )));
                }

                // No strategy matched, but we may still have named arg items.
                if !named_arg_items.is_empty() {
                    return Ok(Some(CompletionResponse::Array(named_arg_items)));
                }

                Ok(None)
            })()?;

            // A Blade file was swapped to its virtual PHP content and
            // position above, so any item carrying a `TextEdit` names a
            // position in the virtual file, not one the editor's buffer
            // has — translate it back, dropping items whose range falls in
            // the injected prologue rather than clamping them to the
            // template's start.
            return Ok(match response {
                Some(response) if self.is_blade_file(&uri) => {
                    Some(self.translate_completion_response(&uri, response))
                }
                other => other,
            });
        }

        Ok(None)
    }

    fn complete_phpstan_ignore_code(&self, uri: &str, prefix: &str) -> CompletionResponse {
        let prefix_lower = prefix.to_ascii_lowercase();
        let mut identifiers: BTreeSet<String> = BTreeSet::new();

        let cache = self.phpstan_tool.last_diags.lock();
        if let Some(diags) = cache.get(uri) {
            for diag in diags {
                let Some(NumberOrString::String(code)) = &diag.code else {
                    continue;
                };
                if code.is_empty() || code == "phpstan" || code.starts_with("ignore.unmatched") {
                    continue;
                }
                identifiers.insert(code.clone());
            }
        }

        let items = identifiers
            .into_iter()
            .filter(|id| id.to_ascii_lowercase().starts_with(&prefix_lower))
            .enumerate()
            .map(|(idx, id)| CompletionItem {
                label: id.clone(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some("PHPStan error identifier".to_string()),
                insert_text: Some(id),
                sort_text: Some(format!("0_{idx:03}")),
                ..CompletionItem::default()
            })
            .collect();

        CompletionResponse::Array(items)
    }

    // ─── Strategy: array shape key completion ────────────────────────────

    /// Try to offer known array shape keys when the cursor is inside
    /// `$var['` or `$var["`.
    ///
    /// Returns `None` when the cursor is not in an array-key context or
    /// when no shape keys could be resolved.
    fn try_array_shape_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let ak_ctx = crate::completion::array_shape::detect_array_key_context(content, position)?;
        let items = self.build_array_key_completions(&ak_ctx, content, position, ctx);
        if items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(items))
        }
    }

    // ─── Strategy: variable name completion ──────────────────────────────

    /// Try to offer `$variable` name completions.
    ///
    /// When the user is typing `$us`, `$_SE`, or just `$`, suggest
    /// variable names found in the current file plus PHP superglobals.
    ///
    /// Returns `None` when the cursor is not at a variable-name position
    /// or when no variables are found.
    fn try_variable_name_completion(
        &self,
        content: &str,
        position: Position,
        uri: &str,
    ) -> Option<CompletionResponse> {
        let partial = Self::extract_partial_variable_name(content, position)?;
        let symbol_maps = self.symbol_maps.read();
        let symbol_map = symbol_maps.get(uri).map(|arc| arc.as_ref());
        let (var_items, var_incomplete) =
            Self::build_variable_completions(content, &partial, position, symbol_map);

        if var_items.is_empty() {
            None
        } else {
            Some(CompletionResponse::List(CompletionList {
                is_incomplete: var_incomplete,
                items: var_items,
            }))
        }
    }

    // ─── Strategy: catch clause completion ───────────────────────────────

    /// Try to offer exception type completions inside a `catch(…)` clause.
    ///
    /// Analyses the corresponding try block and suggests only the exception
    /// types that are thrown or documented there.  When no specific thrown
    /// types are found, falls back to Throwable-filtered class completion.
    ///
    /// Returns `None` when the cursor is not inside a catch clause or when
    /// no completions could be produced.
    fn try_catch_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        uri: &str,
    ) -> Option<CompletionResponse> {
        let catch_ctx =
            crate::completion::catch_completion::detect_catch_context(content, position)?;

        let items = crate::completion::catch_completion::build_catch_completions(
            &catch_ctx,
            &ctx.use_map,
            &ctx.namespace,
        );
        if catch_ctx.has_specific_types && !items.is_empty() {
            // These items don't carry snippets, but guard for consistency.
            return Some(CompletionResponse::Array(items));
        }

        // No specific throws discovered — fall back to
        // Throwable-filtered class completion.  Already-parsed
        // classes are only offered when their parent chain
        // reaches \Throwable / \Exception / \Error.  Class index
        // and stub classes are included unfiltered because
        // checking their ancestry would require on-demand parsing.
        //
        // Use the partial from the catch context rather than
        // `extract_partial_class_name` — the latter returns
        // `None` when the cursor sits right after `(` with
        // nothing typed, but the catch context already
        // captured the (possibly empty) partial correctly.
        let partial = if catch_ctx.partial.is_empty() {
            Self::extract_partial_class_name(content, position).unwrap_or_default()
        } else {
            catch_ctx.partial.clone()
        };
        let (class_items, class_incomplete) =
            self.build_class_name_completions(ClassCompletionParams {
                file_use_map: &ctx.use_map,
                file_namespace: &ctx.namespace,
                prefix: &partial,
                content,
                context: ClassNameContext::Catch,
                position,
                affinity_table_override: None,
                uri,
            });
        let mut all_items = items; // Throwable item (if matched)
        for ci in class_items {
            if !all_items.iter().any(|existing| existing.label == ci.label) {
                all_items.push(ci);
            }
        }
        if all_items.is_empty() {
            None
        } else {
            let items = if paren_follows_cursor(content, position) {
                strip_snippet_parens(all_items)
            } else {
                all_items
            };
            Some(CompletionResponse::List(CompletionList {
                is_incomplete: class_incomplete,
                items,
            }))
        }
    }
}
