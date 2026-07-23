//! `TextEdit` builders for the "Fix return type" code actions, plus
//! the source-navigation helpers they share (locating the enclosing
//! function's braces, parameter list, and return-type region).

use tower_lsp::lsp_types::{Position, Range, TextEdit};

use crate::code_actions::phpstan::add_iterable_type::{
    find_function_docblock, find_function_keyword_line as find_func_keyword_line,
};
use crate::docblock::type_strings::split_type_token;
use crate::php_type::PhpType;
use crate::util::find_semicolon_balanced;

use super::inference::InferredReturnType;

// ── Edit builders ───────────────────────────────────────────────────────────

/// Build a `TextEdit` that fixes `return {expr};` in a void function.
///
/// The replacement depends on context:
///
/// - **`return null;`** → `return;` (null is not a meaningful value).
/// - **All other expressions** → `{expr};\n{indent}return;` (keep
///   the expression as a standalone statement and add a bare
///   `return;` on the next line).
///
/// When the return is the last statement before the function's closing
/// `}`, the bare `return;` is omitted since it would be redundant.
///
/// Handles multiline return expressions by scanning forward from the
/// `return` keyword to the matching `;`, respecting string literals and
/// parenthesis nesting.
pub(super) fn build_strip_return_expr_edit(content: &str, diag_line: usize) -> Option<TextEdit> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let line_text = lines[diag_line];

    // Find `return ` (with trailing space) on the diagnostic line.
    let return_col = line_text.find("return ")?;

    // Verify this is not `return;` (no expression).
    let after_return = &line_text[return_col + "return".len()..];
    let trimmed = after_return.trim();
    if trimmed == ";" {
        // Already a bare return — nothing to fix.
        return None;
    }

    // Compute the byte offset within `content` where this line starts.
    let line_start_byte: usize = lines[..diag_line]
        .iter()
        .map(|l| l.len() + 1) // +1 for newline
        .sum();

    // The return statement starts at `return` keyword.
    let return_byte = line_start_byte + return_col;

    // Walk forward from after `return` to find the terminating `;`,
    // respecting string literals and balanced parentheses.
    let after_keyword_byte = return_byte + "return".len();
    let semi_offset = find_semicolon_balanced(&content[after_keyword_byte..])?;
    let semi_byte = after_keyword_byte + semi_offset;

    // Build the replacement range: from `return` keyword through `;`.
    let stmt_end_byte = semi_byte + 1;

    // Compute line/col for the start (the `return` keyword).
    let start_line = diag_line as u32;
    let start_char = return_col as u32;

    // Compute line/col for the end (after `;`).
    let end_line = content[..stmt_end_byte].matches('\n').count() as u32;
    let end_line_start = content[..stmt_end_byte]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let end_char = (stmt_end_byte - end_line_start) as u32;

    // Extract the expression text (between `return ` and `;`).
    let expr_start = return_byte + "return ".len();
    let expr_text = content[expr_start..semi_byte].trim();

    // Case 1: `return null;` → just replace with `return;`.
    if expr_text == "null" {
        return Some(TextEdit {
            range: Range {
                start: Position::new(start_line, start_char),
                end: Position::new(end_line, end_char),
            },
            new_text: "return;".to_string(),
        });
    }

    // Capture the indentation of the return line.
    let indent = &line_text[..return_col];

    // Check whether this return is the last statement in the function
    // body.  If the only thing between the `;` and the function's
    // closing `}` is whitespace, the `return;` is redundant.
    let needs_bare_return = !is_last_statement_in_function(content, stmt_end_byte);

    let new_text = if needs_bare_return {
        format!("{};\n{}return;", expr_text, indent)
    } else {
        format!("{};", expr_text)
    };

    Some(TextEdit {
        range: Range {
            start: Position::new(start_line, start_char),
            end: Position::new(end_line, end_char),
        },
        new_text,
    })
}

/// Build a list of `TextEdit`s that change the enclosing function's
/// return type to `target_type` and, when the target is `void`,
/// optionally remove the `@return` docblock tag.
///
/// Returns `None` if the enclosing function cannot be found or its
/// return type already matches `target_type`.
pub(super) fn build_change_return_type_edits_to(
    content: &str,
    diag_line: usize,
    target_type: &PhpType,
) -> Option<Vec<TextEdit>> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let mut edits = Vec::new();

    // ── Step 1: Find the opening `{` of the function body ───────────
    // The diagnostic is on a `return` statement inside the body, so
    // search backward to find the enclosing function's opening brace.
    let brace_line = find_function_open_brace_line(&lines, diag_line)?;

    // ── Step 2: Find the `)` that closes the parameter list ─────────
    let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line)?;

    // ── Step 3: Find the return type hint between `)` and `{` ───────
    let target_str = target_type.to_string();
    let type_edit = find_return_type_edit(&lines, paren_line, paren_col, brace_line, &target_str)?;
    edits.push(type_edit);

    // ── Step 4: Find the function signature line ────────────────────
    let func_line = find_func_keyword_line(&lines, paren_line)?;

    // ── Step 5: Remove @return from docblock when target is void ────
    if target_type.is_void()
        && let Some(return_tag_edit) = find_and_remove_return_tag(&lines, func_line)
    {
        edits.push(return_tag_edit);
    }

    Some(edits)
}

/// The current return type declaration as read from the source text.
///
/// Combines the native type hint (`: array`) with the `@return` tag
/// type (if any) into a single effective type string that can be
/// compared against our inference result.
pub(super) struct CurrentReturnType {
    /// The native type hint after `:`, e.g. `array`, `int`.
    /// `None` when the function has no return type declaration.
    native: Option<PhpType>,
    /// The `@return` tag type, e.g. `array<int, string>`.
    /// `None` when there is no docblock or no `@return` tag.
    docblock: Option<PhpType>,
}

/// Read the current native return type and `@return` tag type from the
/// source text around `diag_line` (a return statement inside the body).
pub(super) fn read_current_return_type(content: &str, diag_line: usize) -> CurrentReturnType {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return CurrentReturnType {
            native: None,
            docblock: None,
        };
    }

    let brace_line = match find_function_open_brace_line(&lines, diag_line) {
        Some(l) => l,
        None => {
            return CurrentReturnType {
                native: None,
                docblock: None,
            };
        }
    };
    let (paren_line, paren_col) = match find_close_paren_before_brace(&lines, brace_line) {
        Some(p) => p,
        None => {
            return CurrentReturnType {
                native: None,
                docblock: None,
            };
        }
    };

    // ── Native type hint ────────────────────────────────────────────
    let between = gather_between_paren_and_brace(&lines, paren_line, paren_col, brace_line);
    let native = between.find(':').map(|colon_pos| {
        let after_colon = &between[colon_pos + 1..];
        let type_start = after_colon.find(|c: char| !c.is_whitespace()).unwrap_or(0);
        let type_text = &after_colon[type_start..];
        let type_len = type_text
            .find(|c: char| c.is_whitespace() || c == '{')
            .unwrap_or(type_text.len());
        PhpType::parse(&type_text[..type_len])
    });

    // ── @return tag type ────────────────────────────────────────────
    let func_line = find_func_keyword_line(&lines, paren_line);
    let docblock = func_line.and_then(|fl| {
        let info = find_function_docblock(&lines, fl);
        let tag_line = info.return_tag_line?;
        let line_text = lines[tag_line];
        let at_pos = line_text.find("@return")?;
        let after = &line_text[at_pos + "@return".len()..];
        let trimmed = after.trim_start();
        if trimmed.is_empty() {
            return None;
        }
        let (type_token, _remainder) = split_type_token(trimmed);
        Some(PhpType::parse(type_token))
    });

    CurrentReturnType { native, docblock }
}

/// Decide whether to use our own inference or fall back to the
/// PHPStan tip.
///
/// Returns `true` when our inference disagrees with the current
/// declaration (native hint + `@return` tag) — meaning we can see
/// the mismatch ourselves and our type is likely more specific
/// (e.g. `list<int>` vs PHPStan's `array<int, int>`).
///
/// Returns `false` when inference failed or agrees with the
/// declaration — we can't see the bug, so the caller should trust
/// the PHPStan tip.
pub(super) fn should_use_own_inference(
    our: &Option<InferredReturnType>,
    current: &CurrentReturnType,
) -> bool {
    let Some(inferred) = our else {
        return false;
    };

    // The effective type our inference would write (prefers the rich
    // docblock type, falls back to the native hint).
    let our_effective = inferred.effective.as_ref().unwrap_or(&inferred.native);

    // The effective type currently declared (prefers the @return tag,
    // falls back to the native hint).
    let current_effective = current.docblock.as_ref().or(current.native.as_ref());
    let Some(current_effective) = current_effective else {
        return true;
    };
    !our_effective.equivalent(current_effective)
}

/// Build edits using pre-split native and effective types from our
/// own inference (where native is already a valid PHP type hint).
///
/// This is the counterpart of [`build_update_return_type_edits`] for
/// when we trust our own inference rather than the PHPStan tip.  The
/// difference is that the caller provides the native/effective split
/// directly (e.g. native=`array`, effective=`list<int>`) instead of
/// a single type string that gets split via `PhpType::to_native_hint`.
pub(super) fn build_update_return_type_edits_split(
    content: &str,
    diag_line: usize,
    native_type: &PhpType,
    effective_type: Option<&PhpType>,
) -> Option<Vec<TextEdit>> {
    // The effective type is the full type for the @return tag.
    // If there is no effective type, the native type is used for both
    // and no docblock is needed.
    let native_str = native_type.to_string();
    let full_type = effective_type
        .map(|e| e.to_string())
        .unwrap_or_else(|| native_str.clone());
    let has_docblock_type = effective_type.is_some();

    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let mut edits = Vec::new();

    // ── Step 1: Update native type hint ─────────────────────────────
    let brace_line = find_function_open_brace_line(&lines, diag_line)?;
    let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line)?;

    if let Some(type_edit) =
        find_return_type_edit(&lines, paren_line, paren_col, brace_line, &native_str)
    {
        edits.push(type_edit);
    }

    // ── Step 2: Update @return docblock tag ──────────────────────────
    let func_line = find_func_keyword_line(&lines, paren_line)?;
    let docblock_info = find_function_docblock(&lines, func_line);

    if docblock_info.has_docblock && docblock_info.has_return_tag {
        // Replace the existing @return tag's type.
        if let Some(tag_line) = docblock_info.return_tag_line {
            let line_text = lines[tag_line];
            if let Some(at_pos) = line_text.find("@return") {
                let after_return = &line_text[at_pos + "@return".len()..];
                let type_start = after_return
                    .find(|c: char| !c.is_whitespace())
                    .unwrap_or(after_return.len());
                let type_text = &after_return[type_start..];
                let (_, remainder) = split_type_token(type_text);
                let description = remainder.to_string();

                let new_line = format!(
                    "{}@return {}{}",
                    &line_text[..at_pos],
                    full_type,
                    description
                );

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(tag_line as u32, 0),
                        end: Position::new(tag_line as u32, line_text.len() as u32),
                    },
                    new_text: new_line,
                });
            }
        }
    } else if has_docblock_type {
        // Only create/insert a @return tag when the effective type
        // differs from the native type.
        let indent = &docblock_info.indent;

        if docblock_info.has_docblock {
            let doc_end = docblock_info.doc_end_line;
            let close_line = lines[doc_end];

            if docblock_info.doc_start_line == doc_end {
                let trimmed = close_line.trim();
                let inner = trimmed
                    .strip_prefix("/**")
                    .and_then(|s| s.strip_suffix("*/"))
                    .map(|s| s.trim())
                    .unwrap_or("");

                let mut new_doc = format!("{}/**\n", indent);
                if !inner.is_empty() {
                    new_doc.push_str(&format!("{} * {}\n", indent, inner));
                    new_doc.push_str(&format!("{} *\n", indent));
                }
                new_doc.push_str(&format!("{} * @return {}\n", indent, full_type));
                new_doc.push_str(&format!("{} */", indent));

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(doc_end as u32, 0),
                        end: Position::new(doc_end as u32, close_line.len() as u32),
                    },
                    new_text: new_doc,
                });
            } else {
                let prev_line = if doc_end > docblock_info.doc_start_line {
                    lines[doc_end - 1].trim()
                } else {
                    ""
                };
                let prev_trimmed = prev_line.trim_start_matches('*').trim();
                let needs_separator = !prev_trimmed.is_empty()
                    && !prev_trimmed.starts_with("@return")
                    && !prev_trimmed.starts_with("@throws")
                    && prev_trimmed.starts_with('@');

                let mut insert_text = String::new();
                if needs_separator {
                    insert_text.push_str(&format!("{} *\n", indent));
                }
                insert_text.push_str(&format!("{} * @return {}\n", indent, full_type));

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(doc_end as u32, 0),
                        end: Position::new(doc_end as u32, 0),
                    },
                    new_text: insert_text,
                });
            }
        } else {
            let new_doc = format!(
                "{}/**\n{} * @return {}\n{} */\n",
                indent, indent, full_type, indent
            );
            edits.push(TextEdit {
                range: Range {
                    start: Position::new(func_line as u32, 0),
                    end: Position::new(func_line as u32, 0),
                },
                new_text: new_doc,
            });
        }
    }

    if edits.is_empty() {
        return None;
    }

    Some(edits)
}

/// Build a list of `TextEdit`s that update both the native return type
/// hint and the `@return` docblock tag for a `return.type` diagnostic.
///
/// The native type is changed to the base type (generics stripped) so
/// that it remains valid PHP.  The `@return` tag gets the full type
/// including any generic parameters.
///
/// When the actual type has no generics and there is no existing
/// `@return` tag, only the native type is changed.
///
/// Returns `None` if the enclosing function cannot be found.
pub(super) fn build_update_return_type_edits(
    content: &str,
    diag_line: usize,
    actual_type: &PhpType,
) -> Option<Vec<TextEdit>> {
    let lines: Vec<&str> = content.lines().collect();
    if diag_line >= lines.len() {
        return None;
    }

    let mut edits = Vec::new();

    let actual_str = actual_type.to_string();
    let base_type = actual_type
        .to_native_hint()
        .unwrap_or_else(|| actual_str.clone());
    let has_generics = base_type != actual_str;

    // ── Step 1: Update native type hint ─────────────────────────────
    let brace_line = find_function_open_brace_line(&lines, diag_line)?;
    let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line)?;

    // Only change the native type if the base type differs from the
    // current native type.
    if let Some(type_edit) =
        find_return_type_edit(&lines, paren_line, paren_col, brace_line, &base_type)
    {
        edits.push(type_edit);
    }

    // ── Step 2: Update @return docblock tag ──────────────────────────
    let func_line = find_func_keyword_line(&lines, paren_line)?;
    let docblock_info = find_function_docblock(&lines, func_line);

    if docblock_info.has_docblock && docblock_info.has_return_tag {
        // Replace the existing @return tag's type.
        if let Some(tag_line) = docblock_info.return_tag_line {
            let line_text = lines[tag_line];
            if let Some(at_pos) = line_text.find("@return") {
                let after_return = &line_text[at_pos + "@return".len()..];
                let type_start = after_return
                    .find(|c: char| !c.is_whitespace())
                    .unwrap_or(after_return.len());
                let type_text = &after_return[type_start..];
                let (_, remainder) = split_type_token(type_text);
                let description = remainder.to_string();

                let new_line = format!(
                    "{}@return {}{}",
                    &line_text[..at_pos],
                    actual_str,
                    description
                );

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(tag_line as u32, 0),
                        end: Position::new(tag_line as u32, line_text.len() as u32),
                    },
                    new_text: new_line,
                });
            }
        }
    } else if has_generics {
        // Only create/insert a @return tag when the actual type has
        // generics — otherwise the native type hint is sufficient.
        let indent = &docblock_info.indent;

        if docblock_info.has_docblock {
            // Docblock exists but has no @return tag — insert one.
            let doc_end = docblock_info.doc_end_line;
            let close_line = lines[doc_end];

            if docblock_info.doc_start_line == doc_end {
                // Single-line docblock: convert to multi-line.
                let trimmed = close_line.trim();
                let inner = trimmed
                    .strip_prefix("/**")
                    .and_then(|s| s.strip_suffix("*/"))
                    .map(|s| s.trim())
                    .unwrap_or("");

                let mut new_doc = format!("{}/**\n", indent);
                if !inner.is_empty() {
                    new_doc.push_str(&format!("{} * {}\n", indent, inner));
                    new_doc.push_str(&format!("{} *\n", indent));
                }
                new_doc.push_str(&format!("{} * @return {}\n", indent, actual_str));
                new_doc.push_str(&format!("{} */", indent));

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(doc_end as u32, 0),
                        end: Position::new(doc_end as u32, close_line.len() as u32),
                    },
                    new_text: new_doc,
                });
            } else {
                // Multi-line docblock: insert @return before `*/`.
                let prev_line = if doc_end > docblock_info.doc_start_line {
                    lines[doc_end - 1].trim()
                } else {
                    ""
                };
                let prev_trimmed = prev_line.trim_start_matches('*').trim();
                let needs_separator = !prev_trimmed.is_empty()
                    && !prev_trimmed.starts_with("@return")
                    && !prev_trimmed.starts_with("@throws")
                    && prev_trimmed.starts_with('@');

                let mut insert_text = String::new();
                if needs_separator {
                    insert_text.push_str(&format!("{} *\n", indent));
                }
                insert_text.push_str(&format!("{} * @return {}\n", indent, actual_str));

                edits.push(TextEdit {
                    range: Range {
                        start: Position::new(doc_end as u32, 0),
                        end: Position::new(doc_end as u32, 0),
                    },
                    new_text: insert_text,
                });
            }
        } else {
            // No existing docblock — create one with `@return`.
            let new_doc = format!(
                "{}/**\n{} * @return {}\n{} */\n",
                indent, indent, actual_str, indent
            );
            edits.push(TextEdit {
                range: Range {
                    start: Position::new(func_line as u32, 0),
                    end: Position::new(func_line as u32, 0),
                },
                new_text: new_doc,
            });
        }
    }

    if edits.is_empty() {
        return None;
    }

    Some(edits)
}

/// Check whether there is already a return type hint between `)` and
/// `{`.  Returns `true` if a `:` is found in that region.
pub(super) fn has_return_type_between(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    brace_line: usize,
) -> bool {
    for (line_idx, line) in lines
        .iter()
        .enumerate()
        .take(brace_line + 1)
        .skip(paren_line)
    {
        let start_col = if line_idx == paren_line {
            paren_col + 1
        } else {
            0
        };
        let end_col = if line_idx == brace_line {
            line.find('{').unwrap_or(line.len())
        } else {
            line.len()
        };
        if start_col <= end_col && line[start_col..end_col].contains(':') {
            return true;
        }
    }
    false
}

/// Check whether the byte position `after_semi` (just past a `;`) is
/// the last statement in its enclosing function body.
///
/// Scans forward from `after_semi` through whitespace, comments, and
/// closing braces.  If only `}` characters (closing nested blocks like
/// `if`/`foreach`/`try`) and whitespace/comments appear between the
/// `;` and the function's own closing `}`, then the statement is the
/// last one in the function and a trailing `return;` would be
/// redundant.
///
/// Returns `false` when any other statement or token appears, meaning
/// the `return;` is needed to exit early.
fn is_last_statement_in_function(content: &str, after_semi: usize) -> bool {
    let bytes = content.as_bytes();
    let mut i = after_semi;

    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                // Line comment — skip to end of line.
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // Block comment — skip to `*/`.
                i += 2;
                while i + 1 < bytes.len() {
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            }
            b'}' => {
                // Closing brace — could be an `if`/`foreach`/etc.
                // block or the function itself.  Keep scanning to
                // see if anything other than more `}` and whitespace
                // follows.
                i += 1;
            }
            _ => return false,
        }
    }
    // Reached end of content with only `}` and whitespace — the
    // statement was the last one.
    true
}

// ── Source navigation helpers ───────────────────────────────────────────────

/// Walk backward from `start_line` to find the line containing the
/// opening `{` of the enclosing function body.
///
/// The opening brace is the first `{` found scanning backward that is
/// not inside a string or comment.  We use a simple heuristic: look
/// for a line whose trimmed content ends with `{` or contains `{`
/// after a `)`.
///
/// **Limitation:** Braces inside string literals and comments are
/// counted, which can produce wrong results in rare cases.  A fully
/// correct backward scan would require re-parsing from the top of the
/// file.  This simple heuristic works for typical PHP code.
pub(super) fn find_function_open_brace_line(lines: &[&str], start_line: usize) -> Option<usize> {
    // Track brace depth: we start inside the function body (depth 1)
    // and look backward for the opening `{`.
    let mut depth: i32 = 0;
    for i in (0..start_line).rev() {
        let line = lines[i];
        // Count braces on this line (simple heuristic, ignoring strings).
        for ch in line.chars() {
            match ch {
                '{' => depth -= 1,
                '}' => depth += 1,
                _ => {}
            }
        }
        if depth < 0 {
            return Some(i);
        }
    }
    None
}

/// Search forward from a declaration line to find the opening `{` of
/// the function body.
///
/// Checks the declaration line itself and up to 5 lines after it.
/// Returns the line number containing `{`, or `None`.
pub(super) fn find_open_brace_from_declaration(lines: &[&str], decl_line: usize) -> Option<usize> {
    let end = (decl_line + 6).min(lines.len());
    (decl_line..end).find(|&i| lines[i].contains('{'))
}

/// Find the closing `)` of the parameter list before the opening `{`.
///
/// Scans backward from `brace_line` looking for `)`.
pub(super) fn find_close_paren_before_brace(
    lines: &[&str],
    brace_line: usize,
) -> Option<(usize, usize)> {
    // First check the brace line itself (before the `{`).
    let brace_text = lines[brace_line];
    if let Some(brace_pos) = brace_text.rfind('{') {
        let before_brace = &brace_text[..brace_pos];
        if let Some(paren_pos) = before_brace.rfind(')') {
            return Some((brace_line, paren_pos));
        }
    }

    // Walk backward to find `)`.
    for i in (0..brace_line).rev() {
        if let Some(paren_pos) = lines[i].rfind(')') {
            return Some((i, paren_pos));
        }
    }

    None
}

/// Gather the source text between the closing `)` at `(paren_line, paren_col)`
/// and the opening `{` on `brace_line`.
///
/// The result spans from column `paren_col + 1` on `paren_line` to just
/// before the `{` on `brace_line`, with newlines between lines.
pub(super) fn gather_between_paren_and_brace(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    brace_line: usize,
) -> String {
    let mut between = String::new();

    for (line_idx, line) in lines
        .iter()
        .enumerate()
        .take(brace_line + 1)
        .skip(paren_line)
    {
        let start_col = if line_idx == paren_line {
            paren_col + 1
        } else {
            0
        };
        let end_col = if line_idx == brace_line {
            line.find('{').unwrap_or(line.len())
        } else {
            line.len()
        };
        if start_col <= end_col {
            between.push_str(&line[start_col..end_col]);
        }
        if line_idx < brace_line {
            between.push('\n');
        }
    }

    between
}

/// Find the return type hint between the closing `)` and opening `{`,
/// and build a `TextEdit` that replaces it with `: {target_type}`.
///
/// Looks for the pattern `: TypeName` (with optional whitespace and
/// nullable `?` prefix).  Returns `None` if the current type already
/// matches `target_type`.
fn find_return_type_edit(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    brace_line: usize,
    target_type: &str,
) -> Option<TextEdit> {
    // Gather the text between `)` and `{` across potentially multiple
    // lines.
    let between = gather_between_paren_and_brace(lines, paren_line, paren_col, brace_line);

    // Find `: Type` in the between text.
    let colon_pos = between.find(':')?;
    let after_colon = &between[colon_pos + 1..];
    let type_start_offset = after_colon.find(|c: char| !c.is_whitespace()).unwrap_or(0);
    let type_text_start = colon_pos + 1 + type_start_offset;
    let type_text = &between[type_text_start..];

    // The type name ends at the first whitespace, `{`, or end of
    // the between text.
    let type_len = type_text
        .find(|c: char| c.is_whitespace() || c == '{')
        .unwrap_or(type_text.len());

    if type_len == 0 {
        return None;
    }

    let type_name = &type_text[..type_len];
    if type_name == target_type {
        return None;
    }

    // Convert the offset within `between` to a line/col position.
    // The colon_pos tells us where `:` is; the type starts at
    // `type_text_start` and ends at `type_text_start + type_len`.

    // Map `colon_pos` back to an absolute line/col.
    let colon_abs = map_between_offset_to_position(lines, paren_line, paren_col, colon_pos)?;
    let type_end_abs =
        map_between_offset_to_position(lines, paren_line, paren_col, type_text_start + type_len)?;

    Some(TextEdit {
        range: Range {
            start: Position::new(colon_abs.0 as u32, colon_abs.1 as u32),
            end: Position::new(type_end_abs.0 as u32, type_end_abs.1 as u32),
        },
        new_text: format!(": {}", target_type),
    })
}

/// Map an offset within the "between" text back to an absolute
/// (line, col) position in the original source.
fn map_between_offset_to_position(
    lines: &[&str],
    paren_line: usize,
    paren_col: usize,
    offset: usize,
) -> Option<(usize, usize)> {
    // Re-walk the between region character by character.
    let mut remaining = offset;
    for (line_idx, line) in lines.iter().enumerate().skip(paren_line) {
        let start_col = if line_idx == paren_line {
            paren_col + 1
        } else {
            0
        };
        let end_col = line.len();
        let span = end_col - start_col;

        if remaining <= span {
            return Some((line_idx, start_col + remaining));
        }
        remaining -= span;

        // Account for the newline character.
        if remaining == 0 {
            // Exactly at the newline boundary — start of next line.
            return Some((line_idx + 1, 0));
        }
        remaining -= 1; // for the '\n'
    }
    None
}

/// Look for a docblock above the function signature and remove any
/// `@return` tag line from it.
fn find_and_remove_return_tag(lines: &[&str], func_line: usize) -> Option<TextEdit> {
    if func_line == 0 {
        return None;
    }

    // Walk backward from the line before the function to find the
    // docblock.  Skip attribute lines like `#[Override]`.
    let mut doc_end_line = None;
    for i in (0..func_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.ends_with("*/") {
            doc_end_line = Some(i);
            break;
        }
        // Skip attributes and blank lines between function and docblock.
        if trimmed.starts_with("#[") || trimmed.is_empty() {
            continue;
        }
        // Hit non-docblock, non-attribute content — no docblock.
        break;
    }

    let doc_end_line = doc_end_line?;

    // Find the start of the docblock.
    let mut doc_start_line = doc_end_line;
    for i in (0..=doc_end_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("/**") {
            doc_start_line = i;
            break;
        }
        if trimmed.starts_with('*') || trimmed.starts_with("/*") {
            continue;
        }
        break;
    }

    // Look for a `@return` line within the docblock.
    let return_line =
        (doc_start_line..=doc_end_line).find(|&i| lines[i].trim().contains("@return"))?;

    Some(TextEdit {
        range: Range {
            start: Position::new(return_line as u32, 0),
            end: Position::new((return_line + 1) as u32, 0),
        },
        new_text: String::new(),
    })
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "edits_tests.rs"]
mod tests;
