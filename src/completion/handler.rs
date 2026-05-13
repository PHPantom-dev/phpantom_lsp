/// Completion request orchestration.
///
/// This module contains the main `handle_completion` method that was
/// previously inlined in `server.rs`.  It coordinates the various
/// completion strategies (PHPDoc tags, named arguments, array shape keys,
/// member access, variable names, class/constant/function names) and
/// returns the first successful result.
///
/// Each strategy is extracted into a named private method:
/// - `complete_phpdoc_tag` — `@tag` completion inside docblocks
/// - `complete_docblock_type_or_variable` — type/variable after `@param`, `@return`, etc.
/// - `complete_type_hint` — type completion in parameter lists, return types, properties
/// - `try_named_arg_completion` — `name:` argument completion inside call parens
/// - `try_array_shape_completion` — `$arr['key']` completion from shape annotations
/// - `try_member_access_completion` — `->` and `::` member completion
/// - `try_variable_name_completion` — `$var` name completion
/// - `try_catch_completion` — exception type completion inside `catch()`
/// - `try_class_constant_function_completion` — bare class/constant/function names (including `new` and `throw new`)
///
/// Methods prefixed with `complete_` always short-circuit: the caller
/// unconditionally returns their result.  Methods prefixed with `try_`
/// return `Option<CompletionResponse>` where `None` means "not applicable,
/// try the next strategy."
///
/// Helper methods `patch_content_at_cursor` and `resolve_named_arg_params`
/// are also housed here because they are exclusively used by the
/// completion handler.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::resolver::ResolutionCtx;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::class_completion::{
    ClassCompletionParams, ClassNameContext, detect_class_name_context, is_class_declaration_name,
};
use crate::completion::named_args::{
    NamedArgContext, cursor_inside_nested_bracket, parse_existing_args,
};
use crate::docblock::types::PHPDOC_TYPE_KEYWORDS;
use crate::php_type::PhpType;
use crate::symbol_map::SymbolKind;
use crate::types::{ClassInfo, ResolvedType};
use crate::types::{CompletionTarget, FileContext};
use crate::util::{find_class_at_offset, position_to_byte_offset, position_to_offset};

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

/// Filter out completion items for classes defined in the current file.
///
/// When writing a `use` statement it makes no sense to import a class
/// from the file you are already in.  The `detail` field of each item
/// carries the FQN, which is matched against the FQNs of classes in the
/// file's `ctx.classes` (from the uri_classes_index).
fn filter_current_file_classes(
    items: Vec<CompletionItem>,
    ctx: &FileContext,
) -> Vec<CompletionItem> {
    if ctx.classes.is_empty() {
        return items;
    }
    let current_fqns: HashSet<String> = ctx
        .classes
        .iter()
        .map(|cls| {
            if let Some(ref ns) = ctx.namespace {
                format!("{}\\{}", ns, cls.name)
            } else {
                cls.name.to_string()
            }
        })
        .collect();
    items
        .into_iter()
        .filter(|item| {
            item.detail
                .as_ref()
                .is_none_or(|d| !current_fqns.contains(d))
        })
        .collect()
}

/// Filter out completion items for functions defined in the current file.
///
/// Collects the map keys (FQNs) of functions whose URI matches the
/// current file and removes any completion item whose `insert_text`
/// matches one of those FQNs.  This works for both use-import items
/// (where `insert_text` is the FQN) and inline items (where
/// `insert_text` is a snippet starting with the short name, which
/// equals the FQN for global functions).
fn filter_current_file_functions(
    items: Vec<CompletionItem>,
    current_uri: &str,
    backend: &Backend,
) -> Vec<CompletionItem> {
    let current_funcs: HashSet<String> = {
        let fmap = backend.global_functions().read();
        fmap.iter()
            .filter(|(_, (uri, _))| uri == current_uri)
            .map(|(key, _)| key.clone())
            .collect()
    };
    if current_funcs.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|item| {
            item.insert_text
                .as_ref()
                .is_none_or(|it| !current_funcs.contains(it))
        })
        .collect()
}

/// Filter out completion items for constants defined in the current file.
fn filter_current_file_constants(
    items: Vec<CompletionItem>,
    current_uri: &str,
    backend: &Backend,
) -> Vec<CompletionItem> {
    let current_consts: HashSet<String> = {
        let dmap = backend.global_defines().read();
        dmap.iter()
            .filter(|(_, info)| info.file_uri.as_str() == current_uri)
            .map(|(name, _)| name.clone())
            .collect()
    };
    if current_consts.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|item| {
            item.filter_text
                .as_ref()
                .is_none_or(|ft| !current_consts.contains(ft))
        })
        .collect()
}

/// Append a semicolon to the `insert_text` of each completion item.
///
/// Used for `use`, `use function`, and `use const` completions so that
/// accepting a suggestion produces a complete statement (e.g. `use Foo\Bar;`).
fn append_semicolon_to_insert_text(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    items
        .into_iter()
        .map(|mut item| {
            // Namespace segment items (MODULE kind) represent
            // intermediate namespace paths the user can drill into.
            // They should not receive a trailing semicolon because
            // the user will continue typing after selecting one
            // (e.g. `use App\Models\` → pick a class next).
            if item.kind == Some(CompletionItemKind::MODULE) {
                return item;
            }
            if let Some(ref mut text) = item.insert_text
                && !text.ends_with(';')
            {
                text.push(';');
            }
            if let Some(CompletionTextEdit::Edit(ref mut edit)) = item.text_edit
                && !edit.new_text.ends_with(';')
            {
                edit.new_text.push(';');
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
    pub(crate) async fn handle_completion(
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
            let _cache_guard = crate::virtual_members::with_active_resolved_class_cache(
                &self.resolved_class_cache,
            );

            // Gather per-file context (classes, use-map, namespace) in one
            // call instead of three separate lock-and-unwrap blocks.
            // Use the cursor offset for position-aware namespace resolution
            // so that multi-namespace files resolve to the correct namespace.
            let cursor_offset = crate::util::position_to_offset(&content, position);
            let ctx = self.file_context_at(&uri, cursor_offset);

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

    // ─── Strategy: PHPDoc tag completion ─────────────────────────────────

    /// Build completions for `@tag` names inside a `/** … */` docblock.
    ///
    /// Called when [`crate::completion::phpdoc::extract_phpdoc_prefix`]
    /// detects that the cursor follows an `@` sign inside a docblock.
    /// Always returns a response (possibly with an empty item list) so
    /// that partial tags like `@potato` never fall through to
    /// class/constant/function completion.
    fn complete_phpdoc_tag(
        &self,
        content: &str,
        prefix: &str,
        position: Position,
        ctx: &FileContext,
    ) -> CompletionResponse {
        let context = crate::completion::phpdoc::detect_context(content, position);
        let class_loader = self.class_loader(ctx);
        let function_loader = self.function_loader(ctx);

        // For inline variable assignments, try to infer the type from
        // the assignment RHS so that @var can be pre-filled.
        let inferred_var_type =
            if matches!(context, crate::completion::phpdoc::DocblockContext::Inline) {
                let sym = crate::completion::phpdoc::extract_symbol_info(content, position);
                crate::completion::phpdoc::generation::infer_inline_variable_type(
                    &sym,
                    content,
                    position,
                    &ctx.classes,
                    &class_loader,
                    Some(&function_loader as &dyn Fn(&str) -> Option<crate::types::FunctionInfo>),
                )
            } else {
                None
            };

        let smart = crate::completion::phpdoc::SmartContext {
            inferred_inline_var_type: inferred_var_type,
            class_loader: Some(&class_loader),
            function_loader: Some(&function_loader),
        };
        let items = crate::completion::phpdoc::build_phpdoc_completions(
            content,
            prefix,
            context,
            position,
            &ctx.use_map,
            &ctx.namespace,
            &smart,
        );
        CompletionResponse::Array(items)
    }

    // ─── Strategy: docblock type / variable completion ───────────────────

    /// Build completions at a type or variable position inside a docblock.
    ///
    /// When the cursor is inside a `/** … */` docblock at a recognised tag
    /// position (e.g. after `@param `, `@return `, `@throws `, `@var `),
    /// offer class-name or `$variable` completions as appropriate.  At all
    /// other docblock positions (descriptions, unknown tags) return `None`
    /// so that random words don't trigger class/variable suggestions.
    fn complete_docblock_type_or_variable(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        uri: &str,
    ) -> Option<CompletionResponse> {
        use crate::completion::phpdoc::{
            DocblockTypingContext, detect_docblock_typing_position, extract_symbol_info,
        };

        match detect_docblock_typing_position(content, position) {
            Some(DocblockTypingContext::Type { partial, tag }) => {
                // For @throws, use Throwable-filtered completion with
                // the same ordering as `throw new` so that exception
                // classes appear at the top.
                if tag == "throws" {
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
                    return if class_items.is_empty() {
                        None
                    } else {
                        Some(CompletionResponse::List(CompletionList {
                            is_incomplete: class_incomplete,
                            items: class_items,
                        }))
                    };
                }

                // Offer scalar / built-in types first, then class
                // / interface / enum names from the project.
                let partial_lower = partial.to_lowercase();
                let mut items: Vec<CompletionItem> = PHPDOC_TYPE_KEYWORDS
                    .iter()
                    .filter(|t| t.to_lowercase().starts_with(&partial_lower))
                    .enumerate()
                    .map(|(idx, t)| CompletionItem {
                        label: t.to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        detail: Some("PHP built-in type".to_string()),
                        insert_text: Some(t.to_string()),
                        filter_text: Some(t.to_string()),
                        sort_text: Some(format!("0_scalar_{:03}", idx)),
                        ..CompletionItem::default()
                    })
                    .collect();

                let (class_items, class_incomplete) =
                    self.build_class_name_completions(ClassCompletionParams {
                        file_use_map: &ctx.use_map,
                        file_namespace: &ctx.namespace,
                        prefix: &partial,
                        content,
                        context: ClassNameContext::TypeHint,
                        position,
                        affinity_table_override: None,
                        uri,
                    });
                items.extend(class_items);

                if items.is_empty() {
                    None
                } else {
                    Some(CompletionResponse::List(CompletionList {
                        is_incomplete: class_incomplete,
                        items,
                    }))
                }
            }
            Some(DocblockTypingContext::Variable { partial }) => {
                // Offer $parameter names from the function declaration.
                let sym = extract_symbol_info(content, position);
                let partial_lower = partial.to_lowercase();

                // Compute an explicit replacement range covering the typed
                // `$…` prefix.  Using `text_edit` with a range prevents
                // the double-dollar problem in editors (Helix, Neovim) that
                // don't treat `$` as a word character — the same fix that
                // was applied to regular variable completion.
                let prefix_char_len = partial.chars().count() as u32;
                let replace_range = Range {
                    start: Position {
                        line: position.line,
                        character: position.character.saturating_sub(prefix_char_len),
                    },
                    end: position,
                };

                let items: Vec<CompletionItem> = sym
                    .params
                    .iter()
                    .filter(|(_, name)| {
                        partial_lower.is_empty() || name.to_lowercase().starts_with(&partial_lower)
                    })
                    .map(|(type_hint, name)| {
                        let detail = type_hint
                            .as_ref()
                            .map(|t| t.to_string())
                            .unwrap_or_else(|| PhpType::mixed().to_string());
                        CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::VARIABLE),
                            detail: Some(detail),
                            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                                range: replace_range,
                                new_text: name.clone(),
                            })),
                            filter_text: Some(name.clone()),
                            sort_text: Some(format!("0_{}", name.to_lowercase())),
                            ..CompletionItem::default()
                        }
                    })
                    .collect();
                if items.is_empty() {
                    None
                } else {
                    Some(CompletionResponse::Array(items))
                }
            }
            None => {
                // Description text or unrecognised position — no
                // completions.
                None
            }
        }
    }

    // ─── Strategy: type hint completion ──────────────────────────────────

    /// Build completions at a type-hint position inside a function/method
    /// parameter list, return type, or property declaration.
    ///
    /// Offers PHP native scalar types alongside class-name completions (but
    /// NOT constants or standalone functions, which are invalid in type
    /// positions).
    ///
    /// This check MUST run before named-argument detection so that typing
    /// inside a function *definition* like `function foo(Us|)` offers type
    /// completions rather than named-argument suggestions.
    fn complete_type_hint(
        &self,
        content: &str,
        th_ctx: &crate::completion::type_hint_completion::TypeHintContext,
        ctx: &FileContext,
        position: Position,
        uri: &str,
    ) -> Option<CompletionResponse> {
        let partial_lower = th_ctx.partial.to_lowercase();
        let space_prefix = if th_ctx.needs_space_prefix { " " } else { "" };
        let mut items: Vec<CompletionItem> =
            crate::completion::type_hint_completion::PHP_NATIVE_TYPES
                .iter()
                .filter(|t| t.to_lowercase().starts_with(&partial_lower))
                .enumerate()
                .map(|(idx, t)| CompletionItem {
                    label: t.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some("PHP built-in type".to_string()),
                    insert_text: Some(format!("{}{}", space_prefix, t)),
                    filter_text: Some(t.to_string()),
                    sort_text: Some(format!("0_{:03}", idx)),
                    ..CompletionItem::default()
                })
                .collect();

        let (class_items, class_incomplete) =
            self.build_class_name_completions(ClassCompletionParams {
                file_use_map: &ctx.use_map,
                file_namespace: &ctx.namespace,
                prefix: &th_ctx.partial,
                content,
                context: ClassNameContext::TypeHint,
                position,
                affinity_table_override: None,
                uri,
            });

        // When a leading space is needed (return type after `:` with no
        // space), prefix the insert text of each class-name item so that
        // the result is `: ClassName` rather than `:ClassName`.
        if th_ctx.needs_space_prefix {
            for mut item in class_items {
                if let Some(ref txt) = item.insert_text {
                    item.insert_text = Some(format!(" {}", txt));
                }
                if let Some(CompletionTextEdit::Edit(ref te)) = item.text_edit {
                    item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
                        range: te.range,
                        new_text: format!(" {}", te.new_text),
                    }));
                }
                items.push(item);
            }
        } else {
            items.extend(class_items);
        }

        if items.is_empty() {
            // Even when empty, the caller returns early so we don't fall
            // through to named-arg or class+constant+function completion.
            None
        } else {
            Some(CompletionResponse::List(CompletionList {
                is_incomplete: class_incomplete,
                items,
            }))
        }
    }

    // ─── Strategy: named argument completion ─────────────────────────────

    /// Collect `name:` argument completion items inside function/method
    /// call parentheses.
    ///
    /// Returns an empty `Vec` when the cursor is not in a named-argument
    /// context or when no parameters could be resolved.  The items are
    /// meant to be **merged** into whatever other completion strategy
    /// wins — named args are always valid alongside normal completions.
    fn collect_named_arg_items(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
    ) -> Vec<CompletionItem> {
        // ── Primary path: AST-based detection via symbol map ────────
        // The symbol map's `CallSite` data handles chains, nesting,
        // and strings correctly.  Fall back to text scanning when the
        // AST has no hit (typically because the parser couldn't recover
        // from incomplete code).
        let na_ctx = match self
            .detect_named_arg_from_symbol_map(uri, content, position)
            .or_else(|| crate::completion::named_args::detect_named_arg_context(content, position))
        {
            Some(ctx) => ctx,
            None => return Vec::new(),
        };

        let mut params = self.resolve_named_arg_params(&na_ctx, content, position, ctx);

        // If resolution failed, the parser may have choked on
        // incomplete code (e.g. an unclosed `(`).  Patch the
        // content by inserting `);` at the cursor position so
        // the class body becomes syntactically valid, then
        // re-parse and retry resolution.
        if params.is_empty() {
            let patched = Self::patch_content_at_cursor(content, position);
            if patched != content {
                let patched_classes: Vec<Arc<crate::types::ClassInfo>> =
                    self.parse_php(&patched).into_iter().map(Arc::new).collect();
                if !patched_classes.is_empty() {
                    let patched_ctx = FileContext {
                        classes: patched_classes,
                        use_map: ctx.use_map.clone(),
                        namespace: ctx.namespace.clone(),
                        resolved_names: ctx.resolved_names.clone(),
                    };
                    params =
                        self.resolve_named_arg_params(&na_ctx, &patched, position, &patched_ctx);
                }
            }
        }

        crate::completion::named_args::build_named_arg_completions(&na_ctx, &params)
    }

    /// Detect a named-argument context using precomputed [`CallSite`] data
    /// from the symbol map.
    ///
    /// Returns `None` when the symbol map has no enclosing call site at the
    /// cursor (e.g. the parser couldn't recover from incomplete code) or
    /// when the cursor is in a position that should not trigger named-arg
    /// completion (preceded by `$`, `->`, or `::`).
    fn detect_named_arg_from_symbol_map(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<NamedArgContext> {
        let symbol_map = self.symbol_maps.read().get(uri).cloned()?;

        let cursor_byte_offset = position_to_offset(content, position);
        let cs = symbol_map.find_enclosing_call_site(cursor_byte_offset)?;

        // ── Bail out when cursor is inside a nested `[…]` or `{…}` ─
        // If the cursor sits inside an array literal or braced
        // expression that is itself an argument, named-arg completion
        // for the outer call must not fire — the user wants normal
        // value completion, not parameter names.
        if cursor_inside_nested_bracket(
            content,
            cs.args_start as usize,
            cursor_byte_offset as usize,
        ) {
            return None;
        }

        // ── Check eligibility at cursor ─────────────────────────────
        // Walk backward from cursor through identifier chars to find the
        // start of the current "word" in the raw source text.
        let bytes = content.as_bytes();
        let mut word_start = cursor_byte_offset as usize;
        while word_start > 0 && {
            let b = bytes[word_start - 1];
            b.is_ascii_alphanumeric() || b == b'_'
        } {
            word_start -= 1;
        }

        // If preceded by `$`, this is a variable, not a named arg.
        if word_start > 0 && bytes[word_start - 1] == b'$' {
            return None;
        }
        // If preceded by `->` or `::`, member completion handles this.
        if word_start >= 2 && bytes[word_start - 2] == b'-' && bytes[word_start - 1] == b'>' {
            return None;
        }
        if word_start >= 2 && bytes[word_start - 2] == b':' && bytes[word_start - 1] == b':' {
            return None;
        }

        let prefix = content
            .get(word_start..cursor_byte_offset as usize)
            .unwrap_or("")
            .to_string();

        // ── Parse arguments between `(` and cursor ──────────────────
        let args_text = content
            .get(cs.args_start as usize..word_start)
            .unwrap_or("");
        let (existing_named, positional_count) = parse_existing_args(args_text);

        Some(NamedArgContext {
            call_expression: cs.call_expression.clone(),
            existing_named_args: existing_named,
            positional_count,
            prefix,
        })
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

    // ─── Strategy: member access completion ──────────────────────────────

    /// Try to offer member completions after `->`, `?->`, or `::`.
    ///
    /// Resolves the subject to one or more `ClassInfo` values, merges
    /// inherited members, and builds completion items filtered by access
    /// kind and visibility.
    ///
    /// Returns `None` when there is no access operator before the cursor
    /// or when resolution produces no results.
    fn try_member_access_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
        completion_context: Option<&CompletionContext>,
    ) -> Option<CompletionResponse> {
        // ── Primary path: AST-based detection via symbol map ────────
        // The symbol map's `MemberAccess` correctly handles `(new Foo)->`,
        // call-result chains, array access chains, and null-safe chains.
        // Fall back to text scanning when the symbol map has no hit.
        let target = self
            .extract_completion_target_from_symbol_map(uri, content, position)
            .or_else(|| super::target::extract_completion_target(content, position))?;

        let cursor_offset = position_to_offset(content, position);
        let current_class = find_class_at_offset(&ctx.classes, cursor_offset);
        let prefix = if completion_context.is_some() {
            Self::member_completion_prefix(content, position)
        } else {
            String::new()
        };

        let class_loader = self.class_loader(ctx);
        let function_loader = self.function_loader(ctx);

        // `static::` in a final class is equivalent to `self::` but
        // suggests the class can be subclassed — which it can't.
        // Suppress suggestions to nudge the developer toward `self::`.
        let suppress = target.subject == "static" && current_class.is_some_and(|cc| cc.is_final);

        // ── Resolve subject to concrete types ───────────────────────
        // We resolve the subject BEFORE checking the cache so that the
        // cache key can include the actual resolved types. This prevents
        // stale results if a variable (e.g. `$model`) changes type
        // within the same file.
        let rctx = ResolutionCtx {
            current_class,
            all_classes: &ctx.classes,
            content,
            cursor_offset,
            class_loader: &class_loader,
            resolved_class_cache: Some(&self.resolved_class_cache),
            function_loader: Some(&function_loader),
            scope_var_resolver: None,
            is_in_static_method: false,
        };
        let mut resolved = if suppress {
            vec![]
        } else {
            super::resolver::resolve_target_classes(&target.subject, target.access_kind, &rctx)
        };

        // ── Incomplete-expression retry ─────────────────────────────
        if resolved.is_empty() && !suppress && target.subject.starts_with('$') {
            let patched = Self::patch_incomplete_member_access(content, position);
            if patched != content {
                let patched_classes: Vec<Arc<crate::types::ClassInfo>> =
                    self.parse_php(&patched).into_iter().map(Arc::new).collect();
                let patched_offset = position_to_offset(&patched, position);
                let patched_current = find_class_at_offset(&patched_classes, patched_offset);
                let patched_rctx = ResolutionCtx {
                    current_class: patched_current,
                    all_classes: &patched_classes,
                    content: &patched,
                    cursor_offset: patched_offset,
                    class_loader: &class_loader,
                    resolved_class_cache: Some(&self.resolved_class_cache),
                    function_loader: Some(&function_loader),
                    scope_var_resolver: None,
                    is_in_static_method: false,
                };
                resolved = super::resolver::resolve_target_classes(
                    &target.subject,
                    target.access_kind,
                    &patched_rctx,
                );
            }
        }

        if resolved.is_empty() {
            return None;
        }

        let resolved_types: Vec<String> = resolved
            .iter()
            .map(|rt| rt.type_string.to_string())
            .collect();
        let cache_key =
            Self::member_completion_cache_key(uri, &target, current_class, &resolved_types);

        // Wrap resolution + inheritance merging in catch_unwind so
        // that a stack overflow (e.g. from deep trait/inheritance
        // resolution when the subject is a call expression like
        // `collect($x)->`) doesn't crash the LSP server process.
        let started = std::time::Instant::now();
        let cached_items = self.member_completion_cache.lock().get(&cache_key).cloned();
        let cache_hit = cached_items.is_some();
        let member_items = cached_items.or_else(|| {
            crate::util::catch_panic_unwind_safe(
                "member-access completion",
                uri,
                Some(position),
                || {
                    let candidates = ResolvedType::into_arced_classes(resolved);
                    if candidates.is_empty() {
                        return vec![];
                    }

                    // `parent::`, `self::`, and `static::` are syntactically
                    // `::` but semantically different from external static
                    // access.
                    let effective_access =
                        if matches!(target.subject.as_str(), "parent" | "self" | "static") {
                            crate::AccessKind::ParentDoubleColon
                        } else {
                            target.access_kind
                        };

                    super::builder::build_union_completion_items(
                        &candidates,
                        effective_access,
                        current_class,
                        &class_loader,
                        &self.resolved_class_cache,
                        uri,
                    )
                },
            )
            .inspect(|items| {
                if !items.is_empty() {
                    let mut cache = self.member_completion_cache.lock();
                    // Simple size limit: evict everything if the cache gets too big.
                    // This is crude but effective at preventing memory leaks
                    // in long-running sessions.
                    if cache.len() > 200 {
                        cache.clear();
                    }
                    cache.insert(cache_key.clone(), items.clone());
                }
            })
        });

        match member_items {
            Some(all_items) if !all_items.is_empty() => {
                let is_filtered = !prefix.is_empty();
                let unfiltered_count = all_items.len();
                let items = if is_filtered {
                    Self::filter_member_completion_items(all_items, &prefix)
                } else {
                    all_items
                };

                // ── Suppress snippet parentheses when `(` already follows ──
                let items = if paren_follows_cursor(content, position) {
                    strip_snippet_parens(items)
                } else {
                    items
                };
                let returned_count = items.len();

                let elapsed = started.elapsed();
                if elapsed >= std::time::Duration::from_millis(20) || !cache_hit {
                    tracing::debug!(
                        target: "performance",
                        "PHPantom: member completion subject={} access={:?} prefix={} cache={} resolved=[{}] took {:?}, returned {}/{} items",
                        target.subject,
                        target.access_kind,
                        prefix,
                        if cache_hit { "hit" } else { "miss" },
                        resolved_types.join(","),
                        elapsed,
                        returned_count,
                        unfiltered_count
                    );
                }

                if is_filtered {
                    Some(CompletionResponse::List(CompletionList {
                        is_incomplete: false,
                        items,
                    }))
                } else {
                    Some(CompletionResponse::Array(items))
                }
            }
            _ => None,
        }
    }

    fn member_completion_cache_key(
        uri: &str,
        target: &CompletionTarget,
        current_class: Option<&ClassInfo>,
        resolved_types: &[String],
    ) -> String {
        format!(
            "{}\n{:?}\n{}\n{}\n{}",
            uri,
            target.access_kind,
            target.subject,
            current_class
                .map(|c| c.fqn().to_string())
                .unwrap_or_default(),
            resolved_types.join(",")
        )
    }

    fn member_completion_prefix(content: &str, position: Position) -> String {
        let cursor_offset = position_to_offset(content, position) as usize;
        let bytes = content.as_bytes();
        let mut start = cursor_offset.min(bytes.len());
        while start > 0 {
            let b = bytes[start - 1];
            if b.is_ascii_alphanumeric() || b == b'_' {
                start -= 1;
            } else {
                break;
            }
        }

        let has_member_operator = (start >= 2
            && ((bytes[start - 2] == b'-' && bytes[start - 1] == b'>')
                || (bytes[start - 2] == b':' && bytes[start - 1] == b':')))
            || (start >= 3
                && bytes[start - 3] == b'?'
                && bytes[start - 2] == b'-'
                && bytes[start - 1] == b'>');
        if !has_member_operator {
            return String::new();
        }

        content[start..cursor_offset.min(content.len())].to_string()
    }

    fn filter_member_completion_items(
        items: Vec<CompletionItem>,
        prefix: &str,
    ) -> Vec<CompletionItem> {
        if prefix.is_empty() {
            return items;
        }

        let prefix_lower = prefix.to_ascii_lowercase();
        let started = std::time::Instant::now();
        let filtered: Vec<CompletionItem> = items
            .into_iter()
            .filter(|item| item.label.to_ascii_lowercase().starts_with(&prefix_lower))
            .collect();
        let elapsed = started.elapsed();
        if elapsed >= std::time::Duration::from_micros(500) {
            tracing::debug!(
                target: "performance",
                "PHPantom: filter_member_completion_items (prefix: {}) took {:?}",
                prefix,
                elapsed
            );
        }
        filtered
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

    // ─── Strategy: class / constant / function completion ────────────────

    /// Build completion item for class keywords (`self`, `static`, `parent`)
    /// in `new` expression contexts.
    ///
    /// When the cursor is inside a class and typing `new s`, these keywords
    /// should be offered alongside regular class names. If the current class
    /// has a constructor, the completion includes parameter snippets.
    fn build_class_keyword_completions(
        &self,
        prefix: &str,
        current_class: Option<&ClassInfo>,
    ) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        let Some(current_class) = current_class else {
            return items;
        };

        let prefix_lower = prefix.to_lowercase();

        for keyword in ["self", "static"] {
            if !keyword.starts_with(&prefix_lower) {
                continue;
            }

            let mut item = CompletionItem {
                label: keyword.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("Instantiate current class".to_string()),
                filter_text: Some(keyword.to_string()),
                sort_text: Some(format!("0_{keyword}")),
                ..CompletionItem::default()
            };

            // Add constructor snippet if available
            if let Some(ctor) = current_class.get_method("__construct") {
                let snippet =
                    crate::completion::builder::build_callable_snippet(keyword, &ctor.parameters);
                item.insert_text = Some(snippet);
                item.insert_text_format = Some(InsertTextFormat::SNIPPET);
            } else {
                item.insert_text = Some(format!("{}()$0", keyword));
                item.insert_text_format = Some(InsertTextFormat::SNIPPET);
            }

            items.push(item);
        }

        // `parent` - reference the parent class
        if "parent".starts_with(&prefix_lower)
            && let Some(parent_name) = &current_class.parent_class
        {
            let mut item = CompletionItem {
                label: "parent".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(format!("Instantiate parent class ({})", parent_name)),
                filter_text: Some("parent".to_string()),
                sort_text: Some("0_parent".to_string()),
                ..CompletionItem::default()
            };

            // Try to load parent class and get its constructor
            if let Some(parent_cls) = self.find_or_load_class(parent_name) {
                if let Some(ctor) = parent_cls.get_method("__construct") {
                    let snippet = crate::completion::builder::build_callable_snippet(
                        "parent",
                        &ctor.parameters,
                    );
                    item.insert_text = Some(snippet);
                    item.insert_text_format = Some(InsertTextFormat::SNIPPET);
                } else {
                    item.insert_text = Some("parent()$0".to_string());
                    item.insert_text_format = Some(InsertTextFormat::SNIPPET);
                }
            } else {
                item.insert_text = Some("parent()$0".to_string());
                item.insert_text_format = Some(InsertTextFormat::SNIPPET);
            }

            items.push(item);
        }

        items
    }

    /// Try to offer class name, constant, and function completions.
    ///
    /// When there is no `->` or `::` operator, check whether the user is
    /// typing a class name, constant, or function name and offer
    /// completions from all known sources (use-imports, same namespace,
    /// stubs, fqn_uri_index, global_defines, stub_constant_index,
    /// global_functions, stub_function_index).
    ///
    /// Returns `None` when the cursor is not at an identifier position or
    /// when no completions could be produced.
    /// Suggest the filename (without `.php` extension) as the class name
    /// when the cursor is inside a class/interface/trait/enum declaration.
    ///
    /// Returns a single completion item so the user can quickly name the
    /// class to match the file, following PSR-4 conventions.
    fn try_class_declaration_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        if !is_class_declaration_name(content, position) {
            return None;
        }

        let name = Self::filename_class_name(uri)?;

        let item = CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some("Match filename".to_string()),
            insert_text: Some(name),
            ..CompletionItem::default()
        };

        Some(CompletionResponse::Array(vec![item]))
    }

    /// Extract the filename without extension from a `file://` URI.
    ///
    /// For example, `file:///home/user/Test.php` returns `Some("Test")`.
    fn filename_class_name(uri: &str) -> Option<String> {
        let url = Url::parse(uri).ok()?;
        let file_path = url.to_file_path().ok()?;
        let stem = file_path.file_stem()?;
        let name = stem.to_string_lossy();
        if name.is_empty() {
            return None;
        }
        Some(name.into_owned())
    }

    fn try_class_constant_function_completion(
        &self,
        content: &str,
        position: Position,
        ctx: &FileContext,
        current_uri: &str,
    ) -> Option<CompletionResponse> {
        if let Some(partial) =
            crate::completion::keyword_completion::enum_backing_type_partial(content, position)
        {
            let items =
                crate::completion::keyword_completion::build_backed_enum_type_completions(&partial);
            if items.is_empty() {
                return None;
            }
            return Some(CompletionResponse::Array(items));
        }

        let class_ctx = detect_class_name_context(content, position);
        let keyword_ctx = {
            let cursor_offset = position_to_offset(content, position);
            let maps = self.symbol_maps.read();
            let map = maps.get(current_uri);
            crate::completion::keyword_completion::build_keyword_context(
                content,
                position,
                cursor_offset,
                map.map(|m| m.as_ref()),
                &ctx.classes,
            )
        };
        let partial = match Self::extract_partial_class_name(content, position) {
            Some(p) => p,
            None => {
                // Allow attribute and namespace-declaration completion on
                // empty prefix (e.g. `#[` or `namespace ` with nothing
                // typed yet).
                if matches!(
                    class_ctx,
                    ClassNameContext::Attribute(_) | ClassNameContext::NamespaceDeclaration
                ) {
                    String::new()
                }
                // Allow keyword completion on empty prefix inside class-like
                // bodies (e.g. after typing `public `).
                else if keyword_ctx.after_member_modifier_chain {
                    let items = crate::completion::keyword_completion::build_keyword_completions(
                        "",
                        class_ctx,
                        keyword_ctx,
                    );
                    if items.is_empty() {
                        return None;
                    }
                    return Some(CompletionResponse::Array(items));
                } else {
                    return None;
                }
            }
        };

        // ── `use function` → only functions ─────────────────────────
        if matches!(class_ctx, ClassNameContext::UseFunction) {
            let (function_items, func_incomplete) = self.build_function_completions(
                &partial,
                true,
                Some(content),
                &ctx.namespace,
                current_uri,
            );
            // Filter out functions defined in the current file.
            let function_items = filter_current_file_functions(function_items, current_uri, self);
            let items = append_semicolon_to_insert_text(function_items);
            return Some(CompletionResponse::List(CompletionList {
                is_incomplete: func_incomplete,
                items,
            }));
        }

        // ── `use const` → only constants ────────────────────────────
        if matches!(class_ctx, ClassNameContext::UseConst) {
            let (constant_items, const_incomplete) =
                self.build_constant_completions(&partial, current_uri, position);
            // Filter out constants defined in the current file.
            let constant_items = filter_current_file_constants(constant_items, current_uri, self);
            let items = append_semicolon_to_insert_text(constant_items);
            return Some(CompletionResponse::List(CompletionList {
                is_incomplete: const_incomplete,
                items,
            }));
        }

        // ── `namespace` declaration → only namespace names ──────────
        if matches!(class_ctx, ClassNameContext::NamespaceDeclaration) {
            let (ns_items, ns_incomplete) =
                self.build_namespace_completions(&partial, position, current_uri);
            return Some(CompletionResponse::List(CompletionList {
                is_incomplete: ns_incomplete,
                items: ns_items,
            }));
        }

        // For `use` imports, pass an empty use_map: the file's own
        // use_map contains the half-typed line (e.g. `use c` → "c")
        // which would appear as a bogus completion item.  Existing
        // imports are irrelevant when writing a new use statement.
        let (use_map_for_completion, affinity_override) =
            if matches!(class_ctx, ClassNameContext::UseImport) {
                // Pass an empty use_map so the half-typed `use` line
                // doesn't appear as a bogus completion item, but build
                // the affinity table from the *real* use-map so that
                // tier-2 candidates are still ranked by namespace affinity.
                let table = crate::completion::class_completion::build_affinity_table(
                    &ctx.use_map,
                    &ctx.namespace,
                );
                (&HashMap::new() as &HashMap<String, String>, Some(table))
            } else {
                (&ctx.use_map, None)
            };

        let (class_items, class_incomplete) =
            self.build_class_name_completions(ClassCompletionParams {
                file_use_map: use_map_for_completion,
                file_namespace: &ctx.namespace,
                prefix: &partial,
                content,
                context: class_ctx,
                position,
                affinity_table_override: affinity_override,
                uri: current_uri,
            });

        // ── `use` (class import) → classes + keyword hints ──────────
        if matches!(class_ctx, ClassNameContext::UseImport) {
            // Filter out classes defined in the current file.
            let class_items = filter_current_file_classes(class_items, ctx);
            // Filter out classes that are already imported via `use`.
            let already_imported: std::collections::HashSet<&str> =
                ctx.use_map.values().map(|v| v.as_str()).collect();
            let class_items: Vec<CompletionItem> = class_items
                .into_iter()
                .filter(|item| {
                    item.detail
                        .as_deref()
                        .is_none_or(|fqn| !already_imported.contains(fqn))
                })
                .collect();
            let mut items = append_semicolon_to_insert_text(class_items);
            // Inject `function` / `const` keyword suggestions when the
            // partial is a case-sensitive prefix of the keyword.  This
            // lets the user type `use f` → select "function" → continue
            // with a function name.
            if "function".starts_with(&partial) {
                items.insert(
                    0,
                    CompletionItem {
                        label: "function".to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        detail: Some("use function import".to_string()),
                        insert_text: Some("function ".to_string()),
                        filter_text: Some("function".to_string()),
                        sort_text: Some("0_!function".to_string()),
                        ..CompletionItem::default()
                    },
                );
            }
            if "const".starts_with(&partial) {
                items.insert(
                    0,
                    CompletionItem {
                        label: "const".to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        detail: Some("use const import".to_string()),
                        insert_text: Some("const ".to_string()),
                        filter_text: Some("const".to_string()),
                        sort_text: Some("0_!const".to_string()),
                        ..CompletionItem::default()
                    },
                );
            }
            return Some(CompletionResponse::List(CompletionList {
                is_incomplete: class_incomplete,
                items,
            }));
        }

        // In restricted contexts (new, extends, implements, use,
        // instanceof), only class names are valid — skip constants
        // and functions.
        if class_ctx.is_class_only() {
            let mut items = if paren_follows_cursor(content, position) {
                strip_snippet_parens(class_items)
            } else {
                class_items
            };

            // For `new` expressions, also offer `self`, `static`, and `parent`
            // keywords when inside a class.
            if class_ctx.is_new() {
                let cursor_offset = position_to_offset(content, position);
                let current_class = find_class_at_offset(&ctx.classes, cursor_offset);
                let keyword_items = self.build_class_keyword_completions(&partial, current_class);
                items.extend(keyword_items);
            }

            if items.is_empty() {
                return None;
            }

            return Some(CompletionResponse::List(CompletionList {
                is_incomplete: class_incomplete,
                items,
            }));
        }

        let keyword_items = crate::completion::keyword_completion::build_keyword_completions(
            &partial,
            class_ctx,
            keyword_ctx,
        );
        let (constant_items, const_incomplete) =
            self.build_constant_completions(&partial, current_uri, position);
        let (function_items, func_incomplete) = self.build_function_completions(
            &partial,
            false,
            Some(content),
            &ctx.namespace,
            current_uri,
        );

        if class_items.is_empty()
            && keyword_items.is_empty()
            && constant_items.is_empty()
            && function_items.is_empty()
        {
            return None;
        }

        let mut items = keyword_items;
        items.extend(class_items);
        items.extend(constant_items);
        items.extend(function_items);

        // Strip snippet parentheses when `(` already follows the cursor
        // (e.g. `array_map|()` or `new ClassName|()`).
        let items = if paren_follows_cursor(content, position) {
            strip_snippet_parens(items)
        } else {
            items
        };

        Some(CompletionResponse::List(CompletionList {
            is_incomplete: class_incomplete || const_incomplete || func_incomplete,
            items,
        }))
    }

    // ─── Shared helpers ─────────────────────────────────────────────────

    /// Insert `);` at the given cursor position in `content`.
    ///
    /// This produces a patched version of the source that the parser can
    /// handle when the user is in the middle of typing a function call
    /// (e.g. `$this->greet(|` where the closing `)` hasn't been typed
    /// yet).  Closing the call expression lets the parser recover the
    /// surrounding class/function structure.
    /// Patch incomplete member-access expressions for parser recovery.
    ///
    /// When the cursor is right after `->` or `?->` and the line has no
    /// semicolon, the PHP parser may fail to recognise the enclosing
    /// statement (e.g. an arrow function body).  This inserts a dummy
    /// identifier and semicolon (`_x;`) at the cursor so the parser can
    /// recover the surrounding structure.
    fn patch_incomplete_member_access(content: &str, position: Position) -> String {
        let line_idx = position.line as usize;
        let col = position.character as usize;
        let mut result = String::with_capacity(content.len() + 4);

        for (i, line) in content.lines().enumerate() {
            if i == line_idx {
                let byte_col = line
                    .char_indices()
                    .nth(col)
                    .map(|(idx, _)| idx)
                    .unwrap_or(line.len());
                // Only patch when the cursor is right after `->` or
                // `?->` with nothing meaningful following it.
                let before = &line[..byte_col];
                let after = line[byte_col..].trim();
                if (before.ends_with("->") || before.ends_with("?->")) && after.is_empty() {
                    result.push_str(before);
                    result.push_str("_x;");
                    result.push_str(&line[byte_col..]);
                } else {
                    result.push_str(line);
                }
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }

        if !content.ends_with('\n') && result.ends_with('\n') {
            result.pop();
        }

        result
    }

    fn patch_content_at_cursor(content: &str, position: Position) -> String {
        let line_idx = position.line as usize;
        let col = position.character as usize;
        let mut result = String::with_capacity(content.len() + 2);

        for (i, line) in content.lines().enumerate() {
            if i == line_idx {
                // Insert `);` at the cursor column
                let byte_col = line
                    .char_indices()
                    .nth(col)
                    .map(|(idx, _)| idx)
                    .unwrap_or(line.len());
                result.push_str(&line[..byte_col]);
                result.push_str(");");
                result.push_str(&line[byte_col..]);
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }

        // Remove the trailing newline we may have added if the original
        // content did not end with one.
        if !content.ends_with('\n') && result.ends_with('\n') {
            result.pop();
        }

        result
    }

    /// Resolve the parameter list for a named-argument completion context.
    ///
    /// Examines the `call_expression` in the context and looks up the
    /// corresponding function or method to extract its parameters.
    ///
    /// Delegates to the shared [`Backend::resolve_callable_target`] and
    /// extracts just the parameters from the result.
    fn resolve_named_arg_params(
        &self,
        ctx: &crate::completion::named_args::NamedArgContext,
        content: &str,
        position: Position,
        file_ctx: &FileContext,
    ) -> Vec<crate::types::ParameterInfo> {
        self.resolve_callable_target(&ctx.call_expression, content, position, file_ctx)
            .map(|r| r.parameters)
            .unwrap_or_default()
    }

    /// Extract a [`CompletionTarget`] from the symbol map's precomputed
    /// `MemberAccess` data.
    ///
    /// Returns `None` when the symbol map has no `MemberAccess` at or
    /// just before the cursor (e.g. the AST is broken at the cursor
    /// position).  The caller should fall back to text-based extraction.
    fn extract_completion_target_from_symbol_map(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<CompletionTarget> {
        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
        let cursor_offset = position_to_offset(content, position);

        // The cursor may be at the end of a partially-typed member name
        // (e.g. `$obj->get|`), so the MemberAccess span may end before
        // the cursor.  Walk backward through identifier characters from
        // the cursor to find where the member name starts, then look up
        // the span that starts at or contains the access operator.
        let bytes = content.as_bytes();
        let mut search_offset = cursor_offset as usize;
        while search_offset > 0 && {
            let b = bytes[search_offset - 1];
            b.is_ascii_alphanumeric() || b == b'_'
        } {
            search_offset -= 1;
        }

        // Check for `->` or `?->` before the member name start
        let has_arrow = search_offset >= 2
            && bytes[search_offset - 2] == b'-'
            && bytes[search_offset - 1] == b'>';
        let has_nullsafe_arrow = search_offset >= 3
            && bytes[search_offset - 3] == b'?'
            && bytes[search_offset - 2] == b'-'
            && bytes[search_offset - 1] == b'>';
        let has_double_colon = search_offset >= 2
            && bytes[search_offset - 2] == b':'
            && bytes[search_offset - 1] == b':';

        if !has_arrow && !has_nullsafe_arrow && !has_double_colon {
            return None;
        }

        // Look up the operator position in the symbol map.  For `->` the
        // span covers the subject + operator + member, so the operator
        // byte is within the span.  We look up a byte inside the
        // operator to find the MemberAccess span.
        let operator_offset = if has_nullsafe_arrow {
            (search_offset - 3) as u32
        } else {
            (search_offset - 2) as u32
        };

        if let Some(span) = map.lookup(operator_offset)
            && let SymbolKind::MemberAccess {
                subject_text,
                is_static,
                ..
            } = &span.kind
        {
            let access_kind = if *is_static {
                crate::AccessKind::DoubleColon
            } else {
                crate::AccessKind::Arrow
            };
            return Some(CompletionTarget {
                access_kind,
                subject: subject_text.clone(),
            });
        }

        None
    }
}
