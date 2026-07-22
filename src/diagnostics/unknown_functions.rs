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

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolKind;

use super::helpers::{
    compute_existence_guards, compute_use_line_ranges, is_offset_in_ranges, make_diagnostic,
};

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
        // ── Gather context under locks ──────────────────────────────────
        let symbol_map = {
            let maps = self.symbol_maps.read();
            match maps.get(uri) {
                Some(sm) => sm.clone(),
                None => return,
            }
        };

        let file_use_map: HashMap<String, String> = self.file_use_map(uri);

        let file_namespace: Option<String> = self.first_file_namespace(uri);

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
                } => {
                    let mut names = vec![name.clone()];
                    if let Some(ref ns) = file_namespace {
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
            let name = match &span.kind {
                SymbolKind::FunctionCall {
                    name,
                    is_definition: false,
                } => name,
                _ => continue,
            };

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
            if self
                .resolve_function_name(name, &file_use_map, &file_namespace)
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
