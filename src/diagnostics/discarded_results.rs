//! Discarded `#[\NoDiscard]` result diagnostics.
//!
//! PHP 8.5 warns when a function or method marked `#[\NoDiscard]` is
//! called as a statement of its own, throwing its result away:
//!
//! ```php
//! #[\NoDiscard]
//! function mintId(): int { return 1; }
//!
//! mintId();        // warning: the return value is discarded
//! (void) mintId(); // fine: the documented opt-out
//! ```
//!
//! Only the resolved callee's own attribute counts, as PHP does not
//! inherit it onto an override.  A call whose callee lacks the attribute
//! is never reported, so this is not a generic unused-result lint.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::parser::with_parse_cache;

use super::helpers::{
    CallTargetCache, FileDiagnosticContext, is_position_independent_call_expression,
    make_diagnostic,
};

/// Diagnostic code used for a discarded `#[\NoDiscard]` result.
pub(crate) const DISCARDED_RETURN_VALUE_CODE: &str = "discarded_return_value";

impl Backend {
    /// Collect discarded `#[\NoDiscard]` result diagnostics for a single
    /// file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_discarded_result_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_discarded_result_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// [`Self::collect_discarded_result_diagnostics`] over an
    /// already-gathered [`FileDiagnosticContext`].
    pub(crate) fn collect_discarded_result_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let symbol_map = &ctx.symbol_map;
        if symbol_map.discarded_calls.is_empty() {
            return;
        }
        let file_ctx = &ctx.file;
        let _parse_guard = with_parse_cache(content);
        let call_targets = CallTargetCache::new(file_ctx);

        // Both lists are sorted by `args_start`, so one merge pass pairs
        // each discarded call with its call site.
        let mut discarded = symbol_map.discarded_calls.iter().peekable();
        for call_site in &symbol_map.call_sites {
            while discarded
                .next_if(|&&(_, args_start)| args_start < call_site.args_start)
                .is_some()
            {}
            let Some(&(call_start, _)) =
                discarded.next_if(|&&(_, args_start)| args_start == call_site.args_start)
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
            if !target.no_discard {
                continue;
            }

            let callee = match (&target.owner_class, member_name(expr)) {
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
                DISCARDED_RETURN_VALUE_CODE,
                format!(
                    "The return value of {callee} should either be used or intentionally ignored by casting it as (void)"
                ),
            ));
        }
    }
}

/// The method a `$subject->method` or `Class::method` call expression
/// names.
fn member_name(expr: &str) -> Option<&str> {
    let arrow = expr.rfind("->").map(|i| i + 2);
    let colons = expr.rfind("::").map(|i| i + 2);
    Some(&expr[arrow.max(colons)?..])
}
