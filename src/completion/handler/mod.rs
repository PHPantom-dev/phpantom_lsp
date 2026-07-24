/// Completion request orchestration.
///
/// This module contains the main `handle_completion` method that was
/// previously inlined in `server.rs`.  It coordinates the various
/// completion strategies (PHPDoc tags, named arguments, array shape keys,
/// member access, variable names, class/constant/function names) and
/// returns the first successful result.
///
/// Each strategy is a private method, grouped into sibling modules by
/// concern:
/// - `phpdoc` — `complete_phpdoc_tag` (`@tag` completion inside docblocks)
///   and `complete_docblock_type_or_variable` (type/variable after
///   `@param`, `@return`, etc.)
/// - `class_constant` — `complete_type_hint` (type completion in
///   parameter lists, return types, properties) and
///   `try_class_constant_function_completion` (bare class/constant/
///   function names, including `new` and `throw new`)
/// - `named_args` — `try_named_arg_completion`-style collection of
///   `name:` argument completion inside call parens
/// - `member_access` — `try_member_access_completion` (`->` and `::`
///   member completion) and the related override-suggestion strategy
/// - `patching` — source-text patches shared by `member_access` and
///   `named_args` to recover a parseable AST mid-keystroke
///
/// Strategies not big enough to warrant their own module (array shape
/// completion, variable name completion, catch clause completion, and
/// the PHPStan-ignore-code completion) stay here alongside the
/// orchestrator.
///
/// Methods prefixed with `complete_` always short-circuit: the caller
/// unconditionally returns their result.  Methods prefixed with `try_`
/// return `Option<CompletionResponse>` where `None` means "not applicable,
/// try the next strategy."
use std::collections::BTreeSet;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::class_completion::{ClassCompletionParams, ClassNameContext};
use crate::text_position::position_to_byte_offset;
use crate::types::FileContext;

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
            // Activate the chain resolution cache so that shared chain
            // prefixes are resolved once and reused within this completion
            // request.  The guard is re-entrant safe.
            let _chain_guard = super::resolver::with_chain_resolution_cache();
            let _body_infer_guard = self.activate_body_return_inferrer();
            let _auth_user_guard = self.activate_auth_user_resolver();
            let _cache_guard = crate::virtual_members::with_active_resolved_class_cache(
                &self.resolved_class_cache,
            );

            // Gather per-file context (classes, use-map, namespace) in one
            // call instead of three separate lock-and-unwrap blocks.
            // Use the cursor offset for position-aware namespace resolution
            // so that multi-namespace files resolve to the correct namespace.
            let cursor_offset = crate::text_position::position_to_offset(&content, position);
            let ctx = self.file_context_at(&uri, cursor_offset);

            if (crate::completion::comment_position::is_inside_non_doc_comment(&content, position)
                || crate::completion::comment_position::is_inside_docblock(&content, position))
                && let Some(prefix) =
                    crate::phpstan_ignore::phpstan_ignore_code_prefix(&content, position)
            {
                return Ok(Some(self.complete_phpstan_ignore_code(&uri, &prefix)));
            }

            // ── Suppress completion inside non-doc comments ─────────
            if crate::completion::comment_position::is_inside_non_doc_comment(&content, position) {
                return Ok(None);
            }

            // ── PHPDoc block generation on `/**` ────────────────────
            // When the user types `/**` above a declaration, generate
            // a complete docblock skeleton as a single snippet item.
            // Must run before the docblock-interior checks below.
            {
                let class_loader = self.class_loader(&ctx);
                let function_loader = self.function_loader(&ctx);
                if let Some(response) = crate::completion::phpdoc::generation::try_generate_docblock(
                    &content,
                    position,
                    &ctx.use_map,
                    &ctx.namespace,
                    &ctx.classes,
                    &class_loader,
                    Some(&function_loader),
                ) {
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
                return Ok(self.complete_docblock_type_or_variable(&content, position, &ctx, &uri));
            }

            // ── Type hint completion in definitions ─────────────────
            // Always short-circuits when a type-hint position is detected.
            if let Some(th_ctx) = crate::completion::type_hint_completion::detect_type_hint_context(
                &content, position,
            ) {
                return Ok(self.complete_type_hint(&content, &th_ctx, &ctx, position, &uri));
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
            let string_ctx =
                crate::completion::comment_position::classify_string_context(&content, position);
            use crate::completion::comment_position::StringContext;
            // ── Array shape key completion ───────────────────────────
            // Runs before `InStringLiteral` suppression because in
            // normal code `$arr['` puts the scanner inside a
            // single-quoted string, yet array shape completion is
            // designed to work there.  Skip only in simple
            // interpolation: `"$arr['key']"` does NOT perform array
            // access in PHP (only `"{$arr['key']}"` does).
            if !matches!(string_ctx, StringContext::SimpleInterpolation)
                && let Some(response) = self.try_array_shape_completion(&content, position, &ctx)
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
                && let Some(response) = self.try_laravel_string_key_completion(&content, position)
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
            ) && let Some(response) =
                self.try_eloquent_string_completion(&content, position, &ctx)
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
            ) && let Some(response) =
                self.try_model_property_completion(&content, position, &ctx)
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
            if let Some(response) = self.try_variable_name_completion(&content, position, &uri) {
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
            if let Some(response) = self.try_class_declaration_completion(&uri, &content, position)
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
        }

        // Nothing matched — return no completions.
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
