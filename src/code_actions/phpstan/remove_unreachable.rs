//! "Remove unreachable code" code action for PHPStan
//! `deadCode.unreachable`.
//!
//! When PHPStan reports that a statement is unreachable (the code above
//! always terminates), this code action offers to delete ALL dead code
//! from the diagnostic line through the end of the enclosing block.
//! PHPStan only reports one `deadCode.unreachable` diagnostic per block
//! (on the first dead line), but everything after the terminating
//! statement in that block is dead and should be removed together.
//!
//! **Trigger:** A PHPStan diagnostic with identifier
//! `deadCode.unreachable` whose message is
//! `Unreachable statement - code above always terminates.` overlaps
//! the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_remove_unreachable_actions`) validates that the
//! action is applicable and emits a lightweight `CodeAction` with a
//! `data` payload but no `edit`.  Phase 2 (`resolve_remove_unreachable`)
//! recomputes the workspace edit on demand when the user picks the
//! action.
//!
//! ## Reusable helpers
//!
//! - [`build_remove_unreachable_block_edit`] removes everything from
//!   a line to the enclosing block's closing `}`.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::util::{find_brace_match_line, ranges_overlap};

// ── PHPStan identifier ──────────────────────────────────────────────────────

/// PHPStan identifier for the "unreachable statement" diagnostic.
const UNREACHABLE_ID: &str = "deadCode.unreachable";

/// Action kind string for removing unreachable code.
const ACTION_KIND: &str = "phpstan.removeUnreachable";

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Collect "Remove unreachable statement" code actions for PHPStan
    /// `deadCode.unreachable` diagnostics.
    ///
    /// Offer a "Remove unreachable code" action when the cursor is on
    /// a `deadCode.unreachable` diagnostic.
    ///
    /// The action removes ALL dead code from the diagnostic line to the
    /// enclosing block's closing `}`, not just a single statement.
    /// PHPStan only reports one diagnostic per block, but everything
    /// after the terminating statement is dead.
    pub(crate) fn collect_remove_unreachable_actions(
        &self,
        uri: &str,
        _content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let phpstan_diags: Vec<Diagnostic> = {
            let cache = self.phpstan_tool.last_diags.lock();
            cache.get(uri).cloned().unwrap_or_default()
        };

        // Only match diagnostics under the cursor.
        let cursor_hits: Vec<&Diagnostic> = phpstan_diags
            .iter()
            .filter(|diag| {
                matches!(
                    &diag.code,
                    Some(NumberOrString::String(s)) if s == UNREACHABLE_ID
                ) && ranges_overlap(&diag.range, &params.range)
            })
            .collect();

        for diag in &cursor_hits {
            let diag_line = diag.range.start.line as usize;

            let extra = serde_json::json!({
                "diagnostic_line": diag_line,
            });

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Remove unreachable code".to_string(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![(*diag).clone()]),
                edit: None,
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: Some(make_code_action_data(
                    ACTION_KIND,
                    uri,
                    &params.range,
                    extra,
                )),
            }));
        }
    }

    /// Resolve a "Remove unreachable code" code action by computing
    /// the full workspace edit.
    pub(crate) fn resolve_remove_unreachable(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let extra = &data.extra;
        let doc_uri: Url = data.uri.parse().ok()?;

        match data.action_kind.as_str() {
            ACTION_KIND => {
                let diag_line = extra.get("diagnostic_line")?.as_u64()? as usize;
                let edit = build_remove_unreachable_block_edit(content, diag_line)?;
                let mut changes = HashMap::new();
                changes.insert(doc_uri, vec![edit]);
                Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                })
            }
            _ => None,
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Build a [`TextEdit`] that removes all code from `diag_line` to the
/// enclosing block's closing `}`.
///
/// Everything from the start of `diag_line` to the line before the `}`
/// is deleted.  The `}` itself (which closes the function, if-block,
/// etc.) is preserved.
///
/// Returns `None` if the line is out of bounds or no enclosing `}` can
/// be found.
fn build_remove_unreachable_block_edit(content: &str, diag_line: usize) -> Option<TextEdit> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    // Find the enclosing `}` by scanning forward from the diagnostic
    // line.  Depth starts at 0; when it goes negative we've found the
    // enclosing block's close brace.
    let close_line = find_brace_match_line(&lines, diag_line, |d| d < 0)?;

    // Delete from the start of the diagnostic line to the start of the
    // closing brace line (preserving the `}` and its indentation).
    let start_pos = Position::new(diag_line as u32, 0);
    let end_pos = Position::new(close_line as u32, 0);

    Some(TextEdit {
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        new_text: String::new(),
    })
}

/// Check whether a `deadCode.unreachable` diagnostic is stale.
///
/// The diagnostic is considered stale when the diagnostic line is now
/// empty/whitespace-only or contains only a closing brace `}` (meaning
/// the unreachable statement has already been removed).
pub(crate) fn is_remove_unreachable_stale(content: &str, diag_line: usize) -> bool {
    let line_text = match content.lines().nth(diag_line) {
        Some(l) => l,
        None => return true, // line doesn't exist any more → stale
    };

    let trimmed = line_text.trim();
    trimmed.is_empty() || trimmed == "}"
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::find_semicolon_balanced;

    /// Build a [`TextEdit`] that removes a single semicolon-terminated
    /// statement starting on the given line.  Currently only used by
    /// tests; can be extracted to `pub(crate)` when the unreachable-code diagnostic needs it.
    fn build_remove_statement_edit(content: &str, diag_line: usize) -> Option<TextEdit> {
        let lines: Vec<&str> = content.lines().collect();
        if diag_line >= lines.len() {
            return None;
        }

        let line_text = lines[diag_line];
        let first_non_ws = line_text.find(|c: char| !c.is_whitespace())?;

        let line_start_byte: usize = lines[..diag_line].iter().map(|l| l.len() + 1).sum();

        let stmt_start_byte = line_start_byte + first_non_ws;
        let from_stmt = &content[stmt_start_byte..];
        let semi_offset = find_semicolon_balanced(from_stmt)?;
        let semi_byte = stmt_start_byte + semi_offset;
        let stmt_end_byte = semi_byte + 1;

        let delete_end_byte =
            if stmt_end_byte < content.len() && content.as_bytes()[stmt_end_byte] == b'\n' {
                stmt_end_byte + 1
            } else if stmt_end_byte < content.len() {
                let after_semi = &content[stmt_end_byte..];
                let next_nl = after_semi.find('\n');
                match next_nl {
                    Some(nl_off)
                        if content[stmt_end_byte..stmt_end_byte + nl_off]
                            .trim()
                            .is_empty() =>
                    {
                        stmt_end_byte + nl_off + 1
                    }
                    _ => stmt_end_byte,
                }
            } else {
                content.len()
            };

        let start_pos = Position::new(diag_line as u32, 0);
        let end_line = content[..delete_end_byte].matches('\n').count();
        let end_col = delete_end_byte
            - content[..delete_end_byte]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0);

        Some(TextEdit {
            range: Range {
                start: start_pos,
                end: Position::new(end_line as u32, end_col as u32),
            },
            new_text: String::new(),
        })
    }

    // ── build_remove_unreachable_block_edit ─────────────────────────

    #[test]
    fn removes_all_dead_code_to_closing_brace() {
        let content = "<?php\nfunction foo(): int {\n    return 1;\n    $b = 'second';\n    $a = 'first';\n    echo $a . $b;\n}\n";
        let edit = build_remove_unreachable_block_edit(content, 3).unwrap();
        assert_eq!(edit.new_text, "");
        assert_eq!(edit.range.start, Position::new(3, 0));
        // Should stop at the `}` line (line 6), preserving it.
        assert_eq!(edit.range.end, Position::new(6, 0));
    }

    #[test]
    fn removes_single_dead_statement_before_brace() {
        let content = "<?php\nfunction foo(): int {\n    return 1;\n    echo 'dead';\n}\n";
        let edit = build_remove_unreachable_block_edit(content, 3).unwrap();
        assert_eq!(edit.new_text, "");
        assert_eq!(edit.range.start, Position::new(3, 0));
        assert_eq!(edit.range.end, Position::new(4, 0));
    }

    #[test]
    fn handles_nested_braces_in_dead_code() {
        // Dead code contains an if block with braces.
        let content = "<?php\nfunction foo(): int {\n    return 1;\n    if (true) {\n        echo 'nested';\n    }\n    echo 'also dead';\n}\n";
        let edit = build_remove_unreachable_block_edit(content, 3).unwrap();
        assert_eq!(edit.range.start, Position::new(3, 0));
        // The dead code includes the if block and the echo after it.
        // The function's `}` on line 7 is preserved.
        assert_eq!(edit.range.end, Position::new(7, 0));
    }

    #[test]
    fn removes_dead_code_inside_if_block() {
        let content = "<?php\nif (true) {\n    return;\n    echo 'dead';\n}\necho 'alive';\n";
        let edit = build_remove_unreachable_block_edit(content, 3).unwrap();
        assert_eq!(edit.range.start, Position::new(3, 0));
        // Preserves the if block's `}` on line 4.
        assert_eq!(edit.range.end, Position::new(4, 0));
    }

    #[test]
    fn returns_none_for_invalid_line() {
        let content = "<?php\n";
        assert!(build_remove_unreachable_block_edit(content, 5).is_none());
    }

    #[test]
    fn returns_none_when_no_closing_brace() {
        // Malformed — no closing brace after the dead code.
        let content = "<?php\nreturn;\necho 'dead';";
        assert!(build_remove_unreachable_block_edit(content, 2).is_none());
    }

    // ── build_remove_statement_edit (still used by unreachable-code diagnostic) ──────────────

    #[test]
    fn removes_simple_statement() {
        let content =
            "<?php\nfunction foo(): never { throw new \\Exception(); }\necho 'unreachable';\n";
        let edit = build_remove_statement_edit(content, 2).unwrap();
        assert_eq!(edit.new_text, "");
        assert_eq!(edit.range.start, Position::new(2, 0));
    }

    #[test]
    fn removes_indented_statement() {
        let content = "<?php\nif (true) {\n    return;\n    echo 'dead';\n}\n";
        let edit = build_remove_statement_edit(content, 3).unwrap();
        assert_eq!(edit.new_text, "");
        assert_eq!(edit.range.start, Position::new(3, 0));
        assert_eq!(edit.range.end, Position::new(4, 0));
    }

    #[test]
    fn handles_string_with_semicolons() {
        let content = "<?php\nreturn;\necho 'a;b;c';\n";
        let edit = build_remove_statement_edit(content, 2).unwrap();
        assert_eq!(edit.new_text, "");
        assert_eq!(edit.range.start, Position::new(2, 0));
        assert_eq!(edit.range.end, Position::new(3, 0));
    }

    // ── is_remove_unreachable_stale ─────────────────────────────────

    #[test]
    fn stale_when_line_empty() {
        assert!(is_remove_unreachable_stale("<?php\n\n", 1));
    }

    #[test]
    fn stale_when_line_is_close_brace() {
        assert!(is_remove_unreachable_stale("<?php\n}\n", 1));
    }

    #[test]
    fn not_stale_when_code_present() {
        assert!(!is_remove_unreachable_stale("<?php\necho 'hi';\n", 1));
    }

    #[test]
    fn stale_when_line_gone() {
        assert!(is_remove_unreachable_stale("<?php\n", 5));
    }

    #[test]
    fn stale_when_line_is_whitespace() {
        assert!(is_remove_unreachable_stale("<?php\n    \n", 1));
    }

    #[test]
    fn stale_when_line_is_indented_close_brace() {
        assert!(is_remove_unreachable_stale("<?php\n    }\n", 1));
    }

    #[test]
    fn not_stale_when_different_statement() {
        assert!(!is_remove_unreachable_stale("<?php\n$x = 1;\n", 1));
    }
}
