use tower_lsp::lsp_types::Position;

/// Source map from virtual PHP back to original Blade positions.
#[derive(Debug, Clone)]
pub struct BladeSourceMap {
    /// Per-line column anchor points.
    ///
    /// Each entry is a pair `(blade_utf16_col, php_utf16_col)` representing
    /// a synchronisation point: position `blade_utf16_col` in the original
    /// Blade line corresponds to position `php_utf16_col` in the virtual
    /// PHP line.  Between two adjacent anchors the mapping is linear (1:1
    /// for PHP content, 0:N for boilerplate replacements).
    pub adjustments: Vec<Vec<(u32, u32)>>,
    /// Number of prologue lines the preprocessor injected before the
    /// first template line.  At least [`super::PROLOGUE_LINES`]; larger
    /// when call-site-inferred `@var` declarations are injected.
    pub prologue_lines: u32,
}

impl Default for BladeSourceMap {
    fn default() -> Self {
        Self {
            adjustments: Vec::new(),
            prologue_lines: super::PROLOGUE_LINES,
        }
    }
}

impl BladeSourceMap {
    pub fn blade_to_php(&self, pos: Position) -> Position {
        let line = pos.line as usize;
        let virtual_line = line as u32 + self.prologue_lines;

        if line >= self.adjustments.len() {
            return Position {
                line: virtual_line,
                character: pos.character,
            };
        }

        let line_adj = &self.adjustments[line];
        if line_adj.is_empty() {
            return Position {
                line: virtual_line,
                character: pos.character,
            };
        }

        let mut best_b = 0;
        let mut best_p = 0;

        for (b, p) in line_adj.iter() {
            if *b <= pos.character {
                best_b = *b;
                best_p = *p;
            } else {
                break;
            }
        }

        let char_offset = pos.character.saturating_sub(best_b);

        Position {
            line: virtual_line,
            character: best_p + char_offset,
        }
    }

    /// Map a virtual-PHP position back to Blade, clamping prologue
    /// positions to the start of the template.
    ///
    /// Prefer [`Self::try_php_to_blade`] whenever the result becomes a text
    /// edit or a range the user is sent to: the clamp invents a position the
    /// template never had.
    pub fn php_to_blade(&self, pos: Position) -> Position {
        self.try_php_to_blade(pos).unwrap_or(Position {
            line: 0,
            character: 0,
        })
    }

    /// Map a virtual-PHP position back to Blade, or `None` when it falls in
    /// the preprocessor's prologue.
    ///
    /// The prologue holds declarations no template wrote (`$errors`,
    /// `$__env`, the injected `@var` docblocks, the `extends` clause of a
    /// synthesized `$this` wrapper class), so there is no template text
    /// behind it and no position to map to.
    pub fn try_php_to_blade(&self, pos: Position) -> Option<Position> {
        if pos.line < self.prologue_lines {
            return None;
        }
        let line = (pos.line - self.prologue_lines) as usize;

        if line >= self.adjustments.len() {
            return Some(Position {
                line: line as u32,
                character: pos.character,
            });
        }

        let line_adj = &self.adjustments[line];
        if line_adj.is_empty() {
            return Some(Position {
                line: line as u32,
                character: pos.character,
            });
        }

        let mut best_idx = 0;
        let mut best_b = 0;
        let mut best_p = 0;

        for (i, (b, p)) in line_adj.iter().enumerate() {
            if *p <= pos.character {
                best_idx = i;
                best_b = *b;
                best_p = *p;
            } else {
                break;
            }
        }

        let mut char_offset = pos.character.saturating_sub(best_p);

        if let Some((next_b, next_p)) = line_adj.get(best_idx + 1) {
            let max_b_offset = next_b.saturating_sub(best_b);
            let max_p_offset = next_p.saturating_sub(best_p);

            if max_p_offset == 0 {
                // PHP boilerplate mapped to zero-width Blade point?
                // This shouldn't happen with our anchor strategy, but be safe.
                return Some(Position {
                    line: line as u32,
                    character: best_b,
                });
            }

            if max_b_offset == 0 {
                // PHP boilerplate mapped to a single Blade position.
                // EVERYTHING in this PHP segment maps to best_b.
                return Some(Position {
                    line: line as u32,
                    character: best_b,
                });
            }

            // Normal 1:1 or N:M mapping.
            // If the ratios are different (e.g. multi-byte characters),
            // we could scale char_offset, but for PHPantom we mostly
            // deal with 1:1 code or 0:N boilerplate.
            // We'll stick to 1:1 interpolation but cap it to next_b.
            if char_offset > max_b_offset {
                char_offset = max_b_offset;
            }
        }

        Some(Position {
            line: line as u32,
            character: best_b + char_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blade::TemplateKind;
    use crate::blade::preprocessor::{preprocess, preprocess_with_vars};

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn map(adjustments: Vec<Vec<(u32, u32)>>) -> BladeSourceMap {
        BladeSourceMap {
            adjustments,
            ..BladeSourceMap::default()
        }
    }

    /// A line of plain HTML carries no anchors, so only the prologue the
    /// preprocessor injected separates the two coordinate systems.
    #[test]
    fn a_line_without_anchors_shifts_by_the_prologue_alone() {
        let map = map(vec![Vec::new()]);
        let prologue = map.prologue_lines;
        assert_eq!(map.blade_to_php(at(0, 7)), at(prologue, 7));
        assert_eq!(map.try_php_to_blade(at(prologue, 7)), Some(at(0, 7)));
    }

    /// `{{` becomes a shorter (or longer) piece of PHP, so everything
    /// after the anchor is offset by the difference.
    #[test]
    fn an_anchor_moves_the_column_by_its_own_offset() {
        let map = map(vec![vec![(0, 0), (3, 8)]]);
        let prologue = map.prologue_lines;
        assert_eq!(map.blade_to_php(at(0, 5)), at(prologue, 10));
        assert_eq!(map.try_php_to_blade(at(prologue, 10)), Some(at(0, 5)));
    }

    /// A line holding several echoes has several anchors; the one at or
    /// before the column is the one that applies.
    #[test]
    fn the_nearest_anchor_at_or_before_the_column_wins() {
        let map = map(vec![vec![(0, 0), (3, 8), (20, 40)]]);
        let prologue = map.prologue_lines;
        // Between the second and third anchors: the second applies.
        assert_eq!(map.blade_to_php(at(0, 10)), at(prologue, 15));
        // Past the last anchor: it applies, with no cap to hold it back.
        assert_eq!(map.blade_to_php(at(0, 25)), at(prologue, 45));
        assert_eq!(map.try_php_to_blade(at(prologue, 45)), Some(at(0, 25)));
    }

    /// A PHP column inside a stretch of boilerplate the template never
    /// wrote maps to the one Blade position the whole segment came from,
    /// rather than running past the next anchor.
    #[test]
    fn a_php_column_inside_a_boilerplate_segment_maps_to_its_anchor() {
        // Blade column 3 became PHP columns 8..30: 22 columns of generated
        // code behind no template text at all.
        let map = map(vec![vec![(0, 0), (3, 8), (3, 30)]]);
        let prologue = map.prologue_lines;
        for php_column in [8, 15, 29] {
            assert_eq!(
                map.try_php_to_blade(at(prologue, php_column)),
                Some(at(0, 3)),
                "column {php_column} sits inside the generated segment"
            );
        }
    }

    /// A segment where both sides advance, but by different amounts, is
    /// walked 1:1 and held at the next anchor rather than running past it.
    #[test]
    fn a_column_in_a_grown_segment_stops_at_the_next_anchor() {
        let map = map(vec![vec![(0, 0), (3, 8), (4, 30)]]);
        let prologue = map.prologue_lines;
        assert_eq!(map.try_php_to_blade(at(prologue, 8)), Some(at(0, 3)));
        assert_eq!(map.try_php_to_blade(at(prologue, 20)), Some(at(0, 4)));
    }

    /// The prologue holds declarations no template line stands behind, so
    /// there is no position to map a diagnostic in it back to.
    #[test]
    fn a_prologue_position_maps_to_no_template_position() {
        let map = map(vec![vec![(0, 0)]]);
        assert_eq!(map.try_php_to_blade(at(0, 0)), None);
        assert_eq!(map.try_php_to_blade(at(map.prologue_lines - 1, 4)), None);
        // The clamping variant answers the start of the template instead.
        assert_eq!(map.php_to_blade(at(0, 0)), at(0, 0));
    }

    /// A line past the recorded ones (the preprocessor's trailing wrapper)
    /// still maps, so a position there is not lost.
    #[test]
    fn a_line_past_the_recorded_ones_still_maps() {
        let map = map(vec![vec![(0, 0)]]);
        let prologue = map.prologue_lines;
        assert_eq!(map.try_php_to_blade(at(prologue + 4, 2)), Some(at(4, 2)));
        assert_eq!(map.blade_to_php(at(4, 2)), at(prologue + 4, 2));
    }

    /// The round trip is what every hover, diagnostic, and go-to-definition
    /// on a template rides on: a position on a variable in the template has
    /// to come back as that same position after a trip through the virtual
    /// PHP.
    ///
    /// Only the PHP the template actually wrote round-trips. `{{` and the
    /// directive keywords are replaced wholesale, so several of their
    /// columns share one PHP position and the trip back cannot tell them
    /// apart — which is why hover and diagnostics anchor on expressions.
    #[test]
    fn a_position_on_a_variable_round_trips() {
        let blade = "<h1>{{ $title }}</h1>\n\
                     @foreach ($rows as $row)\n\
                     <p>{{ $row->name }} and {{ $row->id }}</p>\n\
                     @endforeach\n";
        let (_, map) = preprocess(blade);
        let mut checked = 0;
        for (line, text) in blade.lines().enumerate() {
            for (start, _) in text.match_indices('$') {
                let end = start
                    + text[start + 1..]
                        .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                        .unwrap_or(text.len() - start - 1)
                    + 1;
                for character in start..end {
                    let position = at(line as u32, character as u32);
                    assert_eq!(
                        map.try_php_to_blade(map.blade_to_php(position)),
                        Some(position),
                        "line {line} column {character} of {text:?} did not survive the round trip"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 20, "the walk must reach every variable");
    }

    /// Declarations injected ahead of the template (a layout's variables,
    /// a backing class's members, a call site's inferred types) add
    /// prologue lines, and every one of them has to be accounted for or
    /// every position in the file is reported one line off.
    #[test]
    fn injected_declarations_do_not_shift_the_templates_own_lines() {
        let blade = "<h1>{{ $title }}</h1>\n<p>{{ $user }}</p>\n";
        let (php, map) = preprocess_with_vars(
            blade,
            &[
                ("title".to_string(), "string".to_string()),
                ("user".to_string(), "\\App\\Models\\User".to_string()),
            ],
            TemplateKind::View,
            None,
            None,
            &Default::default(),
        );
        assert!(
            map.prologue_lines > super::super::PROLOGUE_LINES,
            "the injected declarations must be counted as prologue"
        );
        let title = map.blade_to_php(at(0, 7));
        assert_eq!(map.try_php_to_blade(title), Some(at(0, 7)));
        // The mapped line really is the template's first line in the PHP.
        assert!(
            php.lines()
                .nth(title.line as usize)
                .is_some_and(|line| line.contains("$title")),
            "the first template line must sit where the map says it does"
        );
    }
}
