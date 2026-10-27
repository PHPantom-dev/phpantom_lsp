//! Completion request orchestration.
//!
//! Coordinates the various completion strategies (PHPDoc tags, named
//! arguments, array shape keys, member access, variable names,
//! class/constant/function names) and returns the first successful
//! result.
//!
//! The strategies run in stages, each of which either answers the
//! request or hands it on: a Blade template's own source, comments,
//! declarations, string arguments, then code.  Each strategy is a private
//! method, grouped into sibling modules by concern:
//! - `phpdoc` — the comment stage: `complete_phpdoc_tag` (`@tag`
//!   completion inside docblocks), `complete_docblock_type_or_variable`
//!   (type/variable after `@param`, `@return`, etc.) and the
//!   PHPStan-ignore-code completion
//! - `class_constant` — `complete_type_hint` (type completion in
//!   parameter lists, return types, properties) and
//!   `try_class_constant_function_completion` (bare class/constant/
//!   function names, including `new` and `throw new`)
//! - `string_arguments` — the string-argument stage (Laravel string
//!   keys, Eloquent names, array callables, …)
//! - `named_args` — `try_named_arg_completion`-style collection of
//!   `name:` argument completion inside call parens
//! - `member_access` — `try_member_access_completion` (`->` and `::`
//!   member completion) and the related override-suggestion strategy
//! - `patching` — source-text patches shared by `member_access` and
//!   `named_args` to recover a parseable AST mid-keystroke
//!
//! Strategies not big enough to warrant their own module (array shape
//! completion, variable name completion and catch clause completion)
//! stay here alongside the orchestrator.
//!
//! Methods prefixed with `complete_` always short-circuit: the caller
//! unconditionally returns their result.  Methods prefixed with `try_`
//! return `Option<CompletionResponse>` where `None` means "not applicable,
//! try the next strategy."
use std::ops::ControlFlow;

use tower_lsp_server::gen_lsp_types::*;
use tower_lsp_server::jsonrpc::Result;

use crate::Backend;
use crate::completion::class_completion::{ClassCompletionParams, ClassNameContext};
use crate::completion::comment_position::StringContext;
use crate::text_position::position_to_byte_offset;
use crate::types::FileContext;

mod blade_component;
mod blade_directive;
mod class_constant;
mod member_access;
mod named_args;
mod patching;
mod phpdoc;
mod string_arguments;

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
        CompletionResponse::CompletionItemList(mut items) => {
            items.extend(named_arg_items);
            CompletionResponse::CompletionItemList(items)
        }
        CompletionResponse::CompletionList(mut list) => {
            list.items.extend(named_arg_items);
            CompletionResponse::CompletionList(list)
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

/// Give the text every item inserts the file's line terminator `eol`.
fn match_completion_line_endings(response: &mut CompletionResponse, eol: &str) {
    use crate::text_position::match_line_ending;

    if eol == "\n" {
        return;
    }
    let items = match response {
        CompletionResponse::CompletionItemList(items) => items,
        CompletionResponse::CompletionList(list) => &mut list.items,
    };
    for item in items {
        if let Some(text) = &mut item.insert_text {
            match_line_ending(text, eol);
        }
        match &mut item.text_edit {
            Some(CompletionItemTextEdit::TextEdit(edit)) => {
                match_line_ending(&mut edit.new_text, eol)
            }
            Some(CompletionItemTextEdit::InsertReplaceEdit(edit)) => {
                match_line_ending(&mut edit.new_text, eol);
            }
            None => {}
        }
        for edit in item.additional_text_edits.iter_mut().flatten() {
            match_line_ending(&mut edit.new_text, eol);
        }
    }
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
            if item.insert_text_format == Some(InsertTextFormat::Snippet)
                && matches!(
                    item.kind,
                    Some(CompletionItemKind::Method)
                        | Some(CompletionItemKind::Function)
                        | Some(CompletionItemKind::Class)
                )
            {
                // Replace the snippet with just the name
                // (the filter_text already holds it).
                if let Some(ref name) = item.filter_text {
                    item.insert_text = Some(name.clone());
                }
                // Also clear any text_edit that carries the snippet text.
                if let Some(CompletionItemTextEdit::TextEdit(ref mut te)) = item.text_edit
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

/// What one completion stage decided: `Break` answers the request (with
/// nothing when the position takes no completion), `Continue` hands it to
/// the next stage.
type Stage = ControlFlow<Option<CompletionResponse>>;

/// The cursor a completion request asks about, in the content it is
/// analysed as (a Blade template's virtual PHP).
struct CompletionSite<'a> {
    uri: &'a str,
    content: &'a str,
    position: Position,
    cursor_offset: u32,
    /// Per-file context (classes, use-map, namespace) read once at the
    /// cursor offset, so that a file with several namespaces resolves
    /// names against the block the cursor is in.
    ctx: FileContext,
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
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let mut response = self.completion_response(&uri, params);
        if let Some(response) = &mut response
            && let Some(content) = self.get_file_content_arc(&uri)
        {
            match_completion_line_endings(response, crate::text_position::line_ending(&content));
        }
        Ok(response)
    }

    /// The completion items at the request's position, before their edit
    /// text is given the file's line terminator.
    fn completion_response(
        &self,
        uri: &str,
        params: CompletionParams,
    ) -> Option<CompletionResponse> {
        let position = params.text_document_position_params.position;

        if self.is_blade_file(uri)
            && let Some(response) = self.blade_source_completion(uri, position)
        {
            return Some(response);
        }

        let is_template = self.is_twig_template_uri(uri);
        if (is_template || self.is_resource_uri(uri))
            && let Some(content) = self.get_file_content_arc(uri)
            && let Some(response) = self.try_symfony_resource_completion(uri, &content, position)
        {
            return Some(response);
        }
        // A template is not PHP, so nothing below applies to it.
        if is_template {
            return None;
        }

        // A template is analysed as the virtual PHP it lowers to, with the
        // cursor moved into it, so variable resolution walks the
        // preprocessed AST.
        let (content, position) = self.analysable_content_at(uri, position)?;

        let response = {
            let _scope = self.enter_request_scope(uri, std::sync::Arc::clone(&content));

            let cursor_offset = crate::text_position::position_to_offset(&content, position);
            let site = CompletionSite {
                uri,
                content: &content,
                position,
                cursor_offset,
                ctx: self.file_context_at(uri, cursor_offset),
            };
            match self.completion_stages(&site, params.context.as_ref()) {
                ControlFlow::Break(response) => response,
                ControlFlow::Continue(()) => None,
            }
        };

        // A Blade file was swapped to its virtual PHP content and
        // position above, so any item carrying a `TextEdit` names a
        // position in the virtual file, not one the editor's buffer
        // has — translate it back, dropping items whose range falls in
        // the injected prologue rather than clamping them to the
        // template's start.
        match response {
            Some(response) if self.is_blade_file(uri) => {
                Some(self.translate_completion_response(uri, response))
            }
            other => other,
        }
    }

    /// Completion that reads a Blade template's own source at the
    /// untranslated position, before the request is moved into the
    /// template's virtual PHP.
    fn blade_source_completion(&self, uri: &str, position: Position) -> Option<CompletionResponse> {
        // ── Blade directive-name completion ─────────────────────────────
        // An `@` the user is still typing a directive name after doesn't
        // survive Blade preprocessing (an unrecognised directive is masked
        // as inert HTML), so this reads the raw buffer. Always
        // short-circuits once the cursor is confirmed to be in an
        // HTML/directive position — even a prefix matching no known
        // directive returns an empty list rather than falling through to
        // class/variable completion.
        if let Some(prefix) = self.blade_directive_prefix_at(uri, position) {
            return Some(self.complete_blade_directive(&prefix));
        }

        // ── Blade component tag and attribute completion ────────────────
        // A `<x-` the user is still typing a component name after names no
        // component yet, so the preprocessor degrades it to a comment and
        // nothing of it survives into the virtual PHP. Runs after the
        // directive check so a Blade directive written inside a tag
        // (`<x-alert @if(…)`) still completes as one.
        if let Some(response) = self.blade_component_completion(uri, position) {
            return Some(response);
        }

        // ── Blade section / stack name completion ───────────────────────
        // Inside `@yield('|')`, `@section('|')`, `@push('|')` and their
        // helpers, the names come from the templates that render this one
        // (or the ones it renders), and the edit is in Blade coordinates.
        self.blade_block_name_completion(uri, position)
    }

    /// The completion strategies for a position in PHP code, in priority
    /// order.
    fn completion_stages(
        &self,
        site: &CompletionSite,
        trigger: Option<&CompletionContext>,
    ) -> Stage {
        self.comment_completion(site)?;
        self.declaration_completion(site)?;

        // ── Named argument completion (collected, not short-circuited) ──
        // Named arg items are always valid alongside normal
        // completions, so collect them here and merge them into
        // whatever strategy wins below.
        let named_arg_items =
            self.collect_named_arg_items(site.uri, site.content, site.position, &site.ctx);

        // Classify once and use throughout the remaining pipeline.
        let string_ctx = crate::completion::comment_position::classify_string_context(
            site.content,
            site.position,
        );
        if let Some(response) = self.string_argument_completion(site, string_ctx) {
            return ControlFlow::Break(Some(response));
        }
        if matches!(string_ctx, StringContext::InStringLiteral) {
            return ControlFlow::Break(None);
        }
        ControlFlow::Break(self.code_completion(site, string_ctx, trigger, named_arg_items))
    }

    /// Completion at a position in a declaration: a type hint, a new
    /// member in a class body, or the name of a member being declared.
    /// Answers for every such position, even with nothing to offer.
    fn declaration_completion(&self, site: &CompletionSite) -> Stage {
        let CompletionSite {
            uri,
            content,
            position,
            cursor_offset,
            ctx,
        } = site;
        let position = *position;

        // ── Type hint completion in definitions ─────────────────
        if let Some(th_ctx) =
            crate::completion::type_hint_completion::detect_type_hint_context(content, position)
        {
            return ControlFlow::Break(
                self.complete_type_hint(content, &th_ctx, ctx, position, uri),
            );
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
        if let Some(items) = self.try_class_root_member_completion(uri, content, position, ctx) {
            return ControlFlow::Break(
                (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items)),
            );
        }

        if crate::completion::context::override_completion::is_member_declaration_name_position_at_offset(
            content,
            *cursor_offset as usize,
        ) {
            let items = self
                .try_method_override_completion(content, position, ctx)
                .filter(|items| !items.is_empty());
            return ControlFlow::Break(items.map(CompletionResponse::CompletionItemList));
        }

        ControlFlow::Continue(())
    }

    /// Completion for a position in code outside a string literal: member
    /// access, variable names, catch clauses, a class declaration's name,
    /// and class, constant and function names, with the named-argument
    /// items merged into the name completions.
    fn code_completion(
        &self,
        site: &CompletionSite,
        string_ctx: StringContext,
        trigger: Option<&CompletionContext>,
        named_arg_items: Vec<CompletionItem>,
    ) -> Option<CompletionResponse> {
        let CompletionSite {
            uri,
            content,
            position,
            ctx,
            ..
        } = site;
        let position = *position;

        // ── Member access completion (-> or ::) ─────────────────
        if let Some(response) =
            self.try_member_access_completion(uri, content, position, ctx, trigger)
        {
            // In simple interpolation (`"$var->"`), PHP only allows
            // property access — method calls and constants are
            // syntax errors.  Filter to properties only.
            if matches!(string_ctx, StringContext::SimpleInterpolation) {
                let items = match response {
                    CompletionResponse::CompletionItemList(items) => items,
                    CompletionResponse::CompletionList(list) => list.items,
                };
                let filtered = items
                    .into_iter()
                    .filter(|i| i.kind == Some(CompletionItemKind::Property))
                    .collect();
                return Some(CompletionResponse::CompletionItemList(filtered));
            }
            return Some(response);
        }

        // ── Variable name completion ────────────────────────────
        // Placed before the interpolation guard so that `"$`
        // and `"{$` both offer variable suggestions.
        if let Some(response) = self.try_variable_name_completion(content, position, uri) {
            return Some(response);
        }

        // Inside any interpolation context the only useful
        // completions are variable names and member access (handled
        // above).  Suppress the remaining completion strategies so
        // class names, catch clauses, etc. don't leak into strings.
        if matches!(
            string_ctx,
            StringContext::SimpleInterpolation | StringContext::BraceInterpolation
        ) {
            return None;
        }

        // ── Smart catch clause completion ───────────────────────
        if let Some(response) = self.try_catch_completion(content, position, ctx, uri) {
            return Some(merge_named_args_into_response(response, named_arg_items));
        }

        // ── Class declaration name completion ───────────────────
        // When declaring a new class/interface/trait/enum, suggest
        // the filename (without extension) as the class name.
        if let Some(response) = self.try_class_declaration_completion(uri, content, position) {
            return Some(response);
        }

        // ── Class name + constant + function completion ─────────
        if let Some(response) =
            self.try_class_constant_function_completion(content, position, ctx, uri)
        {
            return Some(merge_named_args_into_response(response, named_arg_items));
        }

        // No strategy matched, but we may still have named arg items.
        (!named_arg_items.is_empty())
            .then_some(CompletionResponse::CompletionItemList(named_arg_items))
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
            Some(CompletionResponse::CompletionItemList(items))
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
        let symbol_map = self.symbol_map_for(uri);
        let (var_items, var_incomplete) =
            Self::build_variable_completions(content, &partial, position, symbol_map.as_deref());

        if var_items.is_empty() {
            None
        } else {
            Some(CompletionResponse::CompletionList(CompletionList {
                is_incomplete: var_incomplete,
                items: var_items,
                apply_kind: None,
                item_defaults: None,
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
            return Some(CompletionResponse::CompletionItemList(items));
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
            Some(CompletionResponse::CompletionList(CompletionList {
                is_incomplete: class_incomplete,
                items,
                apply_kind: None,
                item_defaults: None,
            }))
        }
    }
}
