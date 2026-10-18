//! Remove unused import code action.
//!
//! When the cursor overlaps with an unused `use` statement (identified by
//! matching diagnostics with `DiagnosticTag::Unnecessary`), offer:
//!
//! 1. A per-import quick-fix: `Remove unused import 'Foo\Bar'`
//! 2. A bulk action: `Remove all unused imports` (when ≥ 2 unused imports exist)
//!
//! The detection reuses the same logic as `diagnostics::unused_imports` —
//! we collect unused-import diagnostics and then generate `TextEdit`s that
//! delete the corresponding lines.
//!
//! ## Deferred edit computation
//!
//! Both actions use the two-phase `codeAction/resolve` model.  Phase 1
//! returns a lightweight stub with the diagnostic(s) attached; Phase 2
//! recomputes the deletion edits when the user picks the action.
//! On resolve the matched diagnostics are eagerly removed from the
//! published set so the squiggly lines disappear before the text edit
//! is applied.

use std::cmp::Reverse;
use std::collections::HashSet;

use tower_lsp::lsp_types::*;

use super::use_line_deletion::{build_line_deletion_edit, range_touches_use_import};
use super::{CodeActionData, make_code_action_data};
use crate::Backend;
use crate::text_position::ranges_overlap;

impl Backend {
    /// Collect "Remove unused import" code actions.
    ///
    /// For each unused-import diagnostic that overlaps with the request
    /// range, offer a quick-fix to remove it.  When there are two or more
    /// unused imports in the file, also offer a bulk "Remove all unused
    /// imports" action.
    ///
    /// Phase 1 only — edits are deferred to [`resolve_remove_unused_import`].
    pub(crate) fn collect_remove_unused_import_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        // ── Collect all unused-import diagnostics for this file ─────────
        let mut all_unused_diags: Vec<Diagnostic> = Vec::new();
        self.collect_unused_import_diagnostics(uri, content, &mut all_unused_diags);

        if all_unused_diags.is_empty() {
            return;
        }

        // ── Find diagnostics that overlap with the request range ────────
        let overlapping: Vec<&Diagnostic> = all_unused_diags
            .iter()
            .filter(|d| ranges_overlap(&d.range, &params.range))
            .collect();

        for diag in &overlapping {
            let title = format!(
                "Remove {}",
                diag.message
                    .strip_prefix("Unused import ")
                    .map(|rest| format!("unused import {rest}"))
                    .unwrap_or_else(|| "unused import".to_string())
            );

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![(*diag).clone()]),
                edit: None,
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: Some(make_code_action_data(
                    "quickfix.removeUnusedImport",
                    uri,
                    &params.range,
                    serde_json::json!({}),
                )),
            }));
        }

        // ── Bulk action: remove unused imports ──────────────────────────
        // Only offer when the request touches a namespace-level `use`
        // import (used or unused), so it doesn't pop up on unrelated
        // lines elsewhere in the file.
        if range_touches_use_import(content, &params.range) {
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Remove all unused imports".to_string(),
                kind: Some(CodeActionKind::new("source.organizeImports")),
                diagnostics: Some(all_unused_diags),
                edit: None,
                command: None,
                is_preferred: None,
                disabled: None,
                data: Some(make_code_action_data(
                    "quickfix.removeAllUnusedImports",
                    uri,
                    &params.range,
                    serde_json::json!({}),
                )),
            }));
        }
    }

    /// Resolve a deferred "Remove unused import" or "Remove all unused
    /// imports" code action.
    ///
    /// Recomputes the deletion edits from the diagnostics attached to
    /// the action.  Each diagnostic's range identifies the `use`
    /// statement to remove.
    pub(crate) fn resolve_remove_unused_import(
        &self,
        data: &CodeActionData,
        content: &str,
        diagnostics: Option<&[Diagnostic]>,
    ) -> Option<WorkspaceEdit> {
        let doc_uri: Url = data.uri.parse().ok()?;
        let diags = diagnostics?;

        if diags.is_empty() {
            return None;
        }

        let is_bulk = data.action_kind == "quickfix.removeAllUnusedImports";

        if is_bulk {
            // For the bulk action, recompute all unused-import
            // diagnostics from the current content (the set may have
            // changed since Phase 1).
            let mut fresh_diags: Vec<Diagnostic> = Vec::new();
            self.collect_unused_import_diagnostics(&data.uri, content, &mut fresh_diags);

            if fresh_diags.is_empty() {
                return None;
            }

            let removed_import_lines: HashSet<usize> = fresh_diags
                .iter()
                .map(|d| d.range.start.line as usize)
                .collect();
            let all_ranges: Vec<Range> = fresh_diags.iter().map(|d| d.range).collect();

            let mut edits: Vec<TextEdit> = fresh_diags
                .iter()
                .map(|d| {
                    build_line_deletion_edit(content, &d.range, &removed_import_lines, &all_ranges)
                })
                .collect();

            // Sort edits in reverse order so that byte offsets remain
            // valid as we apply deletions from bottom to top, and drop
            // duplicate edits produced when several diagnostics collapse
            // to one whole-group-statement removal.
            edits.sort_by_key(|e| Reverse(e.range.start));
            edits.dedup_by(|a, b| a.range == b.range);

            Some(crate::code_actions::single_file_edit(doc_uri, edits))
        } else {
            let diag = &diags[0];
            let removed_import_lines = HashSet::from([diag.range.start.line as usize]);
            let removal_edit = build_line_deletion_edit(
                content,
                &diag.range,
                &removed_import_lines,
                std::slice::from_ref(&diag.range),
            );

            Some(crate::code_actions::single_file_edit(
                doc_uri,
                vec![removal_edit],
            ))
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Range helpers ───────────────────────────────────────────────

    #[test]
    fn overlapping_ranges() {
        let a = Range::new(Position::new(1, 0), Position::new(1, 10));
        let b = Range::new(Position::new(1, 5), Position::new(1, 15));
        assert!(ranges_overlap(&a, &b));
    }

    #[test]
    fn non_overlapping_ranges() {
        let a = Range::new(Position::new(1, 0), Position::new(1, 5));
        let b = Range::new(Position::new(2, 0), Position::new(2, 5));
        assert!(!ranges_overlap(&a, &b));
    }

    #[test]
    fn touching_ranges_do_not_overlap() {
        let a = Range::new(Position::new(1, 0), Position::new(1, 5));
        let b = Range::new(Position::new(1, 5), Position::new(1, 10));
        assert!(!ranges_overlap(&a, &b));
    }

    #[test]
    fn cursor_inside_range() {
        let a = Range::new(Position::new(3, 0), Position::new(3, 20));
        let b = Range::new(Position::new(3, 10), Position::new(3, 10)); // cursor
        assert!(ranges_overlap(&a, &b));
    }
}
