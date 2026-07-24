//! Detection of the `/**` completion trigger and the freshly
//! auto-closed empty docblock (`onTypeFormatting` path), plus
//! extraction of the raw declaration text that follows either.

use tower_lsp::lsp_types::{Position, Range};

use crate::completion::source::comment_position::position_to_byte_offset;
use crate::text_position::{byte_offset_to_utf16_col, utf16_col_to_byte_offset};

/// Check if the cursor is immediately after `/**` with only whitespace
/// before it on the line, and that there is no existing docblock (i.e.
/// the `/**` is not already closed with `*/`).
///
/// Returns the range covering the `/**` text (to be replaced by the
/// snippet) and the leading indentation string.
pub(super) fn detect_docblock_trigger(
    content: &str,
    position: Position,
) -> Option<(Range, String)> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;
    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];

    // Convert the UTF-16 column offset to a byte offset within the line.
    // LSP positions use UTF-16 code units, which diverge from byte offsets
    // when the line contains multibyte characters (e.g. "ń" is 2 bytes in
    // UTF-8 but 1 UTF-16 code unit).
    let col = utf16_col_to_byte_offset(line, position.character);

    // The cursor column must be at least 3 (for `/**`).
    if col < 3 {
        return None;
    }

    // Get the text up to the cursor on this line.
    let before_cursor = if col <= line.len() {
        &line[..col]
    } else {
        line
    };

    // Must end with `/**`.
    if !before_cursor.ends_with("/**") {
        return None;
    }

    // Everything before `/**` must be whitespace.
    let prefix = &before_cursor[..before_cursor.len() - 3];
    if !prefix.chars().all(|c| c == ' ' || c == '\t') {
        return None;
    }

    // Check what follows the `/**` on this line.
    let after_trigger = if col <= line.len() { &line[col..] } else { "" };

    // Editors like VS Code auto-close `/**` into `/** */` on the same
    // line.  We allow this when the only thing after `/**` is optional
    // whitespace and `*/` (i.e. an empty auto-closed block).
    let after_trimmed = after_trigger.trim();
    let auto_closed = after_trimmed == "*/" || after_trimmed.is_empty();

    // If there is a `*/` with real content between `/**` and `*/`
    // (e.g. `/** @var int */`), this is an existing single-line
    // docblock — don't trigger.
    if !auto_closed && after_trigger.contains("*/") {
        return None;
    }

    // Also check that the next few lines don't form an existing
    // docblock (i.e. don't generate a new block inside an existing one).
    // A simple heuristic: if the next non-empty line starts with `*` or
    // contains `*/`, there's already a docblock.
    if !after_trigger.contains("*/") {
        for next_line in lines.iter().skip(line_idx + 1) {
            let trimmed = next_line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('*') || trimmed.starts_with("*/") {
                return None;
            }
            // First non-empty, non-docblock-continuation line found — OK.
            break;
        }
    }

    let indent = prefix.to_string();

    // Convert byte offsets back to UTF-16 columns for the LSP Range.
    let start_col = byte_offset_to_utf16_col(line, col - 3);
    let end_col = if after_trigger.contains("*/") {
        byte_offset_to_utf16_col(line, line.len())
    } else {
        byte_offset_to_utf16_col(line, col)
    };

    let range = Range {
        start: Position {
            line: position.line,
            character: start_col,
        },
        end: Position {
            line: position.line,
            character: end_col,
        },
    };

    Some((range, indent))
}

/// Detect whether the cursor is inside a freshly auto-generated empty
/// docblock.  Returns `(range_of_entire_block, indent, text_after_block)`.
///
/// Recognised patterns (after the editor auto-closes `/**`):
///
/// ```text
/// /** */          ← single-line empty
/// /**             ← multi-line empty
///  *              (cursor is here after Enter)
///  */
/// /**             ← multi-line with blank star line
///  * |
///  */
/// ```
pub(super) fn detect_empty_docblock(
    content: &str,
    position: Position,
) -> Option<(Range, String, String)> {
    let lines: Vec<&str> = content.lines().collect();
    let cur_line = position.line as usize;

    // ── Find the `/**` opening ──────────────────────────────────────
    // Walk backwards from the cursor line to find a line containing `/**`.
    let mut open_line = None;
    for i in (0..=cur_line).rev() {
        if i >= lines.len() {
            continue;
        }
        let trimmed = lines[i].trim();
        if trimmed.contains("/**") {
            open_line = Some(i);
            break;
        }
        // Stop if we hit a non-docblock, non-empty line (e.g. code).
        if !trimmed.is_empty() && !trimmed.starts_with('*') && !trimmed.starts_with("*/") {
            return None;
        }
    }
    let open_idx = open_line?;

    // ── Check this is a fresh empty docblock ────────────────────────
    // The opening line must be just `/**` (with optional whitespace and
    // optional `*/` on the same line).
    let open_text = lines[open_idx];
    let trimmed_open = open_text.trim();
    if !trimmed_open.starts_with("/**") {
        return None;
    }

    // Extract indentation from the opening line.
    let indent: String = open_text
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();

    // ── Find the `*/` closing ───────────────────────────────────────
    let mut close_line = None;

    // Single-line case: `/** */` on one line.
    if trimmed_open.ends_with("*/") && trimmed_open.len() <= "/** */".len() + 2 {
        close_line = Some(open_idx);
    } else {
        // Multi-line: scan forward from the opening line.
        for (i, line) in lines.iter().enumerate().skip(open_idx + 1) {
            let trimmed = line.trim();
            if trimmed == "*/" || trimmed.ends_with("*/") {
                close_line = Some(i);
                break;
            }
            // A line with real content (not just `*` or whitespace)
            // means this is an existing docblock with documentation.
            if let Some(after_star) = trimmed
                .strip_prefix("* ")
                .or_else(|| trimmed.strip_prefix("*\t"))
            {
                let after_star = after_star.trim();
                if !after_star.is_empty() {
                    // There's actual text — this is not a fresh block.
                    return None;
                }
            }
        }
    }
    let close_idx = close_line?;

    // Verify the docblock is "empty" — the only content between `/**`
    // and `*/` should be blank `*` lines.
    for line in lines.iter().take(close_idx).skip(open_idx + 1) {
        let trimmed = line.trim();
        // Allow: empty, bare `*`, `* ` (trailing space), or cursor line.
        if !trimmed.is_empty()
            && trimmed != "*"
            && !trimmed.chars().all(|c| c == '*' || c == ' ' || c == '\t')
        {
            return None;
        }
    }

    // ── Build the range covering the entire block ───────────────────
    let start = Position {
        line: open_idx as u32,
        character: 0,
    };
    // End covers through the closing `*/` line (including its newline
    // if there is a next line).
    let close_line_len = lines.get(close_idx).map(|l| l.len()).unwrap_or(0);
    let end = if close_idx + 1 < lines.len() {
        // Include the trailing newline.
        Position {
            line: (close_idx + 1) as u32,
            character: 0,
        }
    } else {
        Position {
            line: close_idx as u32,
            character: close_line_len as u32,
        }
    };
    let block_range = Range { start, end };

    // ── Collect text after the block ────────────────────────────────
    let after_start = if close_idx + 1 < lines.len() {
        close_idx + 1
    } else {
        lines.len()
    };
    let after_block: String = lines[after_start..].to_vec().join("\n");

    Some((block_range, indent, after_block))
}

/// Extract the indentation of the first declaration line in `text`,
/// skipping empty lines and attribute blocks.
pub(super) fn declaration_indent(text: &str) -> String {
    let mut attr_depth = 0i32;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if attr_depth > 0 || trimmed.starts_with("#[") {
            for ch in trimmed.chars() {
                match ch {
                    '[' => attr_depth += 1,
                    ']' => attr_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }
        // First non-empty, non-attribute line — return its indent.
        return line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
    }
    String::new()
}

/// Get the text after the `/**` trigger position, skipping the rest of
/// the trigger line.
pub(super) fn get_text_after_trigger(content: &str, position: Position) -> String {
    let byte_offset = position_to_byte_offset(content, position);
    let after = &content[byte_offset.min(content.len())..];

    // Skip to the next line (the trigger line has `/**` and possibly
    // nothing else useful).
    if let Some(nl) = after.find('\n') {
        after[nl + 1..].to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
#[path = "trigger_tests.rs"]
mod tests;
