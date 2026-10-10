//! Sort `use` statements code action.
//!
//! Re-sorts a file's existing top-level `use` imports alphabetically,
//! the way PhpStorm's "Optimize Imports" and Phpactor's import sorter do.
//! `import_class.rs` already sorts *new* candidates when offering an
//! import, and `analyze_use_block` (`completion/use_edit.rs`) already
//! parses the existing block's entries — this module reorders the block
//! that already exists.
//!
//! Sorting rules:
//!
//! - The three import kinds (plain `use`, `use function`, `use const`)
//!   never interleave; each is sorted within its own bucket.
//! - Sorting is case-insensitive and keyed on the imported name, not the
//!   alias (`use Zebra\Foo as Aardvark;` sorts under `Zebra\Foo`).
//! - A `use Foo\{Bar, Baz};` group-use statement sorts as one entry,
//!   keyed on `Foo`.
//! - A blank line (or any other line that separates two imports) marks a
//!   group boundary; imports are only reordered within their own group,
//!   never across it.
//! - A comment attached to one `use` line (leading or trailing) moves
//!   with it.
//!
//! The edits are computed at resolve time against the current content,
//! so a client that applies this after "Remove all unused imports" (as
//! organize-on-save does) sorts the text the removal left behind.

use tower_lsp::lsp_types::*;

use super::{CodeActionData, make_code_action_data, range_touches_use_import};
use crate::Backend;
use crate::completion::use_edit::{UseBlockInfo, analyze_use_block};

impl Backend {
    /// Collect the "Sort use statements" code action.
    ///
    /// Only offered when the request touches a top-level `use` import
    /// (mirroring the bulk "Remove all unused imports" action) and when
    /// the block isn't already sorted.
    ///
    /// Phase 1 only — edits are deferred to [`resolve_sort_use_statements`].
    pub(crate) fn collect_sort_use_statements_action(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        if !range_touches_use_import(content, &params.range)
            || compute_sort_use_edits(content).is_empty()
        {
            return;
        }

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Sort use statements".to_string(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            diagnostics: None,
            edit: None,
            command: None,
            is_preferred: None,
            disabled: None,
            data: Some(make_code_action_data(
                "source.sortUseStatements",
                uri,
                &params.range,
                serde_json::json!({}),
            )),
        }));
    }

    /// Resolve a deferred "Sort use statements" action against the
    /// current content.
    pub(crate) fn resolve_sort_use_statements(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let doc_uri: Url = data.uri.parse().ok()?;
        let edits = compute_sort_use_edits(content);
        if edits.is_empty() {
            return None;
        }
        Some(crate::code_actions::single_file_edit(doc_uri, edits))
    }
}

/// One `use` import together with the source lines it must move with:
/// any leading comment lines attached to it and the statement itself
/// (whose last line may carry a trailing inline comment).
struct Entry {
    /// First line index (inclusive) — the start of an attached leading
    /// comment, or the statement's own start line if it has none.
    leading: usize,
    /// Last line index (inclusive) — the line containing the
    /// statement-terminating `;`.
    end: usize,
    /// Lowercased sort key from [`analyze_use_block`], already stripped
    /// of any `as` alias and prefixed with `function `/`const ` for
    /// non-class imports.
    sort_key: String,
}

/// Compute the edits that re-sort the file's `use` block(s).
///
/// Returns one `TextEdit` per group (a maximal run of imports with no
/// gap between them) that isn't already sorted. Groups that are already
/// in order, or that contain a single import, produce no edit.
fn compute_sort_use_edits(content: &str) -> Vec<TextEdit> {
    let use_block: UseBlockInfo = analyze_use_block(content);
    if use_block.existing.len() < 2 {
        return Vec::new();
    }

    let lines: Vec<&str> = content.lines().collect();
    // `lines()` strips the terminator; write back the one the file uses.
    let eol = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };

    let mut entries: Vec<Entry> = Vec::with_capacity(use_block.existing.len());
    let mut floor = 0usize;
    for (line, sort_key) in &use_block.existing {
        let start = *line as usize;
        let end = statement_end_line(&lines, start);
        let leading = leading_comment_start(&lines, start, floor);
        floor = end + 1;
        entries.push(Entry {
            leading,
            end,
            sort_key: sort_key.clone(),
        });
    }

    // Split into groups: an entry continues the previous group only when
    // it starts immediately after the previous one ends (no blank line,
    // comment gap, or other content in between).
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 && entry.leading == entries[i - 1].end + 1 {
            groups.last_mut().expect("non-empty").push(i);
        } else {
            groups.push(vec![i]);
        }
    }

    let mut edits = Vec::new();
    for group in &groups {
        if group.len() < 2 {
            continue;
        }

        let mut sorted = group.clone();
        sorted.sort_by(|&a, &b| {
            let ka = &entries[a].sort_key;
            let kb = &entries[b].sort_key;
            UseBlockInfo::key_group(ka)
                .cmp(&UseBlockInfo::key_group(kb))
                .then_with(|| ka.cmp(kb))
        });

        if sorted == *group {
            continue;
        }

        let group_start_line = entries[group[0]].leading;
        let group_end_line = entries[*group.last().expect("non-empty")].end;

        let mut new_text = String::new();
        for &idx in &sorted {
            for line in &lines[entries[idx].leading..=entries[idx].end] {
                new_text.push_str(line);
                new_text.push_str(eol);
            }
        }

        edits.push(TextEdit {
            range: Range {
                start: Position {
                    line: group_start_line as u32,
                    character: 0,
                },
                end: Position {
                    line: group_end_line as u32 + 1,
                    character: 0,
                },
            },
            new_text,
        });
    }

    edits
}

/// Find the last line (inclusive) of the `use` statement starting at
/// `start`, i.e. the line containing the terminating `;`.
///
/// Tracks brace depth so a multi-line group-use (`use Foo\{\n Bar,\n
/// Baz,\n};`) is treated as one statement rather than ending at the
/// first `;` found (there is none until the closing line).
fn statement_end_line(lines: &[&str], start: usize) -> usize {
    let mut depth: i32 = 0;
    for (offset, line) in lines[start..].iter().enumerate() {
        for ch in line.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                ';' if depth <= 0 => return start + offset,
                _ => {}
            }
        }
    }
    lines.len() - 1
}

/// Find the first line (inclusive) belonging to the `use` statement that
/// starts at `stmt_start`, extending upward over any directly attached
/// leading comment.
///
/// Stops at a blank line, at `floor` (the end of the previous entry,
/// exclusive — never steals lines already claimed by it), or at a line
/// that isn't part of a comment. Handles both single-line comments
/// (`//`, `#`, or a `/* ... */` that opens and closes on one line) and
/// multi-line `/* ... */` blocks by scanning up for the opening `/*`.
fn leading_comment_start(lines: &[&str], stmt_start: usize, floor: usize) -> usize {
    let mut cursor = stmt_start;
    while cursor > floor {
        let idx = cursor - 1;
        let trimmed = lines[idx].trim();

        if trimmed.is_empty() {
            break;
        }

        if trimmed.starts_with("//") || (trimmed.starts_with('#') && !trimmed.starts_with("#[")) {
            cursor = idx;
            continue;
        }

        if trimmed.ends_with("*/") {
            if trimmed.starts_with("/*") {
                cursor = idx;
                continue;
            }
            // Tail of a multi-line block comment — scan up for its start.
            let mut start_idx = idx;
            let mut found = false;
            while start_idx > floor {
                start_idx -= 1;
                if lines[start_idx].trim_start().starts_with("/*") {
                    found = true;
                    break;
                }
            }
            if found {
                cursor = start_idx;
                continue;
            }
            break;
        }

        break;
    }
    cursor
}

#[cfg(test)]
#[path = "sort_use_statements_tests.rs"]
mod tests;
