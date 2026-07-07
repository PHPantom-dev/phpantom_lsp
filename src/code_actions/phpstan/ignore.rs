//! PHPStan ignore code actions.
//!
//! Provides two code actions for PHPStan diagnostics:
//!
//! 1. **Add `@phpstan-ignore`** — when the cursor is on a line with a
//!    PHPStan error, offer to add `// @phpstan-ignore <identifier>` as
//!    an inline comment on the same line.  If the line already has a
//!    `@phpstan-ignore` comment, the new identifier is appended to the
//!    existing comma-separated list.
//!
//! 2. **Remove unnecessary ignore** — when PHPStan reports an unmatched
//!    ignore comment (identifier `ignore.unmatched`), offer to remove
//!    the `@phpstan-ignore` comment (or the specific identifier from a
//!    multi-identifier ignore).

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::util::ranges_overlap;

/// PHPStan identifier prefix for unmatched ignore errors.
///
/// PHPStan uses several identifiers for unmatched ignores:
/// - `ignore.unmatchedIdentifier` — a `@phpstan-ignore` with a specific
///   identifier that doesn't match any error on the target line.
/// - `ignore.unmatched` — a broader unmatched ignore.
///
/// We match by prefix so all variants are caught.
const UNMATCHED_IGNORE_PREFIX: &str = "ignore.unmatched";

impl Backend {
    /// Collect PHPStan-related code actions: add ignore and remove
    /// unnecessary ignore.
    pub(crate) fn collect_phpstan_ignore_actions(
        &self,
        uri: &str,
        _content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        // Look at the diagnostics attached to this code action request
        // and also at all cached PHPStan diagnostics for the file.
        let phpstan_diags: Vec<Diagnostic> = {
            let cache = self.phpstan_last_diags.lock();
            cache.get(uri).cloned().unwrap_or_default()
        };

        // ── Add @phpstan-ignore ─────────────────────────────────────────
        // For each PHPStan diagnostic overlapping the request range that
        // has a usable identifier, offer to add an ignore comment.
        // Group diagnostics by (line, identifier) so that multiple
        // diagnostics with the same identifier on the same line produce
        // a single code action with all of them attached.
        let mut ignore_groups: HashMap<(u32, String), Vec<Diagnostic>> = HashMap::new();
        for diag in &phpstan_diags {
            if !ranges_overlap(&diag.range, &params.range) {
                continue;
            }

            let identifier = match &diag.code {
                Some(NumberOrString::String(s)) => s.as_str(),
                _ => continue,
            };

            // Skip diagnostics without a specific identifier.
            if identifier == "phpstan" || identifier.is_empty() {
                continue;
            }

            // Skip unmatched-ignore errors (those get the "remove" action).
            if identifier.starts_with(UNMATCHED_IGNORE_PREFIX) {
                continue;
            }

            // Skip non-ignorable errors — PHPStan will not honour an
            // `@phpstan-ignore` comment for these (e.g. visibility
            // overrides).  The `ignorable` flag is stored in
            // `Diagnostic.data` by `parse_phpstan_message()`.
            if !is_ignorable(diag) {
                continue;
            }

            let line = diag.range.start.line;
            ignore_groups
                .entry((line, identifier.to_string()))
                .or_default()
                .push(diag.clone());
        }

        for ((line, identifier), diags) in &ignore_groups {
            let extra = serde_json::json!({
                "identifier": identifier,
                "line": line,
            });

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Ignore PHPStan error ({})", identifier),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(diags.clone()),
                edit: None,
                command: None,
                is_preferred: Some(false),
                disabled: None,
                data: Some(make_code_action_data(
                    "phpstan.addIgnore",
                    uri,
                    &params.range,
                    extra,
                )),
            }));
        }

        // ── Remove unnecessary @phpstan-ignore ──────────────────────────
        // When PHPStan reports an unmatched ignore, offer to remove it.
        for diag in &phpstan_diags {
            if !ranges_overlap(&diag.range, &params.range) {
                continue;
            }

            let identifier = match &diag.code {
                Some(NumberOrString::String(s)) => s.as_str(),
                _ => continue,
            };

            if !identifier.starts_with(UNMATCHED_IGNORE_PREFIX) {
                continue;
            }

            // Parse the identifier and line from the message.
            // PHPStan message format:
            //   "No error with identifier <id> is reported on line <N>."
            let ignore_id = parse_unmatched_identifier(&diag.message);
            let extra = serde_json::json!({
                "diagnostic_message": diag.message,
                "diagnostic_line": diag.range.start.line,
                "diagnostic_code": identifier,
            });

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: match ignore_id {
                    Some(ref id) => format!("Remove unnecessary @phpstan-ignore ({})", id),
                    None => "Remove unnecessary @phpstan-ignore".to_string(),
                },
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: None,
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: Some(make_code_action_data(
                    "phpstan.removeIgnore",
                    uri,
                    &params.range,
                    extra,
                )),
            }));
        }
    }

    /// Resolve a deferred "Add @phpstan-ignore" code action by computing
    /// the workspace edit.
    pub(crate) fn resolve_add_ignore(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let identifier = data.extra.get("identifier")?.as_str()?;
        let line = data.extra.get("line")?.as_u64()? as u32;

        let line_text = content.lines().nth(line as usize)?;

        let edit = build_add_ignore_edit(content, line, line_text, identifier);

        let doc_uri: Url = data.uri.parse().ok()?;
        let mut changes = HashMap::new();
        changes.insert(doc_uri, vec![edit]);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }

    /// Resolve a deferred "Remove unnecessary @phpstan-ignore" code action
    /// by computing the workspace edit.
    pub(crate) fn resolve_remove_ignore(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let diagnostic_message = data.extra.get("diagnostic_message")?.as_str()?;
        let diagnostic_line = data.extra.get("diagnostic_line")?.as_u64()? as u32;

        let ignore_id = parse_unmatched_identifier(diagnostic_message);
        let ignore_line = parse_unmatched_line(diagnostic_message);

        let message_line = ignore_line.unwrap_or(diagnostic_line);

        let edit =
            build_remove_ignore_edit(content, message_line, diagnostic_line, ignore_id.as_deref())?;

        let doc_uri: Url = data.uri.parse().ok()?;
        let mut changes = HashMap::new();
        changes.insert(doc_uri, vec![edit]);

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }
}

/// Build a `TextEdit` that adds a `@phpstan-ignore` comment to a line.
///
/// If the line already contains `@phpstan-ignore`, the identifier is
/// appended to the existing comma-separated list.  Otherwise, a
/// `// @phpstan-ignore <id>` comment is inserted at the end of the line.
fn build_add_ignore_edit(content: &str, line: u32, line_text: &str, identifier: &str) -> TextEdit {
    // Check if line already has a @phpstan-ignore comment.
    if let Some(ignore_pos) = line_text.find("@phpstan-ignore") {
        let after_tag = &line_text[ignore_pos + "@phpstan-ignore".len()..];

        // If it's `@phpstan-ignore-line` or `@phpstan-ignore-next-line`,
        // we don't touch it — add a new comment instead.
        if after_tag.starts_with("-line") || after_tag.starts_with("-next-line") {
            return build_eol_comment(content, line, line_text, identifier);
        }

        // Find the end of the existing identifier list.
        // The identifiers are everything after `@phpstan-ignore ` up to
        // a closing comment delimiter, parenthesis, or end of line.
        let ids_start = ignore_pos + "@phpstan-ignore".len();
        let ids_text = &line_text[ids_start..];

        // Trim leading whitespace after the tag.
        let ids_trimmed = ids_text.trim_start();
        let ids_offset = ids_text.len() - ids_trimmed.len();

        // Find where the identifier list ends: at `*/`, `)`, or EOL.
        let ids_end = ids_trimmed
            .find("*/")
            .or_else(|| {
                // For `// @phpstan-ignore id (reason)`, stop at the
                // opening paren of the reason.  But only if the paren
                // is preceded by whitespace (not part of an identifier).
                ids_trimmed.find(" (")
            })
            .unwrap_or(ids_trimmed.len());

        let existing_ids = ids_trimmed[..ids_end].trim();

        // Check if identifier is already present.
        if existing_ids.split(',').any(|id| id.trim() == identifier) {
            // Already ignored — return a no-op edit.
            return TextEdit {
                range: Range {
                    start: Position { line, character: 0 },
                    end: Position { line, character: 0 },
                },
                new_text: String::new(),
            };
        }

        // Insert the new identifier after the existing ones. The accumulated
        // offset is in bytes; LSP positions are UTF-16 code units, so convert
        // before emitting the edit.
        let insert_byte = ids_start + ids_offset + ids_end;
        let insert_col = crate::util::byte_offset_to_utf16_col(line_text, insert_byte);

        // Check if we need a comma separator.
        let separator = if existing_ids.is_empty() { "" } else { ", " };

        return TextEdit {
            range: Range {
                start: Position {
                    line,
                    character: insert_col,
                },
                end: Position {
                    line,
                    character: insert_col,
                },
            },
            new_text: format!("{}{}", separator, identifier),
        };
    }

    build_eol_comment(content, line, line_text, identifier)
}

/// Build a `TextEdit` that appends `// @phpstan-ignore <id>` at the end
/// of a line.
fn build_eol_comment(_content: &str, line: u32, line_text: &str, identifier: &str) -> TextEdit {
    // LSP positions are UTF-16 code units, not bytes: convert the end-of-line
    // byte offset so the comment lands correctly on multibyte lines.
    let end_col = crate::util::byte_offset_to_utf16_col(line_text, line_text.len());
    TextEdit {
        range: Range {
            start: Position {
                line,
                character: end_col,
            },
            end: Position {
                line,
                character: end_col,
            },
        },
        new_text: format!(" // @phpstan-ignore {}", identifier),
    }
}

/// Build a `TextEdit` that removes a `@phpstan-ignore` comment or a
/// specific identifier from it.
///
/// `message_line` is the line referenced in the PHPStan message (the
/// line the ignore is supposed to suppress errors on).  `diag_line` is
/// the line PHPStan reports the unmatched-ignore error on.  We search
/// both, plus `message_line - 1` (the previous-line style), to cover
/// all comment placement conventions.
///
/// Returns `None` if no `@phpstan-ignore` comment is found.
fn build_remove_ignore_edit(
    content: &str,
    message_line: u32,
    diag_line: u32,
    remove_id: Option<&str>,
) -> Option<TextEdit> {
    let lines: Vec<&str> = content.lines().collect();

    // Search the message line, the line above it, and the diagnostic
    // line (which may differ from message_line).  Deduplicate so we
    // don't check the same line twice.
    let mut search_lines = vec![message_line];
    if message_line > 0 {
        search_lines.push(message_line - 1);
    }
    if diag_line != message_line && (message_line == 0 || diag_line != message_line - 1) {
        search_lines.push(diag_line);
    }

    for &check_line in &search_lines {
        let line_text = match lines.get(check_line as usize) {
            Some(l) => *l,
            None => continue,
        };

        if let Some(ignore_pos) = line_text.find("@phpstan-ignore") {
            let after_tag = &line_text[ignore_pos + "@phpstan-ignore".len()..];

            // Don't touch `@phpstan-ignore-line` / `@phpstan-ignore-next-line`.
            if after_tag.starts_with("-line") || after_tag.starts_with("-next-line") {
                // But if we specifically want to remove the whole thing,
                // we can handle it below.
                return build_remove_whole_ignore(content, check_line, line_text, ignore_pos);
            }

            // If we have a specific identifier to remove, try to remove
            // just that one from the comma-separated list.
            if let Some(id) = remove_id {
                return build_remove_single_id(content, check_line, line_text, ignore_pos, id);
            }

            // No specific identifier — remove the whole comment.
            return build_remove_whole_ignore(content, check_line, line_text, ignore_pos);
        }
    }

    None
}

/// Remove the entire `@phpstan-ignore` comment from a line.
///
/// If the comment is the only content on the line (possibly with
/// leading whitespace), delete the entire line.  Otherwise, remove
/// just the comment portion.
fn build_remove_whole_ignore(
    content: &str,
    line: u32,
    line_text: &str,
    ignore_pos: usize,
) -> Option<TextEdit> {
    // Find the start of the comment that contains @phpstan-ignore.
    // Walk backwards from `ignore_pos` to find `//`, `/*`, or `/**`.
    let before = &line_text[..ignore_pos];
    let comment_start = before
        .rfind("//")
        .or_else(|| before.rfind("/*"))
        .unwrap_or(ignore_pos);

    let before_comment = line_text[..comment_start].trim_end();

    if before_comment.is_empty() {
        // The comment is the only thing on this line — delete the whole line.
        let line_count = content.lines().count();
        let next_line = line + 1;

        if (next_line as usize) <= line_count {
            // Delete from start of this line to start of next line.
            Some(TextEdit {
                range: Range {
                    start: Position { line, character: 0 },
                    end: Position {
                        line: next_line,
                        character: 0,
                    },
                },
                new_text: String::new(),
            })
        } else {
            // Last line — delete from end of previous line to end of this line.
            if line > 0 {
                let prev_line_text = content.lines().nth((line - 1) as usize).unwrap_or("");
                Some(TextEdit {
                    range: Range {
                        start: Position {
                            line: line - 1,
                            character: prev_line_text.len() as u32,
                        },
                        end: Position {
                            line,
                            character: line_text.len() as u32,
                        },
                    },
                    new_text: String::new(),
                })
            } else {
                Some(TextEdit {
                    range: Range {
                        start: Position { line, character: 0 },
                        end: Position {
                            line,
                            character: line_text.len() as u32,
                        },
                    },
                    new_text: String::new(),
                })
            }
        }
    } else {
        // There is code before the comment — remove only the comment,
        // including any trailing whitespace before it.
        //
        // Also handle `/* ... */` block comments where we need to
        // remove up to the closing `*/`.
        let end_col = if line_text[comment_start..].starts_with("/*") {
            // Find the closing `*/`.
            line_text[comment_start..]
                .find("*/")
                .map(|p| comment_start + p + 2)
                .unwrap_or(line_text.len())
        } else {
            line_text.len()
        };

        // Trim trailing whitespace before the comment too.
        let trim_start = before_comment.len();

        Some(TextEdit {
            range: Range {
                start: Position {
                    line,
                    character: trim_start as u32,
                },
                end: Position {
                    line,
                    character: end_col as u32,
                },
            },
            new_text: String::new(),
        })
    }
}

/// Remove a single identifier from a `@phpstan-ignore id1, id2` comment.
///
/// If only one identifier remains after removal, or if the target id is
/// the only one, falls back to removing the whole comment.
fn build_remove_single_id(
    content: &str,
    line: u32,
    line_text: &str,
    ignore_pos: usize,
    remove_id: &str,
) -> Option<TextEdit> {
    let ids_start = ignore_pos + "@phpstan-ignore".len();
    let ids_text = &line_text[ids_start..];
    let ids_trimmed = ids_text.trim_start();
    let ids_offset = ids_text.len() - ids_trimmed.len();

    // Find where the identifier list ends.
    let ids_end = ids_trimmed
        .find("*/")
        .or_else(|| ids_trimmed.find(" ("))
        .unwrap_or(ids_trimmed.len());

    let ids_str = ids_trimmed[..ids_end].trim();
    let ids: Vec<&str> = ids_str.split(',').map(|s| s.trim()).collect();

    if ids.len() <= 1 || (ids.len() == 1 && ids[0] == remove_id) {
        // Only one identifier (or it's the one we want to remove) —
        // remove the whole comment.
        return build_remove_whole_ignore(content, line, line_text, ignore_pos);
    }

    // Check if the identifier is actually in the list.
    if !ids.contains(&remove_id) {
        // The identifier we're supposed to remove isn't here.
        // Fall back to removing the whole comment.
        return build_remove_whole_ignore(content, line, line_text, ignore_pos);
    }

    // Remove just this identifier from the list.
    let new_ids: Vec<&str> = ids.iter().filter(|&&id| id != remove_id).copied().collect();
    let new_ids_str = new_ids.join(", ");

    // Reconstruct: replace the identifier list portion.
    let abs_ids_start = (ids_start + ids_offset) as u32;
    let abs_ids_end = (ids_start + ids_offset + ids_end) as u32;

    Some(TextEdit {
        range: Range {
            start: Position {
                line,
                character: abs_ids_start,
            },
            end: Position {
                line,
                character: abs_ids_end,
            },
        },
        new_text: new_ids_str,
    })
}

/// Parse the ignored identifier from an unmatched-ignore error message.
///
/// PHPStan format: `"No error with identifier <id> is reported on line <N>."`
fn parse_unmatched_identifier(message: &str) -> Option<String> {
    let prefix = "No error with identifier ";
    let start = message.find(prefix)?;
    let after = &message[start + prefix.len()..];
    let end = after.find(" is reported")?;
    Some(after[..end].to_string())
}

/// Parse the line number from an unmatched-ignore error message.
///
/// PHPStan format: `"No error with identifier <id> is reported on line <N>."`
/// Returns a 0-based LSP line number.
fn parse_unmatched_line(message: &str) -> Option<u32> {
    let prefix = "is reported on line ";
    let start = message.find(prefix)?;
    let after = &message[start + prefix.len()..];
    let end = after.find('.')?;
    let line_1based: u32 = after[..end].parse().ok()?;
    Some(line_1based.saturating_sub(1))
}

/// Check whether a PHPStan diagnostic is ignorable.
///
/// Returns `true` when the diagnostic can be suppressed with a
/// `@phpstan-ignore` comment.  Defaults to `true` when the `data`
/// field is absent (e.g. diagnostics from older PHPStan versions).
fn is_ignorable(diag: &Diagnostic) -> bool {
    match &diag.data {
        Some(data) => data
            .get("ignorable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        None => true,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::bool_assert_comparison)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── parse_unmatched_identifier ──────────────────────────────────

    #[test]
    fn parse_identifier_from_standard_message() {
        let msg = "No error with identifier variable.undefined is reported on line 31.";
        assert_eq!(
            parse_unmatched_identifier(msg),
            Some("variable.undefined".to_string())
        );
    }

    #[test]
    fn parse_identifier_from_dotted_identifier() {
        let msg = "No error with identifier argument.type is reported on line 10.";
        assert_eq!(
            parse_unmatched_identifier(msg),
            Some("argument.type".to_string())
        );
    }

    #[test]
    fn parse_identifier_returns_none_for_unrelated_message() {
        let msg = "Method Foo::bar() should return string but returns int.";
        assert_eq!(parse_unmatched_identifier(msg), None);
    }

    // ── parse_unmatched_line ────────────────────────────────────────

    #[test]
    fn parse_line_from_standard_message() {
        let msg = "No error with identifier variable.undefined is reported on line 31.";
        // 31 (1-based) → 30 (0-based)
        assert_eq!(parse_unmatched_line(msg), Some(30));
    }

    #[test]
    fn parse_line_returns_none_for_unrelated_message() {
        let msg = "Method Foo::bar() has no return type.";
        assert_eq!(parse_unmatched_line(msg), None);
    }

    // ── build_add_ignore_edit ───────────────────────────────────────

    #[test]
    fn adds_eol_comment_to_plain_line() {
        let content = "<?php\n$x = 1;\necho $x;\n";
        let edit = build_add_ignore_edit(content, 1, "$x = 1;", "variable.undefined");
        assert_eq!(edit.new_text, " // @phpstan-ignore variable.undefined");
        assert_eq!(edit.range.start.line, 1);
        assert_eq!(edit.range.start.character, 7); // end of "$x = 1;"
    }

    #[test]
    fn eol_comment_column_is_utf16_on_multibyte_line() {
        // "café" is 5 bytes but 4 UTF-16 code units; the comment must be
        // inserted at the UTF-16 column, not the byte length.
        let line_text = "$x = \"café\";";
        let content = format!("<?php\n{line_text}\n");
        let edit = build_add_ignore_edit(&content, 1, line_text, "variable.undefined");
        assert_eq!(edit.new_text, " // @phpstan-ignore variable.undefined");
        // 13 bytes ("é" is two) but 12 UTF-16 code units.
        assert_eq!(edit.range.start.character, 12);
    }

    #[test]
    fn appends_after_multibyte_in_existing_ignore() {
        // The receiver expression contains a multibyte character before the
        // existing ignore comment; the append position must be a UTF-16
        // column, not a byte offset.
        let line_text = "echo \"é\"; // @phpstan-ignore variable.undefined";
        let content = format!("<?php\n{line_text}\n");
        let edit = build_add_ignore_edit(&content, 1, line_text, "argument.type");
        assert_eq!(edit.new_text, ", argument.type");
        let insert_char = edit.range.start.character as usize;
        // The byte offset of the end of the id list would be one greater than
        // the UTF-16 column because "é" is two bytes.
        let byte_end = line_text.len();
        assert!(
            insert_char < byte_end,
            "UTF-16 column {insert_char} should be less than byte length {byte_end}",
        );
    }

    #[test]
    fn appends_to_existing_ignore_comment() {
        let content = "<?php\necho $x; // @phpstan-ignore variable.undefined\n";
        let line_text = "echo $x; // @phpstan-ignore variable.undefined";
        let edit = build_add_ignore_edit(content, 1, line_text, "argument.type");
        assert_eq!(edit.new_text, ", argument.type");
    }

    #[test]
    fn does_not_duplicate_existing_identifier() {
        let content = "<?php\necho $x; // @phpstan-ignore variable.undefined\n";
        let line_text = "echo $x; // @phpstan-ignore variable.undefined";
        let edit = build_add_ignore_edit(content, 1, line_text, "variable.undefined");
        // Should be a no-op edit.
        assert_eq!(edit.new_text, "");
        assert_eq!(edit.range.start, edit.range.end);
    }

    #[test]
    fn adds_eol_for_ignore_next_line() {
        let content = "<?php\n// @phpstan-ignore-next-line\necho $x;\n";
        let line_text = "// @phpstan-ignore-next-line";
        let edit = build_add_ignore_edit(content, 1, line_text, "variable.undefined");
        // Should not append to ignore-next-line, should add new comment.
        assert_eq!(edit.new_text, " // @phpstan-ignore variable.undefined");
    }

    #[test]
    fn appends_to_block_comment_ignore() {
        let content = "<?php\necho $x; /** @phpstan-ignore variable.undefined */\n";
        let line_text = "echo $x; /** @phpstan-ignore variable.undefined */";
        let edit = build_add_ignore_edit(content, 1, line_text, "argument.type");
        assert_eq!(edit.new_text, ", argument.type");
    }

    #[test]
    fn appends_to_ignore_with_reason() {
        let content = "<?php\necho $x; // @phpstan-ignore variable.undefined (lazy)\n";
        let line_text = "echo $x; // @phpstan-ignore variable.undefined (lazy)";
        let edit = build_add_ignore_edit(content, 1, line_text, "argument.type");
        assert_eq!(edit.new_text, ", argument.type");
        // Should insert before the reason parentheses.
        let insert_char = edit.range.start.character as usize;
        assert!(
            insert_char <= line_text.find(" (lazy)").unwrap(),
            "Insert position {} should be before the reason at {}",
            insert_char,
            line_text.find(" (lazy)").unwrap()
        );
    }

    // ── build_remove_ignore_edit ────────────────────────────────────

    #[test]
    fn removes_standalone_ignore_comment_line() {
        let content = "<?php\n    /** @phpstan-ignore variable.undefined */\n    echo $x;\n";
        let edit = build_remove_ignore_edit(content, 2, 2, Some("variable.undefined"));
        let edit = edit.unwrap();
        // Should delete the entire comment line.
        assert_eq!(edit.range.start.line, 1);
        assert_eq!(edit.range.start.character, 0);
        assert_eq!(edit.range.end.line, 2);
        assert_eq!(edit.range.end.character, 0);
        assert_eq!(edit.new_text, "");
    }

    #[test]
    fn removes_eol_ignore_comment() {
        let content = "<?php\necho $x; // @phpstan-ignore variable.undefined\n";
        let edit = build_remove_ignore_edit(content, 1, 1, Some("variable.undefined"));
        let edit = edit.unwrap();
        // Should remove the comment but keep "echo $x;"
        assert_eq!(edit.range.start.line, 1);
        assert_eq!(edit.new_text, "");
        // The code part "echo $x;" should be preserved (8 chars).
        assert_eq!(edit.range.start.character, 8);
    }

    #[test]
    fn removes_single_id_from_multi_id_ignore() {
        let content = "<?php\necho $x; // @phpstan-ignore variable.undefined, argument.type\n";
        let edit = build_remove_ignore_edit(content, 1, 1, Some("variable.undefined"));
        let edit = edit.unwrap();
        // Should keep `argument.type`.
        assert_eq!(edit.new_text, "argument.type");
    }

    #[test]
    fn removes_second_id_from_multi_id_ignore() {
        let content = "<?php\necho $x; // @phpstan-ignore variable.undefined, argument.type\n";
        let edit = build_remove_ignore_edit(content, 1, 1, Some("argument.type"));
        let edit = edit.unwrap();
        assert_eq!(edit.new_text, "variable.undefined");
    }

    #[test]
    fn removes_whole_comment_when_no_specific_id() {
        let content = "<?php\necho $x; // @phpstan-ignore variable.undefined\n";
        let edit = build_remove_ignore_edit(content, 1, 1, None);
        let edit = edit.unwrap();
        assert_eq!(edit.new_text, "");
    }

    #[test]
    fn finds_ignore_on_previous_line() {
        let content = "<?php\n// @phpstan-ignore variable.undefined\necho $x;\n";
        // Message references line 2 (echo $x), but the comment is on line 1.
        let edit = build_remove_ignore_edit(content, 2, 1, Some("variable.undefined"));
        let edit = edit.unwrap();
        assert_eq!(edit.range.start.line, 1);
    }

    #[test]
    fn returns_none_when_no_ignore_found() {
        let content = "<?php\necho $x;\n";
        let edit = build_remove_ignore_edit(content, 1, 1, Some("variable.undefined"));
        assert!(edit.is_none());
    }

    #[test]
    fn removes_block_comment_ignore_on_previous_line() {
        // Reproduces the user's exact scenario:
        //   line 0: public function update(): void
        //   line 1: {	/** @phpstan-ignore variable.undefined */
        //   line 2:     $request = new GetOrderStateRequest(...)
        //
        // PHPStan says: "No error with identifier variable.undefined
        // is reported on line 3." (1-based = line 2 in 0-based)
        // The diagnostic itself is on line 1 (the comment line).
        let content = "public function update(): void\n\
                        {\t/** @phpstan-ignore variable.undefined */\n\
                        \t$request = new GetOrderStateRequest((string)$this->orderId);\n";
        // message_line = 2 (0-based for "line 3" in PHPStan's 1-based),
        // diag_line = 1 (where PHPStan reports the unmatched ignore).
        let edit = build_remove_ignore_edit(content, 2, 1, Some("variable.undefined"));
        let edit = edit.unwrap();
        // The comment `/** @phpstan-ignore variable.undefined */` is on
        // line 1.  Since there is code before it (`{`+tab), only the
        // comment portion should be removed, not the whole line.
        assert_eq!(edit.range.start.line, 1);
        assert_eq!(edit.new_text, "");
    }

    #[test]
    fn finds_ignore_via_diag_line_when_message_line_differs() {
        // When the message references a different line than where the
        // diagnostic is reported, we should still find the comment via
        // the diagnostic line.
        let content = "<?php\necho $x; // @phpstan-ignore variable.undefined\necho $y;\n";
        // Message says "on line 3" (0-based: 2), but the diagnostic is
        // on line 1 where the comment actually is.
        let edit = build_remove_ignore_edit(content, 2, 1, Some("variable.undefined"));
        let edit = edit.unwrap();
        assert_eq!(edit.range.start.line, 1);
    }

    // ── ranges_overlap ──────────────────────────────────────────────

    #[test]
    fn overlapping_ranges() {
        let a = Range {
            start: Position {
                line: 5,
                character: 0,
            },
            end: Position {
                line: 5,
                character: 10,
            },
        };
        let b = Range {
            start: Position {
                line: 5,
                character: 5,
            },
            end: Position {
                line: 5,
                character: 15,
            },
        };
        assert!(ranges_overlap(&a, &b));
    }

    #[test]
    fn non_overlapping_ranges() {
        let a = Range {
            start: Position {
                line: 1,
                character: 0,
            },
            end: Position {
                line: 1,
                character: 5,
            },
        };
        let b = Range {
            start: Position {
                line: 3,
                character: 0,
            },
            end: Position {
                line: 3,
                character: 5,
            },
        };
        assert!(!ranges_overlap(&a, &b));
    }

    // ── is_ignorable ────────────────────────────────────────────────

    fn make_diag_with_data(data: Option<serde_json::Value>) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("method.visibility".to_string())),
            code_description: None,
            source: Some("phpstan".to_string()),
            message: "some error".to_string(),
            related_information: None,
            tags: None,
            data,
        }
    }

    #[test]
    fn ignorable_true_when_data_says_true() {
        let diag = make_diag_with_data(Some(json!({ "ignorable": true })));
        assert_eq!(is_ignorable(&diag), true);
    }

    #[test]
    fn ignorable_false_when_data_says_false() {
        let diag = make_diag_with_data(Some(json!({ "ignorable": false })));
        assert_eq!(is_ignorable(&diag), false);
    }

    #[test]
    fn ignorable_defaults_true_when_data_is_none() {
        let diag = make_diag_with_data(None);
        assert_eq!(is_ignorable(&diag), true);
    }

    #[test]
    fn ignorable_defaults_true_when_field_missing() {
        let diag = make_diag_with_data(Some(json!({})));
        assert_eq!(is_ignorable(&diag), true);
    }

    #[test]
    fn cursor_range_overlaps_full_line() {
        // Simulate a cursor (zero-width range) on a line with a
        // full-line PHPStan diagnostic.
        let cursor = Range {
            start: Position {
                line: 5,
                character: 10,
            },
            end: Position {
                line: 5,
                character: 10,
            },
        };
        let diag_range = Range {
            start: Position {
                line: 5,
                character: 0,
            },
            end: Position {
                line: 5,
                character: u32::MAX,
            },
        };
        assert!(ranges_overlap(&cursor, &diag_range));
    }
}
