//! Moving diagnostics from the text they were computed on to the edited
//! text.
//!
//! Outside the edited functions the two texts are byte-for-byte the same,
//! so a position there moves by however much the edited functions before
//! it grew or shrank.  Diagnostics carry LSP positions (lines and UTF-16
//! columns), so each one goes through a byte offset in the old text and
//! comes back out as a position in the new one.

use tower_lsp_server::gen_lsp_types::{Diagnostic, Position, Range};

use crate::text_position::{LineIndex, utf16_col_to_byte_offset};

/// A function or method whose body an edit changed: the span of its
/// declaration in the old text and in the new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChangedSpan {
    pub(crate) old: (u32, u32),
    pub(crate) new: (u32, u32),
}

impl ChangedSpan {
    /// How many bytes longer the declaration got (negative when shorter).
    fn growth(&self) -> i64 {
        i64::from(self.new.1 - self.new.0) - i64::from(self.old.1 - self.old.0)
    }
}

/// Move the diagnostics the old text reported outside the changed spans
/// to their positions in the new text.  Those inside a changed span are
/// dropped, since the partial pass computes them afresh.
///
/// `None` when a diagnostic starts outside the changed spans but ends
/// inside one: there is no position in the new text to move that end to.
pub(crate) fn carry_over(
    old: &str,
    new: &str,
    changed: &[ChangedSpan],
    diagnostics: &[Diagnostic],
) -> Option<Vec<Diagnostic>> {
    if diagnostics.is_empty() {
        return Some(Vec::new());
    }
    let old_lines = LineIndex::new(old);
    let new_lines = LineIndex::new(new);
    let mut carried = Vec::with_capacity(diagnostics.len());
    for diagnostic in diagnostics {
        let start = byte_offset(&old_lines, diagnostic.range.start);
        if changed.iter().any(|span| contains(span.old, start)) {
            continue;
        }
        let end = byte_offset(&old_lines, diagnostic.range.end);
        if changed
            .iter()
            .any(|span| end > span.old.0 as usize && end < span.old.1 as usize)
        {
            return None;
        }
        let mut moved = diagnostic.clone();
        moved.range = Range {
            start: new_lines.position(shift(start, changed)),
            end: new_lines.position(shift(end, changed)),
        };
        carried.push(moved);
    }
    Some(carried)
}

/// Keep only the diagnostics that start inside one of `spans` of
/// `content`.
pub(crate) fn retain_within(
    content: &str,
    spans: &[(u32, u32)],
    diagnostics: &mut Vec<Diagnostic>,
) {
    if diagnostics.is_empty() {
        return;
    }
    let lines = LineIndex::new(content);
    diagnostics.retain(|diagnostic| {
        let start = byte_offset(&lines, diagnostic.range.start);
        spans.iter().any(|&span| contains(span, start))
    });
}

fn contains(span: (u32, u32), offset: usize) -> bool {
    offset >= span.0 as usize && offset < span.1 as usize
}

/// Where `offset`, which lies outside every changed span of the old text,
/// sits in the new text.
fn shift(offset: usize, changed: &[ChangedSpan]) -> usize {
    let growth: i64 = changed
        .iter()
        .filter(|span| span.old.1 as usize <= offset)
        .map(ChangedSpan::growth)
        .sum();
    (offset as i64 + growth) as usize
}

/// The byte offset of an LSP position, through a line table.
fn byte_offset(lines: &LineIndex<'_>, position: Position) -> usize {
    let content = lines.content();
    let line = position.line as usize;
    let start = lines.line_start(line);
    let end = lines.line_start(line + 1);
    start + utf16_col_to_byte_offset(&content[start..end], position.character)
}

#[cfg(test)]
mod tests {
    use tower_lsp_server::gen_lsp_types::DiagnosticSeverity;

    use super::*;
    use crate::text_position::byte_range_to_lsp_range;

    fn diagnostic_at(content: &str, needle: &str) -> Diagnostic {
        let start = content.find(needle).unwrap();
        Diagnostic {
            range: byte_range_to_lsp_range(content, start, start + needle.len()),
            severity: Some(DiagnosticSeverity::Error),
            message: needle.to_string().into(),
            ..Diagnostic::default()
        }
    }

    fn span_of(content: &str, needle: &str) -> (u32, u32) {
        let start = content.find(needle).unwrap();
        (start as u32, (start + needle.len()) as u32)
    }

    const OLD: &str = "<?php\nfunction a() { $x = 1; }\nfunction b() { 'é'; bad(); }\nfunction c() { worse(); }\n";
    const NEW: &str = "<?php\nfunction a() {\n    $x = 1;\n    $y = 2;\n}\nfunction b() { 'é'; bad(); }\nfunction c() { worse(); }\n";

    #[test]
    fn diagnostics_after_an_edited_function_move_with_it() {
        let changed = [ChangedSpan {
            old: span_of(OLD, "function a() { $x = 1; }"),
            new: span_of(NEW, "function a() {\n    $x = 1;\n    $y = 2;\n}"),
        }];
        let carried = carry_over(
            OLD,
            NEW,
            &changed,
            &[diagnostic_at(OLD, "bad()"), diagnostic_at(OLD, "worse()")],
        )
        .unwrap();
        // The multi-byte character before `bad()` must not throw the
        // column off.
        assert_eq!(
            carried,
            vec![diagnostic_at(NEW, "bad()"), diagnostic_at(NEW, "worse()")]
        );
    }

    #[test]
    fn diagnostics_inside_an_edited_function_are_dropped() {
        let changed = [ChangedSpan {
            old: span_of(OLD, "function b() { 'é'; bad(); }"),
            new: span_of(OLD, "function b() { 'é'; bad(); }"),
        }];
        let carried = carry_over(
            OLD,
            OLD,
            &changed,
            &[diagnostic_at(OLD, "$x = 1"), diagnostic_at(OLD, "bad()")],
        )
        .unwrap();
        assert_eq!(carried, vec![diagnostic_at(OLD, "$x = 1")]);
    }

    #[test]
    fn a_diagnostic_ending_inside_an_edited_function_cannot_be_moved() {
        let changed = [ChangedSpan {
            old: span_of(OLD, "function b() { 'é'; bad(); }"),
            new: span_of(OLD, "function b() { 'é'; bad(); }"),
        }];
        let start = OLD.find("$x").unwrap();
        let end = OLD.find("bad").unwrap();
        let straddling = Diagnostic {
            range: byte_range_to_lsp_range(OLD, start, end),
            ..Diagnostic::default()
        };
        assert!(carry_over(OLD, OLD, &changed, &[straddling]).is_none());
    }

    #[test]
    fn only_diagnostics_starting_inside_the_spans_are_retained() {
        let mut diagnostics = vec![diagnostic_at(NEW, "bad()"), diagnostic_at(NEW, "worse()")];
        retain_within(
            NEW,
            &[span_of(NEW, "function c() { worse(); }")],
            &mut diagnostics,
        );
        assert_eq!(diagnostics, vec![diagnostic_at(NEW, "worse()")]);
    }
}
