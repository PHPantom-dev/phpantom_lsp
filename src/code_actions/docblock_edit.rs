//! Shared docblock-location helpers for code actions.
//!
//! Several PHPStan quickfixes need to locate the `/** … */` block that sits
//! directly above a function, method, or property signature so they can
//! rewrite or remove tags inside it.  This module owns the single
//! implementation they share.

use crate::text_position::line_start_byte_offset;

/// Information about a docblock found above a given line.
pub(crate) struct DocblockAbove {
    /// Byte offset of the start of the docblock (first char of the `/**`
    /// line, including leading whitespace).
    pub(crate) start: usize,
    /// Byte offset just past the end of the docblock (past the `*/` line,
    /// including its trailing newline).
    pub(crate) end: usize,
    /// The raw docblock text including indentation.
    pub(crate) text: String,
}

/// Find the docblock immediately above the given line.
///
/// The diagnostic line is the function/method/property signature.  The
/// docblock (if any) sits directly above it, possibly separated by blank
/// lines or attribute (`#[…]`) lines.
pub(crate) fn find_docblock_above_line(content: &str, line: usize) -> Option<DocblockAbove> {
    let lines: Vec<&str> = content.lines().collect();
    if line == 0 || line > lines.len() {
        return None;
    }

    // Walk backward from the line before the diagnostic to find `*/`.
    let mut doc_end_line = None;
    for i in (0..line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.ends_with("*/") {
            doc_end_line = Some(i);
            break;
        }
        // Attributes (#[...]) can appear between docblock and declaration.
        if trimmed.starts_with("#[") {
            continue;
        }
        // Anything else means no docblock.
        break;
    }

    let end_line = doc_end_line?;

    // Walk backward from end_line to find `/**`.
    let mut doc_start_line = None;
    for i in (0..=end_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.contains("/**") {
            doc_start_line = Some(i);
            break;
        }
        // Should be a `*`-prefixed line or end-of-docblock.
        if !trimmed.starts_with('*') && !trimmed.ends_with("*/") {
            break;
        }
    }

    let start_line = doc_start_line?;

    // Convert line numbers to byte offsets.  Counting the bytes of each
    // line and adding one for its terminator would be a line short per
    // `\r\n`, so the offsets come from the line index instead.
    let start_byte = line_start_byte_offset(content, start_line);
    // The docblock ends where the line after its last one begins, so the
    // trailing newline is part of the span and deleting it takes the
    // whole line with it.
    let end_byte = line_start_byte_offset(content, end_line + 1).min(content.len());

    Some(DocblockAbove {
        start: start_byte,
        end: end_byte,
        text: content.get(start_byte..end_byte).unwrap_or("").to_string(),
    })
}

/// Byte offset of the start of the `/** … */` block sitting above the
/// declaration whose first line starts at `decl_line_start`, if there is one.
///
/// Works entirely in byte offsets, so the result is safe to feed straight
/// into a deletion edit regardless of line endings.  Blank lines between the
/// docblock and the declaration are tolerated and included in the returned
/// span, so deleting `[start, decl_line_end)` removes both.
pub(crate) fn docblock_start_above_offset(content: &str, decl_line_start: usize) -> Option<usize> {
    // Walk backwards over blank lines to the line that closes the docblock.
    let mut line_start = decl_line_start;
    let closing_line = loop {
        let prev = prev_line_start(content, line_start)?;
        let trimmed = content.get(prev..line_start)?.trim();
        if trimmed.ends_with("*/") {
            break prev;
        }
        if !trimmed.is_empty() {
            return None;
        }
        line_start = prev;
    };

    // Walk back over the docblock's own lines to its `/**` opener.
    let mut line_start = closing_line;
    loop {
        let line_end = content[line_start..]
            .find('\n')
            .map_or(content.len(), |i| line_start + i);
        let trimmed = content.get(line_start..line_end)?.trim();
        if trimmed.starts_with("/**") {
            return Some(line_start);
        }
        if !trimmed.starts_with('*') {
            return None;
        }
        line_start = prev_line_start(content, line_start)?;
    }
}

/// Start offset of the line preceding the one starting at `line_start`,
/// or `None` when `line_start` is already the start of the file.
fn prev_line_start(content: &str, line_start: usize) -> Option<usize> {
    if line_start == 0 {
        return None;
    }
    let bytes = content.as_bytes();
    // `line_start - 1` is the newline that terminates the previous line.
    let mut pos = line_start - 1;
    while pos > 0 && bytes[pos - 1] != b'\n' {
        pos -= 1;
    }
    Some(pos)
}
