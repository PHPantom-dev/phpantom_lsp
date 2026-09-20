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

use super::{CodeActionData, make_code_action_data};
use crate::Backend;
use crate::diagnostics::helpers::scan_use_statements;
use crate::text_position::{line_start_byte_offset, offset_to_position, ranges_overlap};

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

/// Check whether the cursor line belongs to a namespace-level `use` import.
///
/// Returns `true` for any line of the statement, including the wrapped
/// members of a group import, and `false` for a trait `use` inside a
/// class/trait body.
pub(crate) fn cursor_on_use_import_line(content: &str, line: u32) -> bool {
    // The byte offset the line starts at; a line past the end of the file
    // rests on nothing.
    let mut offset = 0usize;
    for _ in 0..line {
        match content[offset..].find('\n') {
            Some(newline) => offset += newline + 1,
            None => return false,
        }
    }
    scan_use_statements(content).iter().any(|statement| {
        statement.top_level && statement.line_start <= offset && offset <= statement.end
    })
}

/// Build a `TextEdit` that deletes the full line(s) covered by `range`,
/// including the trailing newline.
///
/// When the diagnostic targets a single member inside a group `use`
/// statement (e.g. `use Foo\{Bar, Baz};` where only `Bar` is unused),
/// the edit removes just the member entry rather than the whole line —
/// unless every member of that group is being removed in this same
/// batch (`all_removed_ranges`), in which case the whole statement is
/// deleted, the same way a lone `use Foo\Bar;` line would be.
pub(crate) fn build_line_deletion_edit(
    content: &str,
    range: &Range,
    removed_import_lines: &HashSet<usize>,
    all_removed_ranges: &[Range],
) -> TextEdit {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = range.start.line as usize;

    if let Some((group_start, group_end, member_count)) = group_statement_bounds(&lines, line_idx) {
        let targeted_in_group = all_removed_ranges
            .iter()
            .filter(|r| {
                let l = r.start.line as usize;
                l >= group_start && l <= group_end
            })
            .count();

        if targeted_in_group < member_count {
            if let Some(edit) = extend_range_for_group_member(content, range) {
                return edit;
            }
        } else {
            return delete_line_span(
                content,
                &lines,
                group_start,
                group_end,
                removed_import_lines,
            );
        }
    }

    let start_line = range.start.line as usize;
    let end_line = range.end.line as usize;
    delete_line_span(content, &lines, start_line, end_line, removed_import_lines)
}

/// Delete lines `start_line..=end_line` (inclusive), including the
/// trailing newline, optionally consuming an adjoining blank line so no
/// gap is left behind.
fn delete_line_span(
    content: &str,
    lines: &[&str],
    start_line: usize,
    end_line: usize,
    removed_import_lines: &HashSet<usize>,
) -> TextEdit {
    // Compute line-start offsets from real terminator lengths so the edit
    // stays aligned on CRLF files (where `str::lines()` strips the `\r`).
    let edit_start_offset =
        if should_consume_previous_blank_line(lines, start_line, end_line, removed_import_lines) {
            line_start_byte_offset(content, start_line - 1)
        } else {
            line_start_byte_offset(content, start_line)
        };

    // Deleting through the start of the line after `end_line` consumes
    // `end_line`'s terminator. Optionally extend over a following blank
    // line as well.
    let last_consumed_line =
        if should_consume_following_blank_line(lines, start_line, end_line, removed_import_lines) {
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

/// Locate the `use Foo\{...};` group statement enclosing `line_idx`
/// (which may itself be the opening line, for a single-line group, or
/// any line within a multi-line group). Returns the statement's
/// inclusive `(start_line, end_line)` span and its member count.
///
/// Returns `None` when `line_idx` is not part of a group `use`
/// statement at all.
pub(crate) fn group_statement_bounds(
    lines: &[&str],
    line_idx: usize,
) -> Option<(usize, usize, usize)> {
    if line_idx >= lines.len() {
        return None;
    }

    // Check if any line in the vicinity contains `{` and `}` — the
    // hallmark of a group use statement.
    let line = lines[line_idx];
    let (start, end, full_stmt) = if line.contains('{') && line.contains('}') {
        (line_idx, line_idx, line.to_string())
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
        (start, end, lines[start..=end].join("\n"))
    };

    // Must have both `{` and `}` to be a group use.
    if !full_stmt.contains('{') || !full_stmt.contains('}') {
        return None;
    }

    let brace_start = full_stmt.find('{')?;
    let brace_end = full_stmt.find('}')?;
    let members_text = &full_stmt[brace_start + 1..brace_end];
    let member_count = members_text
        .split(',')
        .filter(|m| !m.trim().is_empty())
        .count();

    Some((start, end, member_count))
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
    let (_, _, member_count) = group_statement_bounds(&lines, line_idx)?;

    let line = lines[line_idx];

    // Locate the member text from the diagnostic range. The range columns
    // are UTF-16 code units; convert them to byte offsets before slicing
    // `line` (and convert back to UTF-16 columns for the resulting edit).
    let start_byte = crate::text_position::utf16_col_to_byte_offset(line, range.start.character);
    let end_byte = crate::text_position::utf16_col_to_byte_offset(line, range.end.character);
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
    if member_count <= 1 {
        return None;
    }

    if member_text.trim().is_empty() {
        return None;
    }

    // In a wrapped group the member usually sits alone on its line, so
    // removing just the member text would leave an empty line inside the
    // braces.  Delete the whole line instead.  A trailing comma left
    // before the closing brace is legal PHP.
    if line[..removal_start].trim().is_empty()
        && line[removal_end..].trim().is_empty()
        && !line.contains('{')
        && !line.contains('}')
    {
        return Some(TextEdit {
            range: Range {
                start: Position::new(range.start.line, 0),
                end: Position::new(range.start.line + 1, 0),
            },
            new_text: String::new(),
        });
    }

    let start_pos = Position::new(
        range.start.line,
        crate::text_position::byte_offset_to_utf16_col(line, removal_start),
    );
    let end_pos = Position::new(
        range.start.line,
        crate::text_position::byte_offset_to_utf16_col(line, removal_end),
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
    use crate::text_position::position_to_byte_offset;

    /// Test-only wrapper: builds a deletion edit treating only the
    /// diagnostic's own line as removed (single-import scenario).
    fn build_single_line_deletion_edit(content: &str, range: &Range) -> TextEdit {
        let removed = HashSet::from([range.start.line as usize]);
        build_line_deletion_edit(content, range, &removed, std::slice::from_ref(range))
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
        let start = position_to_byte_offset(content, edit.range.start);
        let end = position_to_byte_offset(content, edit.range.end);
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
        let start = position_to_byte_offset(content, edit.range.start);
        let end = position_to_byte_offset(content, edit.range.end);
        assert_eq!(&content[start..end], "use Foo\\Bar;\r\n");
    }

    #[test]
    fn deletes_use_line_and_separator_when_last_import_removed() {
        let content = "<?php\nuse Foo\\Bar;\n\nclass Test {}\n";
        let range = Range::new(Position::new(1, 4), Position::new(1, 11));
        let edit = build_single_line_deletion_edit(content, &range);
        let start = position_to_byte_offset(content, edit.range.start);
        let end = position_to_byte_offset(content, edit.range.end);
        assert_eq!(&content[start..end], "use Foo\\Bar;\n\n");
    }

    #[test]
    fn keeps_separator_when_other_imports_remain() {
        let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass Test extends Qux {}\n";
        let range = Range::new(Position::new(2, 4), Position::new(2, 11));
        let edit = build_single_line_deletion_edit(content, &range);
        let start = position_to_byte_offset(content, edit.range.start);
        let end = position_to_byte_offset(content, edit.range.end);
        assert_eq!(&content[start..end], "use Baz\\Qux;\n");
    }

    #[test]
    fn removes_following_blank_line_between_remaining_imports() {
        let content = "<?php\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nuse Quux\\Quuz;\n";
        let range = Range::new(Position::new(2, 4), Position::new(2, 11));
        let edit = build_single_line_deletion_edit(content, &range);
        let start = position_to_byte_offset(content, edit.range.start);
        let end = position_to_byte_offset(content, edit.range.end);
        assert_eq!(&content[start..end], "use Baz\\Qux;\n\n");
    }

    #[test]
    fn removes_previous_blank_line_between_remaining_imports() {
        let content = "<?php\nuse Foo\\Bar;\n\nuse Baz\\Qux;\nuse Quux\\Quuz;\n";
        let range = Range::new(Position::new(3, 4), Position::new(3, 11));
        let edit = build_single_line_deletion_edit(content, &range);
        let start = position_to_byte_offset(content, edit.range.start);
        let end = position_to_byte_offset(content, edit.range.end);

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

    #[test]
    fn removes_whole_single_line_group_when_all_members_unused_in_batch() {
        let content = "<?php\nuse App\\Models\\{User, Post};\n\nclass Foo {}\n";
        let removed = HashSet::from([1usize]);
        let user_range = Range::new(Position::new(1, 18), Position::new(1, 22));
        let post_range = Range::new(Position::new(1, 24), Position::new(1, 28));
        let all_ranges = [user_range, post_range];

        let edit = build_line_deletion_edit(content, &user_range, &removed, &all_ranges);
        let start = position_to_byte_offset(content, edit.range.start);
        let end = position_to_byte_offset(content, edit.range.end);
        let mut result = content.to_string();
        result.replace_range(start..end, &edit.new_text);

        assert_eq!(result, "<?php\nclass Foo {}\n");
    }

    #[test]
    fn removes_whole_multiline_group_when_all_members_unused_in_batch() {
        let content = "<?php\nuse App\\Models\\{\n    User,\n    Post,\n};\n\nclass Foo {}\n";
        let removed = HashSet::from([2usize, 3usize]);
        let user_range = Range::new(Position::new(2, 4), Position::new(2, 8));
        let post_range = Range::new(Position::new(3, 4), Position::new(3, 8));
        let all_ranges = [user_range, post_range];

        let user_edit = build_line_deletion_edit(content, &user_range, &removed, &all_ranges);
        let post_edit = build_line_deletion_edit(content, &post_range, &removed, &all_ranges);
        assert_eq!(
            user_edit.range, post_edit.range,
            "both diagnostics in a fully-removed group should collapse to one edit"
        );

        let start = position_to_byte_offset(content, user_edit.range.start);
        let end = position_to_byte_offset(content, user_edit.range.end);
        let mut result = content.to_string();
        result.replace_range(start..end, &user_edit.new_text);

        assert_eq!(result, "<?php\nclass Foo {}\n");
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
        let edit =
            build_line_deletion_edit(content, &range, &removed, std::slice::from_ref(&range));

        let start = position_to_byte_offset(content, edit.range.start);
        let end = position_to_byte_offset(content, edit.range.end);
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
        let edit =
            build_line_deletion_edit(content, &range, &removed, std::slice::from_ref(&range));

        let start = position_to_byte_offset(content, edit.range.start);
        let end = position_to_byte_offset(content, edit.range.end);
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
