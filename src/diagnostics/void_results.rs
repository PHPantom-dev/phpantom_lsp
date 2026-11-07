//! Diagnostics for using the result of a `void` call.
//!
//! A function or method whose native return type is `void` returns
//! nothing, so a caller that reads its result gets `null`:
//!
//! ```php
//! function flush(): void {}
//!
//! flush();             // fine
//! $flushed = flush();  // warning: always null
//! if (flush()) {}      // warning: never true
//! ```
//!
//! Only the native return type counts: a `@return void` over an untyped
//! signature does not stop the body returning a value.  A call whose
//! value goes nowhere (see [`crate::symbol_map::SymbolMap::used_result_calls`])
//! is never reported.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::parser::with_parse_cache;

use super::helpers::{
    CallTargetCache, FileDiagnosticContext, is_position_independent_call_expression,
    make_diagnostic,
};

/// Diagnostic code used for a used `void` result.
pub(crate) const VOID_RESULT_USED_CODE: &str = "void_result_used";

impl Backend {
    /// Collect used `void` result diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_void_result_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_void_result_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// [`Self::collect_void_result_diagnostics`] over an already-gathered
    /// [`FileDiagnosticContext`].
    pub(crate) fn collect_void_result_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let symbol_map = &ctx.symbol_map;
        if symbol_map.used_result_calls.is_empty() {
            return;
        }
        let file_ctx = &ctx.file;
        let _parse_guard = with_parse_cache(content);
        let call_targets = CallTargetCache::new(file_ctx);

        // Both lists are sorted by `args_start`, so one merge pass pairs
        // each used call with its call site.
        let mut used = symbol_map.used_result_calls.iter().peekable();
        for call_site in &symbol_map.call_sites {
            while used
                .next_if(|&&(_, args_start)| args_start < call_site.args_start)
                .is_some()
            {}
            let Some(&(call_start, _)) =
                used.next_if(|&&(_, args_start)| args_start == call_site.args_start)
            else {
                continue;
            };
            if !super::incremental::reports_site(call_site.args_start) {
                continue;
            }

            let expr = call_site.call_expression;
            let target = if is_position_independent_call_expression(expr) {
                call_targets.target(self, expr, content, call_site.args_start, file_ctx)
            } else {
                self.resolve_callable_target_at_offset(
                    expr,
                    content,
                    call_site.args_start,
                    file_ctx,
                )
            };
            let Some(target) = target else {
                continue;
            };
            if !target.native_void {
                continue;
            }

            let callee = match (
                &target.owner_class,
                super::discarded_results::member_name(expr),
            ) {
                (Some(owner), Some(method)) => format!("method {owner}::{method}()"),
                _ => format!("function {}()", expr.trim_start_matches('\\')),
            };
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                call_start as usize,
                call_site.args_end.saturating_add(1) as usize,
            ) else {
                continue;
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Warning,
                VOID_RESULT_USED_CODE,
                format!("Result of {callee} (void) is used"),
            ));
        }
    }
}
