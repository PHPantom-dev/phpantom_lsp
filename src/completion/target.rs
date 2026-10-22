//! Completion target extraction.
//!
//! This module contains the logic for detecting the access operator (`->` or
//! `::`) before the cursor and extracting the textual subject to its left
//! (e.g. `$this`, `self`, `$var`, `$this->prop`, `ClassName`).
//!
//! The low-level subject extraction helpers (walking backwards through
//! characters to find variables, call expressions, `::` subjects, etc.)
//! live in the shared [`crate::type_engine::subject_extraction`] module so they can be
//! reused by the definition resolver and future features (hover,
//! references).

use tower_lsp::lsp_types::*;

use crate::text_position::utf16_col_to_char_index;
use crate::text_scan::collapse_continuation_lines;
use crate::type_engine::subject_extraction::detect_access_operator;
use crate::types::*;

/// Detect the access operator before the cursor position and extract
/// both the `AccessKind` and the textual subject to its left.
///
/// Returns `None` when no `->` or `::` is found (i.e. `AccessKind::Other`).
pub fn extract_completion_target(content: &str, position: Position) -> Option<CompletionTarget> {
    let lines: Vec<&str> = content.lines().collect();
    if position.line as usize >= lines.len() {
        return None;
    }

    // Collapse multi-line method chains so that continuation lines
    // (starting with `->` or `?->`) are joined with preceding lines.
    let line_idx = position.line as usize;
    let cursor_col = utf16_col_to_char_index(lines[line_idx], position.character);
    let (line, col) = collapse_continuation_lines(&lines, line_idx, cursor_col);
    let chars: Vec<char> = line.chars().collect();

    let (subject, access_kind) = detect_access_operator(&chars, col)?;

    Some(CompletionTarget {
        access_kind,
        subject,
    })
}
