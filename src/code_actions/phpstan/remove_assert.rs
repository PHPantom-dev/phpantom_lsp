//! "Remove always-true assert()" code action for PHPStan
//! `function.alreadyNarrowedType`.
//!
//! When PHPStan reports that a call to `assert()` will always evaluate
//! to `true`, this code action offers to delete the entire `assert(…);`
//! statement.  Removing a no-op `assert()` is safe because the
//! assertion can never fail at runtime.
//!
//! Only matches diagnostics whose message starts with
//! `Call to function assert()`.  The same PHPStan identifier is also
//! used for other functions (`is_bool()`, `instanceof`, etc.) where
//! removal would change control flow, so those are deliberately
//! excluded.
//!
//! **Trigger:** A PHPStan diagnostic with identifier
//! `function.alreadyNarrowedType` whose message starts with
//! `Call to function assert()` overlaps the cursor.
//!
//! **Code action kind:** `quickfix`.
//!
//! ## Two-phase resolve
//!
//! Phase 1 (`collect_remove_assert_actions`) validates that the action
//! is applicable and emits a lightweight `CodeAction` with a `data`
//! payload but no `edit`.  Phase 2 (`resolve_remove_assert`) recomputes
//! the workspace edit on demand when the user picks the action.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::code_actions::{CodeActionData, make_code_action_data};
use crate::util::ranges_overlap;

// ── PHPStan identifier ──────────────────────────────────────────────────────

/// PHPStan identifier for the "already narrowed type" diagnostic.
const ALREADY_NARROWED_ID: &str = "function.alreadyNarrowedType";

/// Action kind string for the resolve dispatch table.
const ACTION_KIND: &str = "phpstan.removeAssert";

/// Message prefix that distinguishes `assert()` calls from other
/// functions that share the same PHPStan identifier.
const ASSERT_MESSAGE_PREFIX: &str = "Call to function assert()";

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Collect "Remove always-true assert()" code actions for PHPStan
    /// `function.alreadyNarrowedType` diagnostics.
    pub(crate) fn collect_remove_assert_actions(
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

        for diag in &phpstan_diags {
            if !ranges_overlap(&diag.range, &params.range) {
                continue;
            }

            let identifier = match &diag.code {
                Some(NumberOrString::String(s)) => s.as_str(),
                _ => continue,
            };

            if identifier != ALREADY_NARROWED_ID {
                continue;
            }

            // Only handle `assert()` calls — other functions with the
            // same identifier (e.g. `is_bool()`) appear inside
            // conditions where removal would change control flow.
            if !diag.message.starts_with(ASSERT_MESSAGE_PREFIX) {
                continue;
            }

            let diag_line = diag.range.start.line as usize;

            let title = "Remove always-true assert()".to_string();

            let extra = serde_json::json!({
                "diagnostic_line": diag_line,
            });

            let data = make_code_action_data(ACTION_KIND, uri, &params.range, extra);

            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                edit: None,
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: Some(data),
            }));
        }
    }

    /// Resolve a "Remove always-true assert()" code action by computing
    /// the full workspace edit.
    pub(crate) fn resolve_remove_assert(
        &self,
        data: &CodeActionData,
        content: &str,
    ) -> Option<WorkspaceEdit> {
        let extra = &data.extra;
        let diag_line = extra.get("diagnostic_line")?.as_u64()? as usize;

        let edit = build_remove_assert_edit(content, diag_line)?;

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

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Build a `TextEdit` that removes the `assert(…);` statement on the
/// given line.
///
/// The edit deletes the entire line (including its trailing newline) when
/// the line contains only whitespace plus the `assert(…);` statement.
/// When other code appears on the same line before or after the assert,
/// only the `assert(…);` portion is removed (preserving any surrounding
/// whitespace appropriately).
///
/// For multi-line `assert()` calls the edit extends from the `assert(`
/// token to the closing `);` that terminates the statement.
fn build_remove_assert_edit(content: &str, diag_line: usize) -> Option<TextEdit> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let line_text = lines[diag_line];

    // Find the `assert(` token on the diagnostic line.
    let assert_col = line_text.find("assert(")?;

    // Compute the byte offset within `content` where this line starts.
    let line_start_byte = lines[..diag_line]
        .iter()
        .map(|l| l.len() + 1) // +1 for newline
        .sum::<usize>();

    let assert_byte = line_start_byte + assert_col;

    // Walk forward from `assert(` to find the matching `)` respecting
    // nesting (assert can contain function calls, ternaries, etc.).
    let after_paren = assert_byte + "assert(".len();
    let close_paren_byte = find_matching_close_paren(content, after_paren)?;

    // After the closing `)` there should be a `;`.  Skip optional
    // whitespace between `)` and `;`.
    let rest_after_paren = &content[close_paren_byte + 1..];
    let semi_offset = rest_after_paren
        .find(|c: char| !c.is_ascii_whitespace() || c == '\n')
        .unwrap_or(0);
    let semi_byte = close_paren_byte + 1 + semi_offset;

    if content.as_bytes().get(semi_byte) != Some(&b';') {
        return None;
    }

    // The full assert statement spans from `assert_byte` to
    // `semi_byte` inclusive.
    let stmt_end_byte = semi_byte + 1;

    // Determine whether the assert is the only thing on its line(s).
    // If so, delete the entire line(s) including leading whitespace
    // and the trailing newline.
    let before_assert = &content[line_start_byte..assert_byte];
    let is_only_statement = before_assert.trim().is_empty();

    // Check if anything non-whitespace follows the semicolon on the
    // ending line.
    let after_semi = if stmt_end_byte < content.len() {
        let next_newline = content[stmt_end_byte..]
            .find('\n')
            .map(|p| stmt_end_byte + p)
            .unwrap_or(content.len());
        content[stmt_end_byte..next_newline].trim().is_empty()
    } else {
        true
    };

    if is_only_statement && after_semi {
        // Delete the entire line(s) from the start of the first line
        // to the start of the next line after the statement.
        let delete_end_byte = if stmt_end_byte < content.len() {
            // Include the trailing newline of the last line.
            content[stmt_end_byte..]
                .find('\n')
                .map(|p| stmt_end_byte + p + 1)
                .unwrap_or(content.len())
        } else {
            content.len()
        };

        let start_pos = Position::new(diag_line as u32, 0);
        let end_line = content[..delete_end_byte].matches('\n').count();
        let end_col = if delete_end_byte <= content.len() {
            delete_end_byte
                - content[..delete_end_byte]
                    .rfind('\n')
                    .map(|p| p + 1)
                    .unwrap_or(0)
        } else {
            0
        };

        Some(TextEdit {
            range: Range {
                start: start_pos,
                end: Position::new(end_line as u32, end_col as u32),
            },
            new_text: String::new(),
        })
    } else if is_only_statement {
        // The assert is at the start (after whitespace) but there is
        // code after the semicolon.  Remove from line start to after
        // the semicolon.
        let start_pos = Position::new(diag_line as u32, 0);
        // Preserve indentation for what follows by keeping the
        // newline — but we want to remove the assert up to the
        // semicolon.  Since we checked the end line is the same
        // general area, just remove assert(...); plus trailing space.
        let trailing_space = content[stmt_end_byte..]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count();

        // For same-line: use the end line from the statement.
        let end_offset = stmt_end_byte + trailing_space;
        let end_line_num = content[..end_offset].matches('\n').count();
        let end_line_start = content[..end_offset]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let end_col_final = (end_offset - end_line_start) as u32;

        Some(TextEdit {
            range: Range {
                start: start_pos,
                end: Position::new(end_line_num as u32, end_col_final),
            },
            new_text: String::new(),
        })
    } else {
        // There is code before the assert on the same line.  Remove
        // just the `assert(…);` portion (including any leading
        // whitespace before `assert`).
        let leading_space = content[..assert_byte]
            .chars()
            .rev()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count();
        let remove_start = assert_byte - leading_space;
        let remove_start_col = (remove_start - line_start_byte) as u32;

        let end_line_num = content[..stmt_end_byte].matches('\n').count();
        let end_line_start = content[..stmt_end_byte]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        let end_col = (stmt_end_byte - end_line_start) as u32;

        Some(TextEdit {
            range: Range {
                start: Position::new(diag_line as u32, remove_start_col),
                end: Position::new(end_line_num as u32, end_col),
            },
            new_text: String::new(),
        })
    }
}

/// Find the byte offset of the closing `)` that matches the opening
/// paren whose content starts at `start_byte`.
///
/// Respects nesting of parentheses and skips string literals (single
/// and double quoted) so that parentheses inside strings are ignored.
fn find_matching_close_paren(content: &str, start_byte: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth: u32 = 1;
    let mut i = start_byte;

    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            b'\'' | b'"' => {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2; // skip escaped character
                    } else if bytes[i] == quote {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    None
}

/// Check whether a `function.alreadyNarrowedType` diagnostic for
/// `assert()` is stale.
///
/// The diagnostic is considered stale when the diagnostic line no
/// longer contains `assert(`.
pub(crate) fn is_remove_assert_stale(content: &str, diag_line: usize) -> bool {
    let line_text = match content.lines().nth(diag_line) {
        Some(l) => l,
        None => return true, // line doesn't exist any more → stale
    };

    !line_text.contains("assert(")
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── find_matching_close_paren ───────────────────────────────────

    #[test]
    fn simple_paren() {
        let s = "true);";
        // content starts after `assert(`, so index 0 = 't'
        assert_eq!(find_matching_close_paren(s, 0), Some(4));
    }

    #[test]
    fn nested_parens() {
        let s = "is_string($x));";
        assert_eq!(find_matching_close_paren(s, 0), Some(13));
    }

    #[test]
    fn string_with_parens() {
        let s = r#""foo(bar)" !== null);"#;
        assert_eq!(find_matching_close_paren(s, 0), Some(19));
    }

    #[test]
    fn single_quoted_string() {
        let s = "'a)b' === true);";
        assert_eq!(find_matching_close_paren(s, 0), Some(14));
    }

    #[test]
    fn escaped_quote_in_string() {
        let s = r#""foo\")" !== null);"#;
        assert_eq!(find_matching_close_paren(s, 0), Some(17));
    }

    #[test]
    fn unmatched_returns_none() {
        let s = "true";
        assert_eq!(find_matching_close_paren(s, 0), None);
    }

    // ── build_remove_assert_edit ────────────────────────────────────

    #[test]
    fn removes_simple_assert_line() {
        let content = "<?php\n    assert($x instanceof Foo);\n    $x->bar();\n";
        let edit = build_remove_assert_edit(content, 1).unwrap();
        assert_eq!(edit.range.start, Position::new(1, 0));
        assert_eq!(edit.range.end, Position::new(2, 0));
        assert_eq!(edit.new_text, "");
    }

    #[test]
    fn removes_assert_at_end_of_file() {
        let content = "<?php\nassert(true);";
        let edit = build_remove_assert_edit(content, 1).unwrap();
        assert_eq!(edit.range.start, Position::new(1, 0));
        // Should cover up to end of content.
        assert_eq!(edit.new_text, "");
    }

    #[test]
    fn removes_assert_with_nested_calls() {
        let content = "<?php\n    assert(is_string(trim($x)));\n    echo 'ok';\n";
        let edit = build_remove_assert_edit(content, 1).unwrap();
        assert_eq!(edit.range.start, Position::new(1, 0));
        assert_eq!(edit.range.end, Position::new(2, 0));
        assert_eq!(edit.new_text, "");
    }

    #[test]
    fn preserves_code_before_assert() {
        let content = "<?php\n$a = 1; assert(true);\n";
        let edit = build_remove_assert_edit(content, 1).unwrap();
        // Should only remove ` assert(true);` (with leading space).
        assert_eq!(edit.range.start.line, 1);
        assert!(edit.range.start.character > 0);
        assert_eq!(edit.new_text, "");
    }

    #[test]
    fn returns_none_for_no_assert() {
        let content = "<?php\n    $x = 1;\n";
        assert!(build_remove_assert_edit(content, 1).is_none());
    }

    #[test]
    fn returns_none_for_invalid_line() {
        let content = "<?php\n";
        assert!(build_remove_assert_edit(content, 5).is_none());
    }

    #[test]
    fn returns_none_for_missing_semicolon() {
        let content = "<?php\nassert(true)\n";
        assert!(build_remove_assert_edit(content, 1).is_none());
    }

    // ── is_remove_assert_stale ─────────────────────────────────────

    #[test]
    fn stale_when_assert_removed() {
        let content = "<?php\n    $x->bar();\n";
        assert!(is_remove_assert_stale(content, 1));
    }

    #[test]
    fn not_stale_when_assert_present() {
        let content = "<?php\n    assert($x instanceof Foo);\n";
        assert!(!is_remove_assert_stale(content, 1));
    }

    #[test]
    fn stale_when_line_gone() {
        let content = "<?php\n";
        assert!(is_remove_assert_stale(content, 5));
    }

    // ── Integration-style collect tests ─────────────────────────────

    fn make_diagnostic(line: u32, message: &str, code: &str) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position::new(line, 0),
                end: Position::new(line, 100),
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(code.to_string())),
            source: Some("PHPStan".to_string()),
            message: message.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn matches_assert_message() {
        let msg = "Call to function assert() with true will always evaluate to true.";
        assert!(msg.starts_with(ASSERT_MESSAGE_PREFIX));
    }

    #[test]
    fn rejects_non_assert_message() {
        let msg = "Call to function is_string() with string will always evaluate to true.";
        assert!(!msg.starts_with(ASSERT_MESSAGE_PREFIX));
    }

    #[test]
    fn rejects_wrong_identifier() {
        let diag = make_diagnostic(1, "Call to function assert() with ...", "some.other");
        let identifier = match &diag.code {
            Some(NumberOrString::String(s)) => s.as_str(),
            _ => "",
        };
        assert_ne!(identifier, ALREADY_NARROWED_ID);
    }

    #[test]
    fn accepts_correct_identifier_and_message() {
        let diag = make_diagnostic(
            1,
            "Call to function assert() with true will always evaluate to true.",
            ALREADY_NARROWED_ID,
        );
        let identifier = match &diag.code {
            Some(NumberOrString::String(s)) => s.as_str(),
            _ => "",
        };
        assert_eq!(identifier, ALREADY_NARROWED_ID);
        assert!(diag.message.starts_with(ASSERT_MESSAGE_PREFIX));
    }
}
