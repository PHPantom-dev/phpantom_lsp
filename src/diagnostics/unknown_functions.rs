//! Unknown function diagnostics.
//!
//! Walk the precomputed [`SymbolMap`] for a file and flag every
//! `FunctionCall` span (that is not a definition) where the function
//! cannot be resolved through any of PHPantom's resolution phases
//! (use-map → namespace-qualified → global_functions → stubs →
//! autoload files).
//!
//! Diagnostics use `Severity::Error` because calling a function that
//! does not exist crashes at runtime with "Call to undefined function".
//!
//! Suppression rules:
//! - Function *definitions* are skipped (`is_definition: true`).
//! - Calls on `use` statement lines are skipped (import declarations).
//! - PHP built-in language constructs that look like function calls
//!   (`isset`, `unset`, `empty`, `eval`, `exit`, `die`, `list`,
//!   `print`, `echo`, `include`, `require`, etc.) are skipped.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolKind;

use super::existence_guards::compute_existence_guards;
use super::helpers::{FileDiagnosticContext, is_offset_in_ranges, make_diagnostic};
use super::use_statements::compute_use_line_ranges;

/// Diagnostic code used for unknown-function diagnostics.
pub(crate) const UNKNOWN_FUNCTION_CODE: &str = "unknown_function";

/// PHP language constructs that syntactically look like function calls
/// but are not actual functions and should never be flagged.
const LANGUAGE_CONSTRUCTS: &[&str] = &[
    "isset",
    "unset",
    "empty",
    "eval",
    "exit",
    "die",
    "list",
    "print",
    "echo",
    "include",
    "include_once",
    "require",
    "require_once",
    "array",
    "compact",
    "extract",
    "assert",
    "function_exists",
    "class_exists",
    "method_exists",
    "property_exists",
    "defined",
];

impl Backend {
    /// Collect unknown-function diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_unknown_function_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(ctx) = FileDiagnosticContext::gather(self, uri) else {
            return;
        };
        self.collect_unknown_function_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// Same as [`Self::collect_unknown_function_diagnostics`] but reuses
    /// an already-gathered [`FileDiagnosticContext`] instead of
    /// re-reading the per-file locks. Used by `collect_slow_diagnostics`
    /// so all slow collectors in the same pass share one consistent
    /// snapshot.
    pub(crate) fn collect_unknown_function_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let symbol_map = &ctx.symbol_map;
        let file_use_map = &ctx.file.use_map;

        // ── Compute byte ranges of `use` statement lines ────────────────
        let use_line_ranges = compute_use_line_ranges(content);

        // ── Compute existence guards ────────────────────────────────────
        let existence_guards = compute_existence_guards(content);

        // ── Collect local function definition names ─────────────────────
        // Functions defined in the same file are always resolvable even
        // before they appear in global_functions (hoisting).  Collect
        // both short names and FQN forms.
        let local_function_names: Vec<String> = symbol_map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::FunctionCall {
                    name,
                    is_definition: true,
                    ..
                } => {
                    let mut names = vec![name.to_string()];
                    if let Some(ns) = ctx.file.namespace_at(span.start) {
                        names.push(format!("{}\\{}", ns, name));
                    }
                    Some(names)
                }
                _ => None,
            })
            .flatten()
            .collect();

        // ── Walk every symbol span ──────────────────────────────────────
        for span in &symbol_map.spans {
            let (name, is_docblock_reference) = match &span.kind {
                SymbolKind::FunctionCall {
                    name,
                    is_definition: false,
                    is_docblock_reference,
                } => (name, *is_docblock_reference),
                _ => continue,
            };

            // `@see` legally carries URIs, prose, and naming suggestions in
            // addition to FQSENs, so a target that resolves to nothing is
            // not an error.  PHPUnit coverage metadata shares the emitter
            // but clears the flag, so `@covers` is still checked.
            if is_docblock_reference {
                continue;
            }

            // Skip spans on `use` statement lines.
            if is_offset_in_ranges(span.start, &use_line_ranges) {
                continue;
            }

            // Skip PHP language constructs.
            if LANGUAGE_CONSTRUCTS
                .iter()
                .any(|&c| c.eq_ignore_ascii_case(name))
            {
                continue;
            }

            // Skip names that match a local function definition.
            if local_function_names.iter().any(|n| n == name) {
                continue;
            }

            // ── Attempt resolution through all phases ───────────────────
            // The offset-aware variant is what makes a call in the second
            // `namespace` block of a file resolve against that block's
            // namespace rather than the first block's.
            if self
                .resolve_function_name_at(
                    name,
                    ctx.file.resolved_names.as_deref(),
                    span.start,
                    file_use_map,
                    ctx.file.namespace_at(span.start),
                )
                .is_some()
            {
                continue;
            }

            // ── Skip functions guarded by function_exists() ──────────────
            if existence_guards.is_function_guarded(name, span.start) {
                continue;
            }

            // ── Function is unresolved — emit diagnostic ────────────────
            let range = match self.offset_range_to_lsp_range(
                uri,
                content,
                span.start as usize,
                span.end as usize,
            ) {
                Some(r) => r,
                None => continue,
            };

            let message = format!("Function '{}' not found", name);

            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::ERROR,
                UNKNOWN_FUNCTION_CODE,
                message,
            ));
        }
    }
}
