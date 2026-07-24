//! Strategy: `name:` argument completion inside call parentheses.
//!
//! Collected (not short-circuited): named-arg items are always valid
//! alongside whatever other completion strategy wins, so
//! [`Backend::collect_named_arg_items`] is called up front by
//! [`super::handle_completion`] and merged into the final response.

use std::sync::Arc;

use tower_lsp::lsp_types::{CompletionItem, Position};

use crate::Backend;
use crate::completion::named_args::{
    NamedArgContext, cursor_inside_nested_bracket, parse_existing_args,
};
use crate::text_position::position_to_offset;
use crate::types::FileContext;

impl Backend {
    /// Collect `name:` argument completion items inside function/method
    /// call parentheses.
    ///
    /// Returns an empty `Vec` when the cursor is not in a named-argument
    /// context or when no parameters could be resolved.  The items are
    /// meant to be **merged** into whatever other completion strategy
    /// wins — named args are always valid alongside normal completions.
    pub(super) fn collect_named_arg_items(
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

    /// Resolve the parameter list for a named-argument completion context.
    ///
    /// Examines the `call_expression` in the context and looks up the
    /// corresponding function or method to extract its parameters.
    ///
    /// Delegates to the shared [`Backend::resolve_callable_target`] and
    /// extracts just the parameters from the result.
    fn resolve_named_arg_params(
        &self,
        ctx: &NamedArgContext,
        content: &str,
        position: Position,
        file_ctx: &FileContext,
    ) -> Vec<crate::types::ParameterInfo> {
        self.resolve_callable_target(&ctx.call_expression, content, position, file_ctx)
            .map(|r| r.parameters)
            .unwrap_or_default()
    }
}
