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
use std::collections::{HashMap, HashSet};

use tower_lsp::lsp_types::*;

use super::{CodeActionData, make_code_action_data};
use crate::Backend;
use crate::util::{line_start_byte_offset, offset_to_position, ranges_overlap};

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
        // Only offer when the cursor is on any namespace-level `use`
        // import line (used or unused), so it doesn't pop up on
        // unrelated lines elsewhere in the file.
        if !all_unused_diags.is_empty()
            && cursor_on_use_import_line(content, params.range.start.line)
        {
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

            let mut edits: Vec<TextEdit> = fresh_diags
                .iter()
                .map(|d| build_line_deletion_edit(content, &d.range, &removed_import_lines))
                .collect();

            // Sort edits in reverse order so that byte offsets remain
            // valid as we apply deletions from bottom to top.
            edits.sort_by_key(|e| Reverse(e.range.start));

            let mut changes = HashMap::new();
            changes.insert(doc_uri, edits);

            Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            })
        } else {
            // Single-import removal: use the diagnostic range from the
            // action to build the deletion edit.
            let diag = &diags[0];
            let removed_import_lines = HashSet::from([diag.range.start.line as usize]);
            let removal_edit =
                build_line_deletion_edit(content, &diag.range, &removed_import_lines);

            let mut changes = HashMap::new();
            changes.insert(doc_uri, vec![removal_edit]);

            Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            })
        }
    }
}

/// Check whether the cursor line is a namespace-level `use` import line.
///
/// Returns `true` when the line starts with `use ` (after optional
/// whitespace) and is NOT inside a class/trait body (where `use` means
/// a trait import, not a namespace import).
fn cursor_on_use_import_line(content: &str, line: u32) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    let idx = line as usize;
    if idx >= lines.len() {
        return false;
    }

    let trimmed = lines[idx].trim();
    if !trimmed.starts_with("use ") {
        return false;
    }

    // Heuristic: if we're inside a class/trait/enum body, this is a
    // trait `use`, not a namespace import.  We track brace depth and
    // account for braced `namespace Foo { … }` blocks (where depth 1
    // is still "top level" for import purposes).
    let mut depth: usize = 0;
    let mut namespace_brace_depth: Option<usize> = None;
    for l in &lines[..idx] {
        let code = l.split("//").next().unwrap_or(l);
        let code = code.split('#').next().unwrap_or(code);
        let ltrimmed = l.trim_start();

        if ltrimmed.starts_with("namespace ") && code.contains('{') {
            namespace_brace_depth = Some(depth);
        }

        for ch in code.chars() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if namespace_brace_depth == Some(depth) {
                        namespace_brace_depth = None;
                    }
                }
                _ => {}
            }
        }
    }

    let top_level_depth = namespace_brace_depth.map_or(0, |d| d + 1);
    depth <= top_level_depth
}

/// Build a `TextEdit` that deletes the full line(s) covered by `range`,
/// including the trailing newline.
///
/// When the diagnostic targets a single member inside a group `use`
/// statement (e.g. `use Foo\{Bar, Baz};` where only `Bar` is unused),
/// the edit removes just the member entry rather than the whole line.
pub(crate) fn build_line_deletion_edit(
    content: &str,
    range: &Range,
    removed_import_lines: &HashSet<usize>,
) -> TextEdit {
    // Try to extend the range to cover the full group member first.
    if let Some(edit) = extend_range_for_group_member(content, range) {
        return edit;
    }

    let lines: Vec<&str> = content.lines().collect();
    let start_line = range.start.line as usize;
    let end_line = range.end.line as usize;

    // Compute line-start offsets from real terminator lengths so the edit
    // stays aligned on CRLF files (where `str::lines()` strips the `\r`).
    let edit_start_offset = if should_consume_previous_blank_line(
        lines.as_slice(),
        start_line,
        end_line,
        removed_import_lines,
    ) {
        line_start_byte_offset(content, start_line - 1)
    } else {
        line_start_byte_offset(content, start_line)
    };

    // Deleting through the start of the line after `end_line` consumes
    // `end_line`'s terminator. Optionally extend over a following blank
    // line as well.
    let last_consumed_line = if should_consume_following_blank_line(
        lines.as_slice(),
        start_line,
        end_line,
        removed_import_lines,
    ) {
        end_line + 1
    } else {
        end_line
    };
    let end_offset = line_start_byte_offset(content, last_consumed_line + 1).min(content.len());

    let start_pos = offset_to_position(content, edit_start_offset);
    let end_pos = offset_to_position(content, end_offset);

    TextEdit {
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        new_text: String::new(),
    }
}

/// Check whether the blank line following the deleted range should also
/// be consumed.  This is true when:
/// - There IS a blank line immediately after `end_line`.
/// - AND either there is a surviving import after the gap (we're
///   collapsing a gap between two import groups) OR there is no
///   surviving import before the deletion (the entire leading block is
///   being removed, so the separator to the class body should go too).
pub(crate) fn should_consume_following_blank_line(
    lines: &[&str],
    start_line: usize,
    end_line: usize,
    removed_import_lines: &HashSet<usize>,
) -> bool {
    if !matches!(lines.get(end_line + 1), Some(line) if line.trim().is_empty()) {
        return false;
    }

    nearest_surviving_import_line(lines, end_line as isize + 2, 1, removed_import_lines).is_some()
        || nearest_surviving_import_line(lines, start_line as isize - 1, -1, removed_import_lines)
            .is_none()
}

/// Check whether the blank line preceding the deleted range should also
/// be consumed.  This is true when:
/// - `start_line` is not the first line.
/// - There IS a blank line immediately before `start_line`.
/// - AND there are surviving imports on BOTH sides (we're collapsing a
///   gap that would otherwise be doubled).
pub(crate) fn should_consume_previous_blank_line(
    lines: &[&str],
    start_line: usize,
    end_line: usize,
    removed_import_lines: &HashSet<usize>,
) -> bool {
    if start_line == 0 {
        return false;
    }

    if !matches!(lines.get(start_line - 1), Some(line) if line.trim().is_empty()) {
        return false;
    }

    nearest_surviving_import_line(lines, start_line as isize - 2, -1, removed_import_lines)
        .is_some()
        && nearest_surviving_import_line(lines, end_line as isize + 1, 1, removed_import_lines)
            .is_some()
}

/// Walk lines from `line` in `direction` (+1 or -1) looking for the
/// nearest `use` import line that is NOT in `removed_import_lines`.
/// Blank lines are skipped; any non-blank, non-`use` line stops the
/// search and returns `None`.
pub(crate) fn nearest_surviving_import_line(
    lines: &[&str],
    mut line: isize,
    direction: isize,
    removed_import_lines: &HashSet<usize>,
) -> Option<usize> {
    while let Some(current) = usize::try_from(line).ok().and_then(|idx| lines.get(idx)) {
        let trimmed = current.trim();

        if trimmed.is_empty() {
            line += direction;
            continue;
        }

        if trimmed.starts_with("use ") {
            let idx = usize::try_from(line).ok()?;
            if !removed_import_lines.contains(&idx) {
                return Some(idx);
            }

            line += direction;
            continue;
        }

        return None;
    }

    None
}

/// When the diagnostic range falls inside a group `use` statement
/// (`use Foo\{Bar, Baz};`), build an edit that removes only the
/// identified member rather than the entire line.
pub(crate) fn extend_range_for_group_member(content: &str, range: &Range) -> Option<TextEdit> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = range.start.line as usize;
    if line_idx >= lines.len() {
        return None;
    }

    // Check if any line in the vicinity contains `{` and `}` — the
    // hallmark of a group use statement.
    let line = lines[line_idx];
    let full_stmt = if line.contains('{') && line.contains('}') {
        line.to_string()
    } else {
        // Multi-line group: gather all lines from the `use` to the `};`
        let mut start = line_idx;
        while start > 0 && !lines[start].trim_start().starts_with("use ") {
            start -= 1;
        }
        // The opening `use ... {` line must contain a `{`.  If the
        // `use` line we found doesn't have one, this isn't a group
        // import at all.
        if !lines[start].contains('{') {
            return None;
        }
        let mut end = line_idx;
        while end < lines.len() && !lines[end].contains('}') {
            end += 1;
        }
        if end >= lines.len() {
            return None;
        }
        lines[start..=end].join("\n")
    };

    // Must have both `{` and `}` to be a group use.
    if !full_stmt.contains('{') || !full_stmt.contains('}') {
        return None;
    }

    // Locate the member text from the diagnostic range. The range columns
    // are UTF-16 code units; convert them to byte offsets before slicing
    // `line` (and convert back to UTF-16 columns for the resulting edit).
    let start_byte = crate::util::utf16_col_to_byte_offset(line, range.start.character);
    let end_byte = crate::util::utf16_col_to_byte_offset(line, range.end.character);
    if end_byte > line.len() || start_byte >= end_byte {
        return None;
    }

    let member_text = &line[start_byte..end_byte];

    // Find this member in the line and determine whether to remove
    // a leading or trailing comma.
    let member_start_in_line = start_byte;

    // Look for a trailing comma+whitespace to consume.
    let after_member = &line[end_byte..];
    let (removal_end, _has_trailing_comma) = if let Some(rest) = after_member.strip_prefix(',') {
        let skip = 1 + rest.len() - rest.trim_start().len();
        (end_byte + skip, true)
    } else {
        (end_byte, false)
    };

    // If no trailing comma, look for a leading comma+whitespace.
    let before_member = &line[..member_start_in_line];
    let removal_start = if removal_end == end_byte {
        // No trailing comma — remove leading comma.
        let trimmed = before_member.trim_end();
        if trimmed.ends_with(',') {
            trimmed.len() - 1
        } else {
            member_start_in_line
        }
    } else {
        member_start_in_line
    };

    // Check if removing this member would leave the group empty.
    // If so, fall back to removing the entire line.
    let brace_start = full_stmt.find('{')?;
    let brace_end = full_stmt.find('}')?;
    let members_text = &full_stmt[brace_start + 1..brace_end];
    let member_count = members_text
        .split(',')
        .filter(|m| !m.trim().is_empty())
        .count();
    if member_count <= 1 {
        return None; // Fall through to full-line deletion.
    }

    // Verify the member text is plausible (non-empty).
    if member_text.trim().is_empty() {
        return None;
    }

    let start_pos = Position::new(
        range.start.line,
        crate::util::byte_offset_to_utf16_col(line, removal_start),
    );
    let end_pos = Position::new(
        range.start.line,
        crate::util::byte_offset_to_utf16_col(line, removal_end),
    );

    Some(TextEdit {
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        new_text: String::new(),
    })
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::position_to_byte_offset;

    fn lsp_position_to_byte_offset(content: &str, pos: Position) -> usize {
        position_to_byte_offset(content, pos)
    }

    /// Test-only wrapper: builds a deletion edit treating only the
    /// diagnostic's own line as removed (single-import scenario).
    fn build_single_line_deletion_edit(content: &str, range: &Range) -> TextEdit {
        let removed = HashSet::from([range.start.line as usize]);
        build_line_deletion_edit(content, range, &removed)
    }

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

    // ── Line deletion ───────────────────────────────────────────────

    #[test]
    fn deletes_full_use_line() {
        let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n";
        let range = Range::new(Position::new(1, 4), Position::new(1, 11));
        let edit = build_single_line_deletion_edit(content, &range);
        // Should delete the entire "use Foo\Bar;\n" line.
        let start = lsp_position_to_byte_offset(content, edit.range.start);
        let end = lsp_position_to_byte_offset(content, edit.range.end);
        assert_eq!(&content[start..end], "use Foo\\Bar;\n");
    }

    #[test]
    fn deletes_full_use_line_crlf() {
        // On a CRLF file the deletion must still cover the entire
        // `use Foo\Bar;\r\n` line including its two-byte terminator,
        // not drift one byte short.
        let content = "<?php\r\nuse Foo\\Bar;\r\nuse Baz\\Qux;\r\n";
        let range = Range::new(Position::new(1, 4), Position::new(1, 11));
        let edit = build_single_line_deletion_edit(content, &range);
        let start = lsp_position_to_byte_offset(content, edit.range.start);
        let end = lsp_position_to_byte_offset(content, edit.range.end);
        assert_eq!(&content[start..end], "use Foo\\Bar;\r\n");
    }

    #[test]
    fn deletes_use_line_and_separator_when_last_import_removed() {
        let content = "<?php\nuse Foo\\Bar;\n\nclass Test {}\n";
        let range = Range::new(Position::new(1, 4), Position::new(1, 11));
        let edit = build_single_line_deletion_edit(content, &range);
        let start = lsp_position_to_byte_offset(content, edit.range.start);
        let end = lsp_position_to_byte_offset(content, edit.range.end);
        assert_eq!(&content[start..end], "use Foo\\Bar;\n\n");
    }

    #[test]
    fn keeps_separator_when_other_imports_remain() {
        let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass Test extends Qux {}\n";
        let range = Range::new(Position::new(2, 4), Position::new(2, 11));
        let edit = build_single_line_deletion_edit(content, &range);
        let start = lsp_position_to_byte_offset(content, edit.range.start);
        let end = lsp_position_to_byte_offset(content, edit.range.end);
        assert_eq!(&content[start..end], "use Baz\\Qux;\n");
    }

    #[test]
    fn removes_following_blank_line_between_remaining_imports() {
        let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nuse Quux\\Quuz;\n";
        let range = Range::new(Position::new(2, 4), Position::new(2, 11));
        let edit = build_single_line_deletion_edit(content, &range);
        let start = lsp_position_to_byte_offset(content, edit.range.start);
        let end = lsp_position_to_byte_offset(content, edit.range.end);
        assert_eq!(&content[start..end], "use Baz\\Qux;\n\n");
    }

    #[test]
    fn removes_previous_blank_line_between_remaining_imports() {
        let content = "<?php\nuse Foo\\Bar;\n\nuse Baz\\Qux;\nuse Quux\\Quuz;\n";
        let range = Range::new(Position::new(3, 4), Position::new(3, 11));
        let edit = build_single_line_deletion_edit(content, &range);
        let start = lsp_position_to_byte_offset(content, edit.range.start);
        let end = lsp_position_to_byte_offset(content, edit.range.end);

        let mut result = content.to_string();
        result.replace_range(start..end, &edit.new_text);

        assert_eq!(result, "<?php\nuse Foo\\Bar;\nuse Quux\\Quuz;\n");
    }

    #[test]
    fn deletes_partial_group_member_trailing_comma() {
        let content = "<?php\nuse Foo\\{Bar, Baz, Qux};\n";
        // Diagnostic covers "Bar" (start col 9, end col 12).
        let range = Range::new(Position::new(1, 9), Position::new(1, 12));
        let edit = extend_range_for_group_member(content, &range);
        assert!(edit.is_some(), "should produce a group member edit");
        let edit = edit.unwrap();
        // Should remove "Bar, " (the member plus the trailing comma+space).
        assert_eq!(edit.new_text, "");
    }

    // ── Code action offering ────────────────────────────────────────

    #[test]
    fn remove_action_offered_for_unused_import() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass Test extends Qux {}\n";

        backend.update_ast(uri, content);

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(1, 4),
                end: Position::new(1, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let remove_action = actions.iter().find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.starts_with("Remove unused import"),
            _ => false,
        });

        assert!(
            remove_action.is_some(),
            "should offer 'Remove unused import' action"
        );
    }

    #[test]
    fn no_remove_action_for_used_import() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse Foo\\Bar;\n\nclass Test extends Bar {}\n";

        backend.update_ast(uri, content);

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(1, 4),
                end: Position::new(1, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let remove_action = actions.iter().find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.starts_with("Remove unused import"),
            _ => false,
        });

        assert!(
            remove_action.is_none(),
            "should NOT offer remove action for used import"
        );
    }

    #[test]
    fn bulk_remove_offered_when_multiple_unused() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n";

        backend.update_ast(uri, content);

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(1, 4),
                end: Position::new(1, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let bulk = actions.iter().find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title == "Remove all unused imports",
            _ => false,
        });

        assert!(
            bulk.is_some(),
            "should offer 'Remove all unused imports' when multiple unused"
        );
    }

    #[test]
    fn bulk_remove_offered_for_single_unused_import() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse Foo\\Bar;\n\nclass Test {}\n";

        backend.update_ast(uri, content);

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(1, 4),
                end: Position::new(1, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let bulk = actions.iter().find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title == "Remove all unused imports",
            _ => false,
        });

        assert!(
            bulk.is_some(),
            "should offer 'Remove all unused imports' even for a single unused import"
        );
    }

    #[test]
    fn bulk_remove_not_offered_when_cursor_outside_import_block() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse Foo\\Bar;\n\nclass Test {}\n";

        backend.update_ast(uri, content);

        // Cursor on "class Test" line, not on a `use` line.
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(3, 0),
                end: Position::new(3, 0),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let bulk = actions.iter().find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title == "Remove all unused imports",
            _ => false,
        });

        let single = actions.iter().find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title.starts_with("Remove unused import"),
            _ => false,
        });

        assert!(
            bulk.is_none(),
            "should NOT offer bulk remove when cursor is not on a use line"
        );
        assert!(
            single.is_none(),
            "should NOT offer single remove when cursor is not on the unused import"
        );
    }

    #[test]
    fn bulk_remove_offered_when_cursor_on_used_import() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass Test extends Qux {}\n";

        backend.update_ast(uri, content);

        // Cursor on the used import (Baz\Qux), not the unused one.
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(2, 4),
                end: Position::new(2, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let bulk = actions.iter().find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title == "Remove all unused imports",
            _ => false,
        });

        assert!(
            bulk.is_some(),
            "should offer bulk remove when cursor is on any use line"
        );
    }

    // ── cursor_on_use_import_line ────────────────────────────────────

    #[test]
    fn cursor_on_use_line_returns_true() {
        let content = "<?php\nuse Foo\\Bar;\nclass Test {}\n";
        assert!(cursor_on_use_import_line(content, 1));
    }

    #[test]
    fn cursor_on_non_use_line_returns_false() {
        let content = "<?php\nuse Foo\\Bar;\nclass Test {\n    public function foo() {}\n}\n";
        assert!(!cursor_on_use_import_line(content, 2)); // class line
        assert!(!cursor_on_use_import_line(content, 3)); // method line
    }

    #[test]
    fn cursor_on_trait_use_returns_false() {
        let content = "<?php\nclass Foo {\n    use SomeTrait;\n}\n";
        assert!(!cursor_on_use_import_line(content, 2));
    }

    #[test]
    fn cursor_on_use_in_braced_namespace_returns_true() {
        let content = "<?php\nnamespace App {\n    use Foo\\Bar;\n}\n";
        // Brace depth at line 2 is 1 (opened by namespace), but
        // namespace braces are tracked separately so depth 1 inside a
        // braced namespace is still "top level" for import purposes.
        assert!(cursor_on_use_import_line(content, 2));
    }

    #[test]
    fn bulk_remove_deletes_both_widely_separated_unused_imports() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "\
<?php

use App\\UnusedA;
use App\\UsedB;

class Foo extends UsedB
{
    public function bar(): void
    {
        // some code
    }
}

use App\\UnusedC;
";

        backend.update_ast(uri, content);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

        // Cursor on the first use line
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(2, 4),
                end: Position::new(2, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let bulk = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title == "Remove all unused imports" => {
                    Some(ca)
                }
                _ => None,
            })
            .expect("should offer bulk remove");

        // Phase 1: no edit, has data.
        assert!(bulk.edit.is_none(), "Phase 1 should not have an edit");
        assert!(bulk.data.is_some(), "Phase 1 should have data");

        // Phase 2: resolve.
        let (resolved, _) = backend.resolve_code_action(bulk.clone());
        let edit = resolved
            .edit
            .as_ref()
            .expect("resolve should produce an edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();

        // Should have edits for both unused imports (UnusedA and UnusedC).
        assert!(
            edits.len() >= 2,
            "should delete both unused imports, got {} edits",
            edits.len()
        );
    }

    #[test]
    fn bulk_remove_in_braced_namespace_with_class_bodies_between() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "\
<?php
use App\\UnusedAlpha;
use App\\UsedBravo;
use App\\UnusedCharlie;

class Demo extends UsedBravo
{
    public function method(): void
    {
    }
}
";

        backend.update_ast(uri, content);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

        // Cursor on the first use line
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(1, 4),
                end: Position::new(1, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let bulk = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title == "Remove all unused imports" => {
                    Some(ca)
                }
                _ => None,
            })
            .expect("should offer bulk remove");

        // Phase 2: resolve.
        let (resolved, _) = backend.resolve_code_action(bulk.clone());
        let edit = resolved
            .edit
            .as_ref()
            .expect("resolve should produce an edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();

        // Should have edits for both unused imports.
        assert!(
            edits.len() >= 2,
            "should delete both unused imports, got {} edits",
            edits.len()
        );

        // Apply the edits to verify the result.
        let mut result = content.to_string();
        let mut sorted: Vec<&TextEdit> = edits.clone();
        sorted.sort_by(|a, b| {
            b.range
                .start
                .line
                .cmp(&a.range.start.line)
                .then(b.range.start.character.cmp(&a.range.start.character))
        });
        for edit in sorted {
            let start = lsp_position_to_byte_offset(&result, edit.range.start);
            let end = lsp_position_to_byte_offset(&result, edit.range.end);
            result.replace_range(start..end, &edit.new_text);
        }

        assert!(
            !result.contains("UnusedAlpha"),
            "UnusedAlpha should be removed:\n{result}"
        );
        assert!(
            !result.contains("UnusedCharlie"),
            "UnusedCharlie should be removed:\n{result}"
        );
        assert!(
            result.contains("UsedBravo"),
            "UsedBravo should be kept:\n{result}"
        );
    }

    #[test]
    fn bulk_remove_consumes_separator_when_import_block_becomes_empty() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass Test {}\n";

        backend.update_ast(uri, content);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(1, 4),
                end: Position::new(1, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let bulk = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title == "Remove all unused imports" => {
                    Some(ca)
                }
                _ => None,
            })
            .expect("should offer bulk remove");

        let (resolved, _) = backend.resolve_code_action(bulk.clone());
        let edit = resolved
            .edit
            .as_ref()
            .expect("resolve should produce an edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();

        let mut result = content.to_string();
        let mut sorted: Vec<&TextEdit> = edits.clone();
        sorted.sort_by(|a, b| {
            b.range
                .start
                .line
                .cmp(&a.range.start.line)
                .then(b.range.start.character.cmp(&a.range.start.character))
        });
        for edit in sorted {
            let start = lsp_position_to_byte_offset(&result, edit.range.start);
            let end = lsp_position_to_byte_offset(&result, edit.range.end);
            result.replace_range(start..end, &edit.new_text);
        }

        assert_eq!(result, "<?php\nclass Test {}\n");
    }

    #[test]
    fn bulk_remove_collapses_gap_when_unused_import_is_between_used_ones() {
        let backend = crate::Backend::new_test();
        let uri = "file:///test.php";
        let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nuse Quux\\Quuz;\n\nclass Test extends Bar\n{\n    public function make(): Quuz\n    {\n        return new Quuz();\n    }\n}\n";

        backend.update_ast(uri, content);
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

        let params = CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: uri.parse().unwrap(),
            },
            range: Range {
                start: Position::new(2, 4),
                end: Position::new(2, 4),
            },
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let actions = backend.handle_code_action(uri, content, &params);
        let bulk = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) if ca.title == "Remove all unused imports" => {
                    Some(ca)
                }
                _ => None,
            })
            .expect("should offer bulk remove");

        let (resolved, _) = backend.resolve_code_action(bulk.clone());
        let edit = resolved
            .edit
            .as_ref()
            .expect("resolve should produce an edit");
        let changes = edit.changes.as_ref().unwrap();
        let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();

        let mut result = content.to_string();
        let mut sorted: Vec<&TextEdit> = edits.clone();
        sorted.sort_by(|a, b| {
            b.range
                .start
                .line
                .cmp(&a.range.start.line)
                .then(b.range.start.character.cmp(&a.range.start.character))
        });
        for edit in sorted {
            let start = lsp_position_to_byte_offset(&result, edit.range.start);
            let end = lsp_position_to_byte_offset(&result, edit.range.end);
            result.replace_range(start..end, &edit.new_text);
        }

        assert_eq!(
            result,
            "<?php\nuse Foo\\Bar;\nuse Quux\\Quuz;\n\nclass Test extends Bar\n{\n    public function make(): Quuz\n    {\n        return new Quuz();\n    }\n}\n"
        );
    }

    // ── Contiguous block blank-line regression ──────────────────────

    #[test]
    fn removing_middle_import_from_contiguous_block_leaves_no_blank_line() {
        // Reproduces: removing `use PHPMD\Rule;` from a contiguous block
        // left a blank line between the surviving imports.
        let content = "\
<?php
use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;
use PHPMD\\Rule\\Design\\CouplingBetweenObjects;
";
        // Line 3 is `use PHPMD\Rule;` — the only removed import.
        let removed = HashSet::from([3usize]);
        let range = Range::new(Position::new(3, 4), Position::new(3, 14));
        let edit = build_line_deletion_edit(content, &range, &removed);

        let start = lsp_position_to_byte_offset(content, edit.range.start);
        let end = lsp_position_to_byte_offset(content, edit.range.end);
        let mut result = content.to_string();
        result.replace_range(start..end, &edit.new_text);

        assert_eq!(
            result,
            "\
<?php
use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule\\Design\\CouplingBetweenObjects;
",
            "Removing a middle import should not leave a blank line"
        );
    }

    #[test]
    fn removing_first_import_from_contiguous_block_leaves_no_blank_line() {
        let content = "\
<?php
use PHPMD\\Node\\AbstractCallableNode;
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;
";
        let removed = HashSet::from([1usize]);
        let range = Range::new(Position::new(1, 4), Position::new(1, 34));
        let edit = build_line_deletion_edit(content, &range, &removed);

        let start = lsp_position_to_byte_offset(content, edit.range.start);
        let end = lsp_position_to_byte_offset(content, edit.range.end);
        let mut result = content.to_string();
        result.replace_range(start..end, &edit.new_text);

        assert_eq!(
            result,
            "\
<?php
use PHPMD\\Node\\MethodNode;
use PHPMD\\Rule;
",
            "Removing the first import should not leave a blank line"
        );
    }
}
