//! Byte-offset / LSP-position conversion helpers.
//!
//! This is the most reused cluster of utilities in the codebase: every
//! feature that reports a diagnostic, builds a `WorkspaceEdit`, or
//! answers a hover/completion/definition request needs to convert
//! between byte offsets (used internally and by `mago-syntax` spans)
//! and LSP `Position`/`Range` values (UTF-16 code units per the LSP
//! spec).

use tower_lsp::lsp_types::{Position, Range, TextEdit};

/// Check whether two LSP ranges overlap (share at least one character
/// position).
///
/// Two ranges do **not** overlap when one ends exactly where the other
/// starts (i.e. touching ranges are non-overlapping).  This matches
/// the LSP convention where a range's `end` position is exclusive.
pub(crate) fn ranges_overlap(a: &Range, b: &Range) -> bool {
    !(a.end.line < b.start.line
        || (a.end.line == b.start.line && a.end.character <= b.start.character)
        || b.end.line < a.start.line
        || (b.end.line == a.start.line && b.end.character <= a.start.character))
}

/// Convert a byte offset in `content` to an LSP `Position` (line, character).
///
/// This is the inverse of [`position_to_byte_offset`].  Characters are
/// counted as UTF-16 code units per the LSP specification.
/// If `offset` is past the end of `content`, the position at the end of
/// the file is returned.  An offset inside a multi-byte character answers
/// the position after that character, rather than never matching.
pub(crate) fn offset_to_position(content: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in content.char_indices() {
        if i >= offset {
            return Position {
                line,
                character: col,
            };
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    // offset == content.len() (end of file)
    Position {
        line,
        character: col,
    }
}

/// Precomputed line-start byte offsets for fast repeated offset→[`Position`]
/// lookups within a single piece of content.
///
/// [`offset_to_position`] rescans `content` from the start on every call, so
/// it is O(offset). Converting many offsets in a loop (e.g. one per semantic
/// token, of which a large file has thousands) is therefore O(n²) in the file
/// size. `LineIndex` builds the line table once and answers each query with a
/// binary search for the line plus a short within-line UTF-16 scan, turning the
/// loop into O(n log n).
pub(crate) struct LineIndex<'a> {
    content: &'a str,
    /// Byte offset of the first character of each line. Always starts with `0`.
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    /// Build the line table for `content` in a single pass.
    pub(crate) fn new(content: &'a str) -> Self {
        Self {
            content,
            line_starts: line_starts(content),
        }
    }

    /// The content this index was built from, for callers that also need to
    /// scan the raw source (e.g. to locate a declaration's end).
    pub(crate) fn content(&self) -> &'a str {
        self.content
    }

    /// Greatest line index whose start is <= `offset`, found by binary
    /// search over the line table. Unlike [`Self::position`], this does not
    /// clamp `offset` to the content length or compute a UTF-16 column.
    pub(crate) fn line_of(&self, offset: usize) -> usize {
        // `line_starts[0] == 0`, so the `Err(0)` case (offset before the
        // first line start) cannot happen.
        match self.line_starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx - 1,
        }
    }

    /// Byte offset of the first character of the 0-based `line`, or the
    /// content length when `line` is past the last one.
    pub(crate) fn line_start(&self, line: usize) -> usize {
        self.line_starts
            .get(line)
            .copied()
            .unwrap_or(self.content.len())
    }

    /// Convert a byte `offset` to an LSP [`Position`] (0-based line, UTF-16
    /// column). Offsets past the end of the content clamp to the content
    /// length, matching [`offset_to_position`]. An offset inside a
    /// multi-byte character is walked forward to the next character
    /// boundary, also matching [`offset_to_position`], rather than slicing
    /// mid-character and panicking.
    pub(crate) fn position(&self, offset: usize) -> Position {
        position_in(self.content, &self.line_starts, offset)
    }
}

/// The line table a [`LineIndex`] is built on: the byte offset of the first
/// character of each line, starting with `0`.
///
/// For a caller that has to store the table next to content it owns, where
/// a [`LineIndex`] borrowing that content cannot live.
pub(crate) fn line_starts(content: &str) -> Vec<usize> {
    let mut line_starts = Vec::with_capacity(content.len() / 24 + 1);
    line_starts.push(0usize);
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    line_starts
}

/// [`LineIndex::position`] over a table built by [`line_starts`] for the
/// same `content`.
pub(crate) fn position_in(content: &str, line_starts: &[usize], offset: usize) -> Position {
    let mut offset = offset.min(content.len());
    while !content.is_char_boundary(offset) {
        offset += 1;
    }
    let line = match line_starts.binary_search(&offset) {
        Ok(idx) => idx,
        Err(idx) => idx - 1,
    };
    let line_start = line_starts[line];
    let character = content[line_start..offset]
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum();
    Position {
        line: line as u32,
        character,
    }
}

/// Return the byte offset of the start of the line at `line_idx`
/// (0-based) within `content`.
///
/// Unlike the common `content.lines().take(n).map(|l| l.len() + 1).sum()`
/// idiom, this counts the real line terminator. `str::lines()` strips the
/// `\r` of a `\r\n` (CRLF) pair, so the sum-of-`len + 1` approach
/// undercounts every preceding line by one byte on CRLF files and drifts
/// the computed offset. Counting newline bytes directly stays correct for
/// both LF and CRLF (the `\n` is the final byte of a CRLF pair either way).
///
/// If `line_idx` exceeds the number of lines, `content.len()` is returned.
pub(crate) fn line_start_byte_offset(content: &str, line_idx: usize) -> usize {
    if line_idx == 0 {
        return 0;
    }
    let mut seen = 0usize;
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line_idx {
                return i + 1;
            }
        }
    }
    content.len()
}

/// Convert an LSP `Position` (line, character) to a byte offset in
/// `content`.
///
/// Characters are counted as UTF-16 code units per the LSP specification.
/// If the position is past the end of the file, the content length is
/// returned.
pub(crate) fn position_to_byte_offset(content: &str, position: Position) -> usize {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in content.char_indices() {
        if line == position.line && col == position.character {
            return i;
        }
        if ch == '\n' {
            if line == position.line {
                // Position is past the end of this line — clamp to newline.
                return i;
            }
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    // Position at end of content.
    content.len()
}

/// Convert an LSP Position (line, character) to a byte offset in content.
///
/// Thin wrapper around [`position_to_byte_offset`] that returns `u32`
/// (matching the offset type used by `ClassInfo::start_offset` /
/// `end_offset` and `ResolutionCtx::cursor_offset`).
pub(crate) fn position_to_offset(content: &str, position: Position) -> u32 {
    position_to_byte_offset(content, position) as u32
}

/// Convert an LSP `Position` (line/character) to a character offset into
/// a pre-built char array.
///
/// Returns `None` when the position is beyond the end of `chars`.
/// Handles UTF-16 column widths, end-of-line clamping, and trailing
/// content without a newline.
pub fn position_to_char_offset(chars: &[char], position: Position) -> Option<usize> {
    let target_line = position.line as usize;
    let target_col = position.character as usize;
    let mut line = 0usize;
    let mut col = 0usize;

    for (i, &ch) in chars.iter().enumerate() {
        if line == target_line && col == target_col {
            return Some(i);
        }
        if ch == '\n' {
            // If we're at the target line and the target column is at or
            // past the end of the line, clamp to end-of-line.
            if line == target_line {
                return Some(i);
            }
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16();
        }
    }

    // Cursor at very end of content
    if line == target_line && col == target_col {
        return Some(chars.len());
    }
    // Target column past end of last line (no trailing newline)
    if line == target_line {
        return Some(chars.len());
    }

    None
}

/// Convert a UTF-16 column offset to a byte offset within a single line.
///
/// LSP positions use UTF-16 code units for the character offset.  When a
/// line contains multi-byte characters (e.g. `ń` is 2 bytes in UTF-8 but
/// 1 UTF-16 code unit), the two offsets diverge.  This helper walks the
/// line counting UTF-16 code units and returns the corresponding byte
/// position.
///
/// Returns `line.len()` if `utf16_col` is past the end of the line.
pub(crate) fn utf16_col_to_byte_offset(line: &str, utf16_col: u32) -> usize {
    let mut col = 0u32;
    for (i, ch) in line.char_indices() {
        if col == utf16_col {
            return i;
        }
        col += ch.len_utf16() as u32;
    }
    line.len()
}

/// Convert a byte offset within a single line to a UTF-16 column offset.
///
/// This is the inverse of [`utf16_col_to_byte_offset`].  It counts
/// UTF-16 code units for all characters before `byte_offset` and returns
/// the result.
///
/// Returns the total UTF-16 length of the line if `byte_offset` is past
/// the end.
pub(crate) fn byte_offset_to_utf16_col(line: &str, byte_offset: usize) -> u32 {
    let mut col = 0u32;
    for (i, ch) in line.char_indices() {
        if i >= byte_offset {
            return col;
        }
        col += ch.len_utf16() as u32;
    }
    col
}

/// Convert a byte offset range to an LSP `Range`.
pub(crate) fn byte_range_to_lsp_range(content: &str, start: usize, end: usize) -> Range {
    let start_pos = offset_to_position(content, start);
    let end_pos = offset_to_position(content, end);
    Range {
        start: start_pos,
        end: end_pos,
    }
}

/// Apply non-overlapping `TextEdit`s to `content` and return the result.
///
/// Edits are applied bottom-to-top so an earlier edit never shifts the
/// positions of a later one; columns are UTF-16 code units, as in LSP.
/// An edit whose range is inverted is skipped rather than applied.
pub(crate) fn apply_text_edits(content: &str, edits: &[TextEdit]) -> String {
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by(|a, b| {
        b.range
            .start
            .line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });
    let mut result = content.to_string();
    for edit in sorted {
        let start = position_to_byte_offset(&result, edit.range.start);
        let end = position_to_byte_offset(&result, edit.range.end);
        if start <= end {
            result.replace_range(start..end, &edit.new_text);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_apply_bottom_up_with_utf16_columns() {
        let content = "ń = 1;\nfoo();\n";
        let edits = vec![
            TextEdit {
                range: Range::new(Position::new(0, 4), Position::new(0, 5)),
                new_text: "2".into(),
            },
            TextEdit {
                range: Range::new(Position::new(1, 0), Position::new(1, 3)),
                new_text: "bar".into(),
            },
        ];
        assert_eq!(apply_text_edits(content, &edits), "ń = 2;\nbar();\n");
    }

    #[test]
    fn an_inverted_range_is_skipped() {
        let edits = vec![TextEdit {
            range: Range::new(Position::new(0, 3), Position::new(0, 1)),
            new_text: "x".into(),
        }];
        assert_eq!(apply_text_edits("abcd", &edits), "abcd");
    }

    #[test]
    fn utf16_col_of_ascii_line_is_the_byte_offset() {
        assert_eq!(
            byte_offset_to_utf16_col("new _PHPStan_foo\\SomeClass()", 4),
            4
        );
        assert_eq!(
            byte_offset_to_utf16_col("new _PHPStan_foo\\SomeClass()", 25),
            25
        );
    }

    #[test]
    fn utf16_col_counts_a_multibyte_char_once() {
        // "é" is 2 bytes in UTF-8 but 1 UTF-16 code unit.
        assert_eq!(byte_offset_to_utf16_col("é_PHPStan_test\\Cls", 2), 1);
    }

    #[test]
    fn line_index_matches_offset_to_position() {
        // Include multi-byte (é = 2 bytes / 1 UTF-16 unit) and an emoji
        // (🎉 = 4 bytes / 2 UTF-16 units) to exercise the column math.
        let content = "<?php\n$x = 'é';\n// 🎉 comment\nfinal;\n";
        let index = LineIndex::new(content);
        // Every char boundary must agree with the scanning implementation.
        for (offset, _) in content
            .char_indices()
            .chain(std::iter::once((content.len(), ' ')))
        {
            assert_eq!(
                index.position(offset),
                offset_to_position(content, offset),
                "mismatch at offset {offset}"
            );
        }
    }

    #[test]
    fn line_index_matches_offset_to_position_mid_character() {
        // An offset landing inside the multi-byte 'é' must not panic, and
        // must agree with `offset_to_position`'s "answer the position
        // after that character" rule.
        let content = "<?php\nclass /*é*/ Foo {}\n";
        let index = LineIndex::new(content);
        let mid = content.find('é').unwrap() + 1;
        assert_eq!(index.position(mid), offset_to_position(content, mid));
    }

    #[test]
    fn line_index_clamps_past_end() {
        let content = "ab\ncd";
        let index = LineIndex::new(content);
        assert_eq!(
            index.position(999),
            offset_to_position(content, content.len())
        );
    }

    #[test]
    fn line_start_byte_offset_lf() {
        let content = "aaa\nbb\nc\n";
        assert_eq!(line_start_byte_offset(content, 0), 0);
        assert_eq!(line_start_byte_offset(content, 1), 4); // after "aaa\n"
        assert_eq!(line_start_byte_offset(content, 2), 7); // after "bb\n"
        assert_eq!(line_start_byte_offset(content, 3), 9); // after "c\n" (EOF)
    }

    #[test]
    fn line_start_byte_offset_crlf() {
        let content = "aaa\r\nbb\r\nc\r\n";
        assert_eq!(line_start_byte_offset(content, 0), 0);
        assert_eq!(line_start_byte_offset(content, 1), 5); // after "aaa\r\n"
        assert_eq!(line_start_byte_offset(content, 2), 9); // after "bb\r\n"
        assert_eq!(line_start_byte_offset(content, 3), 12); // after "c\r\n" (EOF)
    }

    #[test]
    fn line_start_byte_offset_past_end_returns_len() {
        let content = "one\ntwo";
        assert_eq!(line_start_byte_offset(content, 5), content.len());
    }

    #[test]
    fn offset_to_position_start_of_file() {
        let content = "<?php\necho 'hello';\n";
        assert_eq!(offset_to_position(content, 0), Position::new(0, 0));
    }

    #[test]
    fn offset_to_position_second_line() {
        let content = "<?php\necho 'hello';\n";
        // Offset 6 is the 'e' of 'echo' on line 1.
        assert_eq!(offset_to_position(content, 6), Position::new(1, 0));
    }

    #[test]
    fn offset_to_position_mid_line() {
        let content = "<?php\necho 'hello';\n";
        // Offset 10 is the '\'' before 'hello' (line 1, col 4).
        assert_eq!(offset_to_position(content, 10), Position::new(1, 4));
    }

    #[test]
    fn offset_to_position_end_of_content() {
        let content = "ab\ncd";
        // Offset 5 is past the last character.
        assert_eq!(offset_to_position(content, 5), Position::new(1, 2));
    }

    #[test]
    fn offset_to_position_multibyte_char() {
        // '€' is 3 bytes in UTF-8 but 1 code unit in UTF-16.
        let content = "€x";
        assert_eq!(offset_to_position(content, 3), Position::new(0, 1));
    }

    #[test]
    fn offset_to_position_inside_a_multibyte_char() {
        // An offset between the bytes of '€' answers the position after it.
        let content = "€x";
        assert_eq!(offset_to_position(content, 1), Position::new(0, 1));
    }
}
