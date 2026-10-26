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
//! - [`build_remove_unreachable_block_edit`] removes everything from the
//!   dead statement to the end of the statement list it sits in.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::code_actions::phpstan::phpstan_identifier;
use crate::code_actions::{CodeActionData, deferred_quickfix};
use crate::text_position::{byte_range_to_lsp_range, line_start_byte_offset};

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
        out: &mut Vec<CodeActionResponse>,
    ) {
        for diag in self.phpstan_diags_overlapping(uri, &params.range) {
            if phpstan_identifier(&diag) != Some(UNREACHABLE_ID) {
                continue;
            }
            let _diag_line = diag.range.start.line as usize;

            out.push(deferred_quickfix(
                "Remove unreachable code".to_string(),
                ACTION_KIND,
                uri,
                &params.range,
                vec![diag],
                true,
                serde_json::Value::Null,
            ));
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
        let doc_uri: Uri = data.uri.parse().ok()?;

        match data.action_kind.as_str() {
            ACTION_KIND => {
                let diag_line = extra.get("diagnostic_line")?.as_u64()? as usize;
                let edit = build_remove_unreachable_block_edit(content, diag_line)?;
                Some(crate::code_actions::single_file_edit(doc_uri, vec![edit]))
            }
            _ => None,
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Build a [`TextEdit`] that removes the statement PHPStan reports on
/// `diag_line` and every statement after it in the list it sits in: the
/// rest of the block, `switch` case or body, which the statement before it
/// never lets run.  The `}` (or the next `case`) that ends the list stays.
///
/// Whole lines go where the dead statements have them to themselves.
///
/// Returns `None` if no statement starts on the line.
fn build_remove_unreachable_block_edit(content: &str, diag_line: usize) -> Option<TextEdit> {
    let line_start = line_start_byte_offset(content, diag_line);
    let line = content.get(line_start..)?.lines().next()?;
    let offset = (line_start + line.len() - line.trim_start().len()) as u32;
    let line_end = (line_start + line.len()) as u32;
    let (first, last) =
        crate::parser::with_parsed_program(content, "remove_unreachable", |program, _content| {
            crate::code_actions::cursor_context::innermost_at(program, offset, |node| {
                let statements = statement_list_at(node, offset)?;
                let dead = statements
                    .iter()
                    .position(|statement| statement.span().end.offset > offset)?;
                // The dead statement is the one that starts on the line; a
                // list whose statement only spans it holds the line deeper
                // down, if anywhere.
                let start = statements[dead].span().start.offset;
                if start < offset || start > line_end {
                    return None;
                }
                Some((
                    start as usize,
                    statements.last()?.span().end.offset as usize,
                ))
            })
        })?;

    let first_line_start = content[..first].rfind('\n').map_or(0, |at| at + 1);
    let start = if content[first_line_start..first].trim().is_empty() {
        first_line_start
    } else {
        first
    };
    let last_line_end = content[last..]
        .find('\n')
        .map_or(content.len(), |at| last + at);
    let end = if content[last..last_line_end].trim().is_empty() {
        (last_line_end + 1).min(content.len())
    } else {
        last
    };

    Some(TextEdit {
        range: byte_range_to_lsp_range(content, start, end),
        new_text: String::new(),
    })
}

/// The statement list `node` holds that the code at `offset` sits in: a
/// block's, the file's, or one nested in a statement (a `switch` case, a
/// `:`-delimited body, a namespace).
fn statement_list_at<'a>(node: Node<'a, '_>, offset: u32) -> Option<&'a [Statement<'a>]> {
    let holds = |statements: &[Statement<'_>]| {
        statements.iter().any(|statement| {
            let span = statement.span();
            span.start.offset <= offset && offset <= span.end.offset
        })
    };
    let statements = match node {
        Node::Program(program) => program.statements.as_slice(),
        Node::Block(block) => block.statements.as_slice(),
        Node::Statement(statement) => {
            crate::parser::try_for_each_nested_statement_list(statement, |statements| {
                if holds(statements) {
                    std::ops::ControlFlow::Break(statements)
                } else {
                    std::ops::ControlFlow::Continue(())
                }
            })
            .break_value()?
        }
        _ => return None,
    };
    holds(statements).then_some(statements)
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
    fn removes_dead_code_at_the_top_level() {
        let content = "<?php\nreturn;\necho 'dead';\necho 'also dead';\n";
        let edit = build_remove_unreachable_block_edit(content, 2).unwrap();
        assert_eq!(edit.range.start, Position::new(2, 0));
        assert_eq!(edit.range.end, Position::new(4, 0));
    }

    #[test]
    fn dead_code_in_a_switch_case_stops_at_the_next_case() {
        let content = "<?php\nswitch ($x) {\n    case 1:\n        return 1;\n        echo 'dead';\n    case 2:\n        echo 'alive';\n}\n";
        let edit = build_remove_unreachable_block_edit(content, 4).unwrap();
        assert_eq!(edit.range.start, Position::new(4, 0));
        assert_eq!(edit.range.end, Position::new(5, 0));
    }

    #[test]
    fn braces_in_strings_do_not_end_the_block() {
        let content =
            "<?php\nfunction foo(): int {\n    return 1;\n    echo '}';\n    echo 'dead';\n}\n";
        let edit = build_remove_unreachable_block_edit(content, 3).unwrap();
        assert_eq!(edit.range.start, Position::new(3, 0));
        assert_eq!(edit.range.end, Position::new(5, 0));
    }

    #[test]
    fn returns_none_on_a_line_with_no_statement() {
        let content = "<?php\nfunction foo(): int {\n    return 1;\n\n}\n";
        assert!(build_remove_unreachable_block_edit(content, 3).is_none());
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
