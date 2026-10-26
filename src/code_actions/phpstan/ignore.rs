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

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{CodeActionData, deferred_quickfix};
use crate::text_position::byte_offset_to_utf16_col;

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
        out: &mut Vec<CodeActionResponse>,
    ) {
        // Look at all cached PHPStan diagnostics for the file; overlap
        // with the requested range is checked per-diagnostic below.
        let phpstan_diags = self.phpstan_diags_overlapping(uri, &params.range);

        // ── Add @phpstan-ignore ─────────────────────────────────────────
        // For each PHPStan diagnostic overlapping the request range that
        // has a usable identifier, offer to add an ignore comment.
        // Group diagnostics by (line, identifier) so that multiple
        // diagnostics with the same identifier on the same line produce
        // a single code action with all of them attached.
        let mut ignore_groups: HashMap<(u32, String), Vec<Diagnostic>> = HashMap::new();
        for diag in &phpstan_diags {
            let Some(identifier) = phpstan_identifier(diag) else {
                continue;
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

            out.push(deferred_quickfix(
                format!("Ignore PHPStan error ({})", identifier),
                "phpstan.addIgnore",
                uri,
                &params.range,
                diags.clone(),
                false,
                extra,
            ));
        }

        // ── Remove unnecessary @phpstan-ignore ──────────────────────────
        // When PHPStan reports an unmatched ignore, offer to remove it.
        for diag in &phpstan_diags {
            let Some(identifier) = phpstan_identifier(diag) else {
                continue;
            };

            if !identifier.starts_with(UNMATCHED_IGNORE_PREFIX) {
                continue;
            }

            // Parse the identifier and line from the message.
            // PHPStan message format:
            //   "No error with identifier <id> is reported on line <N>."
            let ignore_id = parse_unmatched_identifier(crate::util::message_text(&diag.message));
            out.push(deferred_quickfix(
                match ignore_id {
                    Some(ref id) => format!("Remove unnecessary @phpstan-ignore ({})", id),
                    None => "Remove unnecessary @phpstan-ignore".to_string(),
                },
                "phpstan.removeIgnore",
                uri,
                &params.range,
                vec![diag.clone()],
                true,
                serde_json::Value::Null,
            ));
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

        let doc_uri: Uri = data.uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, vec![edit]))
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

        let doc_uri: Uri = data.uri.parse().ok()?;
        Some(crate::code_actions::single_file_edit(doc_uri, vec![edit]))
    }
}

/// Build a `TextEdit` that adds a `@phpstan-ignore` comment to a line.
///
/// If the line already contains `@phpstan-ignore`, the identifier is
/// appended to the existing comma-separated list.  Otherwise, a
/// `// @phpstan-ignore <id>` comment is inserted at the end of the line.
fn build_add_ignore_edit(content: &str, line: u32, line_text: &str, identifier: &str) -> TextEdit {
    // A `@phpstan-ignore-line` or `@phpstan-ignore-next-line` is left
    // alone, and a new comment is added instead.
    let Some(comment) =
        crate::phpstan_ignore::parse_ignore_comment(line_text).filter(|c| !c.blanket)
    else {
        return build_eol_comment(content, line, line_text, identifier);
    };

    if comment.ids(line_text).any(|id| id == identifier) {
        // Already ignored — return a no-op edit.
        return TextEdit {
            range: Range {
                start: Position { line, character: 0 },
                end: Position { line, character: 0 },
            },
            new_text: String::new(),
        };
    }

    // Insert the new identifier after the last entry, past its reason,
    // so an existing reason stays with the identifier it explains.  The
    // offset is in bytes; LSP positions are UTF-16 code units, so convert
    // before emitting the edit.
    let insert_byte = comment.list.end;
    let insert_col = byte_offset_to_utf16_col(line_text, insert_byte);
    let separator = if !comment.list.is_empty() {
        ", "
    } else if line_text[..insert_byte].ends_with(char::is_whitespace) {
        ""
    } else {
        " "
    };

    TextEdit {
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
    }
}

/// Build a `TextEdit` that appends `// @phpstan-ignore <id>` at the end
/// of a line.
fn build_eol_comment(_content: &str, line: u32, line_text: &str, identifier: &str) -> TextEdit {
    // LSP positions are UTF-16 code units, not bytes: convert the end-of-line
    // byte offset so the comment lands correctly on multibyte lines.
    let end_col = byte_offset_to_utf16_col(line_text, line_text.len());
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

        if let Some(comment) = crate::phpstan_ignore::parse_ignore_comment(line_text) {
            // `-line` / `-next-line` variants have no identifier list to
            // trim, so always remove the whole comment.  With a specific
            // identifier to remove, try to remove just that one from the
            // list.
            return match remove_id {
                Some(id) if !comment.blanket => {
                    build_remove_single_id(content, check_line, line_text, &comment, id)
                }
                _ => build_remove_whole_ignore(content, check_line, line_text, comment.tag_start),
            };
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
                            character: byte_offset_to_utf16_col(
                                prev_line_text,
                                prev_line_text.len(),
                            ),
                        },
                        end: Position {
                            line,
                            character: byte_offset_to_utf16_col(line_text, line_text.len()),
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
                            character: byte_offset_to_utf16_col(line_text, line_text.len()),
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

        let trim_start = before_comment.len();

        Some(TextEdit {
            range: Range {
                start: Position {
                    line,
                    character: byte_offset_to_utf16_col(line_text, trim_start),
                },
                end: Position {
                    line,
                    character: byte_offset_to_utf16_col(line_text, end_col),
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
    comment: &crate::phpstan_ignore::IgnoreComment,
    remove_id: &str,
) -> Option<TextEdit> {
    let ids: Vec<&str> = comment.ids(line_text).collect();
    if ids.len() <= 1 || !ids.contains(&remove_id) {
        // Only one identifier, or the one we are supposed to remove is
        // not here: remove the whole comment.
        return build_remove_whole_ignore(content, line, line_text, comment.tag_start);
    }

    // Rewrite the list without this identifier, keeping every other
    // entry's reason.
    let new_list = comment
        .entries
        .iter()
        .filter(|entry| !entry.span.is_empty() && line_text[entry.id.clone()] != *remove_id)
        .map(|entry| &line_text[entry.span.clone()])
        .collect::<Vec<_>>()
        .join(", ");

    Some(TextEdit {
        range: Range {
            start: Position {
                line,
                character: byte_offset_to_utf16_col(line_text, comment.list.start),
            },
            end: Position {
                line,
                character: byte_offset_to_utf16_col(line_text, comment.list.end),
            },
        },
        new_text: new_list,
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
    use crate::text_position::{apply_text_edits, ranges_overlap};
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
        // A reason belongs to the identifier before it, so the new one
        // goes after it.
        assert_eq!(edit.range.start.character, line_text.len() as u32);
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
    fn appends_after_a_reason_that_contains_a_comma() {
        let line_text = "echo $x; // @phpstan-ignore argument.type (legacy, see #12)";
        let content = format!("<?php\n{line_text}\n");
        let edit = build_add_ignore_edit(&content, 1, line_text, "return.type");
        assert_eq!(edit.new_text, ", return.type");
        assert_eq!(edit.range.start.character, line_text.len() as u32);
    }

    #[test]
    fn adds_a_space_after_a_bare_tag() {
        let line_text = "echo $x; // @phpstan-ignore";
        let content = format!("<?php\n{line_text}\n");
        let edit = build_add_ignore_edit(&content, 1, line_text, "return.type");
        assert_eq!(edit.new_text, " return.type");
    }

    #[test]
    fn removing_an_id_keeps_the_other_entries_reasons() {
        let content = "<?php\necho $x; // @phpstan-ignore argument.type (legacy, see #12), return.type (f(x))\n";
        let edit = build_remove_ignore_edit(content, 1, 1, Some("return.type")).unwrap();
        assert_eq!(edit.new_text, "argument.type (legacy, see #12)");
        let edit = build_remove_ignore_edit(content, 1, 1, Some("argument.type")).unwrap();
        assert_eq!(edit.new_text, "return.type (f(x))");
    }

    #[test]
    fn removal_columns_are_utf16_after_multibyte_code() {
        // "é" is two bytes but one UTF-16 code unit, so a byte offset
        // would land the edit one column too far right.
        let content = "<?php\necho \"é\"; // @phpstan-ignore variable.undefined, argument.type\n";
        let edit = build_remove_ignore_edit(content, 1, 1, Some("argument.type")).unwrap();
        assert_eq!(
            apply_text_edits(content, &[edit]),
            "<?php\necho \"é\"; // @phpstan-ignore variable.undefined\n"
        );

        let content = "<?php\necho \"é\"; // @phpstan-ignore variable.undefined\n";
        let edit = build_remove_ignore_edit(content, 1, 1, None).unwrap();
        assert_eq!(apply_text_edits(content, &[edit]), "<?php\necho \"é\";\n");
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
            severity: Some(DiagnosticSeverity::Error),
            code: Some(Code::String("method.visibility".to_string())),
            code_description: None,
            source: Some("phpstan".to_string()),
            message: "some error".to_string().into(),
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
