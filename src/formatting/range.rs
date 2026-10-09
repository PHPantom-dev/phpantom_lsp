//! Range formatting on top of whole-document formatters.
//!
//! None of the formatters PHPantom drives can format part of a file:
//! mago-formatter, php-cs-fixer, Pint and phpcbf all take a whole file,
//! and formatting the selected lines as an isolated snippet breaks on a
//! selection that starts or ends mid-statement and loses the indentation
//! the surrounding code gives it.  So the whole document is formatted,
//! diffed line by line against the original, and only the changed hunks
//! that touch the requested lines are returned.
//!
//! A hunk that straddles the edge of the range is returned whole: the
//! formatter's output is only known to be valid as a unit, so splitting a
//! hunk could leave the file half-formatted mid-construct.  The exception
//! is a line the formatter only respaced, which can always stand alone.
//! The opposite holds for lines the formatter moved (sorted imports,
//! reordered members): the diff sees a deletion in one place and an
//! insertion in another, and applying only one of them would drop or
//! duplicate the line, so hunks linked by a move go together.

use std::collections::HashMap;
use std::ops::Range as Lines;

use similar::{Algorithm, DiffOp, capture_diff_slices};
use tower_lsp::lsp_types::{Position, Range, TextEdit};

/// The `TextEdit`s that turn `original` into `formatted` within `range`.
///
/// Lines outside the range keep their original text, except where a
/// changed hunk that touches the range extends past it.
pub(crate) fn compute_range_edits(original: &str, formatted: &str, range: Range) -> Vec<TextEdit> {
    if original == formatted {
        return Vec::new();
    }

    let old_lines: Vec<&str> = original.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = formatted.split_inclusive('\n').collect();
    let mut collector = Collector {
        old_lines: &old_lines,
        new_lines: &new_lines,
        selected: selected_lines(range),
        edits: Vec::new(),
    };

    let hunks = hunks(&old_lines, &new_lines);
    let groups = collector.move_groups(&hunks);
    let mut group_size = vec![0usize; hunks.len()];
    let mut group_touched = vec![false; hunks.len()];
    for ((old, _), &group) in hunks.iter().zip(&groups) {
        group_size[group] += 1;
        group_touched[group] |= collector.touches(old);
    }
    for ((old, new), &group) in hunks.into_iter().zip(&groups) {
        if group_size[group] == 1 {
            collector.hunk(old, new);
        } else if group_touched[group] {
            collector.push(old, new);
        }
    }
    collector.edits
}

/// The changed runs of lines, as (original lines, formatted lines).
/// Adjacent delete and insert ops are one change.
fn hunks(old_lines: &[&str], new_lines: &[&str]) -> Vec<(Lines<usize>, Lines<usize>)> {
    let mut hunks: Vec<(Lines<usize>, Lines<usize>)> = Vec::new();
    let mut open = false;
    for op in capture_diff_slices(Algorithm::Myers, old_lines, new_lines) {
        if let DiffOp::Equal { .. } = op {
            open = false;
        } else if let Some((old, new)) = hunks.last_mut().filter(|_| open) {
            old.end = op.old_range().end;
            new.end = op.new_range().end;
        } else {
            hunks.push((op.old_range(), op.new_range()));
            open = true;
        }
    }
    hunks
}

/// The first and last line the range selects, inclusive.
///
/// A multi-line selection that ends at the start of a line does not
/// select that line: selecting whole lines in an editor puts the end
/// there.
fn selected_lines(range: Range) -> (usize, usize) {
    let first = range.start.line as usize;
    let mut last = range.end.line as usize;
    if range.end.character == 0 && last > first {
        last -= 1;
    }
    (first, last.max(first))
}

/// A line's text with all whitespace removed, which is what survives
/// the formatter respacing it.
fn tokens(line: &str) -> String {
    line.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Whether two lines differ only in whitespace.
fn same_tokens(a: &str, b: &str) -> bool {
    a.chars()
        .filter(|c| !c.is_whitespace())
        .eq(b.chars().filter(|c| !c.is_whitespace()))
}

struct Collector<'a> {
    old_lines: &'a [&'a str],
    new_lines: &'a [&'a str],
    selected: (usize, usize),
    edits: Vec<TextEdit>,
}

impl Collector<'_> {
    /// Group the hunks linked by a moved line: one deletes a line that
    /// another inserts.  Returns each hunk's group, named by its lowest
    /// hunk index.  Blank lines are not evidence of a move.
    fn move_groups(&self, hunks: &[(Lines<usize>, Lines<usize>)]) -> Vec<usize> {
        let mut deleted: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, (old, _)) in hunks.iter().enumerate() {
            for line in &self.old_lines[old.clone()] {
                let key = tokens(line);
                if !key.is_empty() {
                    deleted.entry(key).or_default().push(index);
                }
            }
        }

        let mut parent: Vec<usize> = (0..hunks.len()).collect();
        fn root(parent: &mut [usize], mut index: usize) -> usize {
            while parent[index] != index {
                parent[index] = parent[parent[index]];
                index = parent[index];
            }
            index
        }
        for (index, (_, new)) in hunks.iter().enumerate() {
            for line in &self.new_lines[new.clone()] {
                for &other in deleted.get(&tokens(line)).into_iter().flatten() {
                    if other != index {
                        let (a, b) = (root(&mut parent, index), root(&mut parent, other));
                        parent[a.max(b)] = a.min(b);
                    }
                }
            }
        }
        (0..hunks.len())
            .map(|index| root(&mut parent, index))
            .collect()
    }

    /// Collect the edits for the hunk replacing original lines `old` with
    /// formatted lines `new`.
    ///
    /// A hunk that keeps its line count is usually a run of lines the
    /// formatter respaced in place (a reindented block).  A line whose
    /// non-whitespace text is unchanged has kept all its tokens, so it
    /// is applied on its own; only runs of lines whose content changed
    /// have to go together.
    fn hunk(&mut self, old: Lines<usize>, new: Lines<usize>) {
        if old.len() != new.len() {
            self.edit(old, new);
            return;
        }
        let to_new = |lines: Lines<usize>| {
            let start = new.start + (lines.start - old.start);
            start..start + lines.len()
        };
        let mut changed_from = old.start;
        for line in old.clone() {
            if same_tokens(
                self.old_lines[line],
                self.new_lines[to_new(line..line).start],
            ) {
                self.edit(changed_from..line, to_new(changed_from..line));
                self.edit(line..line + 1, to_new(line..line + 1));
                changed_from = line + 1;
            }
        }
        self.edit(changed_from..old.end, to_new(changed_from..old.end));
    }

    /// Collect the edit replacing original lines `old` with formatted
    /// lines `new`, if it changes anything and touches the selection.
    fn edit(&mut self, old: Lines<usize>, new: Lines<usize>) {
        if self.touches(&old) {
            self.push(old, new);
        }
    }

    /// Whether replacing original lines `old` is inside the selection.
    fn touches(&self, old: &Lines<usize>) -> bool {
        let (first, last) = self.selected;
        // A pure insertion sits between two lines, so one at either edge
        // of the selection (a blank line added before or after it)
        // belongs to it.
        if old.is_empty() {
            (first..=last + 1).contains(&old.start)
        } else {
            old.start <= last && old.end > first
        }
    }

    /// Collect the edit replacing original lines `old` with formatted
    /// lines `new`, unless it changes nothing.
    fn push(&mut self, old: Lines<usize>, new: Lines<usize>) {
        if self.old_lines[old.clone()] == self.new_lines[new.clone()] {
            return;
        }
        self.edits.push(TextEdit {
            range: Range {
                start: self.line_position(old.start),
                end: self.line_position(old.end),
            },
            new_text: self.new_lines[new].concat(),
        });
    }

    /// The position at the start of original line `line`, or the end of
    /// the text for the line past the last.
    fn line_position(&self, line: usize) -> Position {
        match self.old_lines.last() {
            Some(last) if line == self.old_lines.len() && !last.ends_with('\n') => Position {
                line: line as u32 - 1,
                character: last.encode_utf16().count() as u32,
            },
            _ => Position {
                line: line as u32,
                character: 0,
            },
        }
    }
}
