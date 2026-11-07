//! Exact mappings for copied PHP regions, including reordered attributes.

use tower_lsp_server::gen_lsp_types::{Position, Range};

/// Bidirectional lookup of copied regions in source and generated order.
#[derive(Debug, Clone, Default)]
pub struct SourceSpans {
    spans: Vec<(Range, Range)>,
    source_order: Vec<usize>,
}

impl SourceSpans {
    pub(crate) fn push(&mut self, source: Range, php: Range) {
        self.spans.push((source, php));
    }

    pub(crate) fn finish(&mut self) {
        self.source_order = (0..self.spans.len()).collect();
        self.source_order
            .sort_unstable_by_key(|&index| key(self.spans[index].0.start));
    }

    pub(crate) fn to_php(&self, pos: Position) -> Option<Position> {
        let index = self
            .source_order
            .partition_point(|&index| key(self.spans[index].0.start) <= key(pos))
            .checked_sub(1)?;
        let (source, php) = self.spans[self.source_order[index]];
        translate(pos, source, php)
    }

    pub(crate) fn to_source(&self, pos: Position) -> Option<Position> {
        let index = self
            .spans
            .partition_point(|(_, php)| key(php.start) <= key(pos))
            .checked_sub(1)?;
        let (source, php) = self.spans[index];
        translate(pos, php, source)
    }

    /// An edit spanning generated syntax or reordered regions cannot be
    /// represented as one edit to the template.
    pub(crate) fn to_source_range(&self, range: Range) -> Option<Range> {
        let index = self
            .spans
            .partition_point(|(_, php)| key(php.start) <= key(range.start))
            .checked_sub(1)?;
        let (source, php) = self.spans[index];
        Some(Range {
            start: translate(range.start, php, source)?,
            end: translate(range.end, php, source)?,
        })
    }
}

fn key(pos: Position) -> (u32, u32) {
    (pos.line, pos.character)
}

fn translate(pos: Position, from: Range, to: Range) -> Option<Position> {
    if key(pos) > key(from.end) {
        return None;
    }
    Some(Position {
        line: to.start.line + pos.line - from.start.line,
        character: if pos.line == from.start.line {
            to.start.character + pos.character - from.start.character
        } else {
            pos.character
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(line: u32, start: u32, end_line: u32, end: u32) -> Range {
        Range {
            start: Position::new(line, start),
            end: Position::new(end_line, end),
        }
    }

    #[test]
    fn tempest_reordered_multiline_regions_round_trip_and_reject_generated_code() {
        let mut map = SourceSpans::default();
        assert_eq!(map.to_php(Position::default()), None);
        assert_eq!(map.to_source(Position::default()), None);
        map.push(range(3, 8, 4, 12), range(2, 9, 3, 12));
        map.push(range(1, 4, 1, 10), range(4, 0, 4, 6));
        map.finish();
        for (source, php) in [
            (Position::new(3, 10), Position::new(2, 11)),
            (Position::new(4, 5), Position::new(3, 5)),
            (Position::new(1, 10), Position::new(4, 6)),
        ] {
            assert_eq!(map.to_php(source), Some(php));
            assert_eq!(map.to_source(php), Some(source));
        }
        for source in [
            Position::default(),
            Position::new(2, 0),
            Position::new(5, 0),
        ] {
            assert_eq!(map.to_php(source), None);
        }
        for php in [
            Position::default(),
            Position::new(3, 13),
            Position::new(5, 0),
        ] {
            assert_eq!(map.to_source(php), None);
        }
        assert_eq!(
            map.to_source_range(range(4, 1, 4, 5)),
            Some(range(1, 5, 1, 9))
        );
        assert_eq!(map.to_source_range(range(2, 9, 4, 1)), None);
        assert_eq!(map.to_source_range(range(0, 0, 0, 1)), None);
    }
}
