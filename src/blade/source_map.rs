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

/// Which side of an anchor pair a lookup compares against.
#[derive(Clone, Copy)]
enum AnchorSide {
    /// The template's own column.
    Blade,
    /// The virtual PHP's column.
    Php,
}

/// The last anchor on a line whose `side` column is at or before
/// `character`, as `(index, blade_column, php_column)`.
///
/// Anchors are recorded in column order, so the first one past
/// `character` ends the search.
fn anchor_at(line_adj: &[(u32, u32)], character: u32, side: AnchorSide) -> (usize, u32, u32) {
    let mut best = (0, 0, 0);
    for (i, (blade, php)) in line_adj.iter().enumerate() {
        let column = match side {
            AnchorSide::Blade => *blade,
            AnchorSide::Php => *php,
        };
        if column > character {
            break;
        }
        best = (i, *blade, *php);
    }
    best
}

impl BladeSourceMap {
    /// Map a Blade position into the virtual PHP.
    ///
    /// A column inside template text the preprocessor replaced (a tag name,
    /// a `{{` opener, a directive keyword, an argument list it skipped) has
    /// no PHP of its own: it maps to where the replacement ends, never into
    /// the PHP behind it.
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

        let (best_idx, best_b, best_p) = anchor_at(line_adj, pos.character, AnchorSide::Blade);
        let mut char_offset = pos.character.saturating_sub(best_b);

        // Walk the segment 1:1, but never past where it ends in the PHP: a
        // replacement is narrower than the text it replaced, and the columns
        // it has no room for would land in the PHP generated behind it.
        if let Some((_, next_p)) = line_adj.get(best_idx + 1) {
            char_offset = char_offset.min(next_p.saturating_sub(best_p));
        }

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

        let (best_idx, best_b, best_p) = anchor_at(line_adj, pos.character, AnchorSide::Php);
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
    use crate::blade::preprocessor::{
        ComponentBinding, ComponentParameter, ComponentResolver, ComponentTarget, preprocess,
        preprocess_with_vars,
    };

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

    /// A Blade column inside text that was replaced by generated PHP maps
    /// to where the replacement ends, the one PHP position the whole span
    /// lowers to, rather than running into the PHP after it by the
    /// column's distance into the span.
    #[test]
    fn a_blade_column_inside_a_replaced_span_maps_to_the_end_of_the_replacement() {
        // Blade columns 0..8 became 15 columns of generated PHP, and the
        // text after them lowered to more generated PHP at column 16.
        let map = map(vec![vec![
            (0, 0),
            (0, 15),
            (8, 15),
            (9, 16),
            (9, 44),
            (18, 44),
        ]]);
        let prologue = map.prologue_lines;
        for blade_column in 0..=8 {
            assert_eq!(
                map.blade_to_php(at(0, blade_column)),
                at(prologue, 15),
                "column {blade_column} sits inside the replaced span"
            );
        }
    }

    /// Template text a construct consumed without emitting anything of its
    /// own (an argument list the preprocessor skips) lowers to the position
    /// the generated PHP after it starts at, so nothing in it can reach
    /// into that PHP.
    #[test]
    fn a_blade_column_inside_skipped_text_does_not_reach_the_php_after_it() {
        // `(0, 0), (0, 11), (8, 11)` is the keyword's replacement; the
        // skipped argument list is Blade columns 8..16, and the suffix
        // generated once it closes starts at the same PHP column.
        let map = map(vec![vec![(0, 0), (0, 11), (8, 11), (16, 11), (16, 25)]]);
        let prologue = map.prologue_lines;
        for blade_column in 8..16 {
            assert_eq!(
                map.blade_to_php(at(0, blade_column)),
                at(prologue, 11),
                "column {blade_column} sits inside the skipped argument list"
            );
        }
    }

    /// A segment that is narrower in the PHP than in the template is walked
    /// 1:1 and held at the next anchor rather than running past it, the way
    /// the trip back holds a grown one.
    #[test]
    fn a_blade_column_in_a_narrowed_segment_stops_at_the_next_anchor() {
        let map = map(vec![vec![(0, 0), (3, 8), (8, 10), (9, 30)]]);
        let prologue = map.prologue_lines;
        assert_eq!(map.blade_to_php(at(0, 3)), at(prologue, 8));
        assert_eq!(map.blade_to_php(at(0, 4)), at(prologue, 9));
        assert_eq!(map.blade_to_php(at(0, 5)), at(prologue, 10));
        assert_eq!(map.blade_to_php(at(0, 7)), at(prologue, 10));
        assert_eq!(map.blade_to_php(at(0, 8)), at(prologue, 10));
    }

    /// The tag name of a component tag is replaced wholesale, and whatever
    /// the tag emits next (a bound attribute's call or assignment) starts
    /// right behind it, so a column in the name must not land in that.
    #[test]
    fn a_column_in_a_component_tag_name_does_not_reach_the_php_the_tag_emits_next() {
        struct Known;
        impl ComponentResolver for Known {
            fn x_component(&self, tag: &str) -> Option<ComponentTarget> {
                match tag {
                    "card" => Some(ComponentTarget {
                        fqn: "App\\View\\Components\\Card".to_string(),
                        binding: ComponentBinding::Construct(vec![ComponentParameter {
                            name: "bakery".to_string(),
                            fallback: None,
                        }]),
                    }),
                    "alert" => Some(ComponentTarget {
                        fqn: "App\\View\\Components\\Alert".to_string(),
                        binding: ComponentBinding::Declare,
                    }),
                    _ => None,
                }
            }
            fn livewire_component(&self, _: &str) -> Option<ComponentTarget> {
                None
            }
        }

        for tag in [
            // Unknown to the project: the name becomes a comment, and the
            // bound attribute behind it a `blade_bound_attr_directive(` call.
            "<x-panel :author=\"$posts->first()?->author\" heading=\"Latest\">",
            // A constructor call, whose name becomes a single space, and the
            // bound attribute that is its argument an assignment.
            "<x-card :bakery=\"$bakery\">",
            // A plain first attribute, which the tag's call swallows.
            "<x-card class=\"mt-4\">",
            // A component with no signature, declaring a variable.
            "<x-alert :title=\"$title\">",
        ] {
            let (php, map) = preprocess_with_vars(
                tag,
                &[],
                TemplateKind::View,
                None,
                Some(&Known),
                &Default::default(),
            );
            let name_len = tag.find(' ').expect("the tag has attributes");
            let virtual_line = php
                .lines()
                .nth(map.prologue_lines as usize)
                .expect("the tag's line");
            let start = map.blade_to_php(at(0, 0));
            for column in 1..name_len {
                assert_eq!(
                    map.blade_to_php(at(0, column as u32)),
                    start,
                    "column {column} of {tag:?} must map where the tag's start does"
                );
            }
            // The line is ASCII, so a column indexes its characters.
            assert!(
                virtual_line
                    .chars()
                    .nth(start.character as usize)
                    .is_none_or(char::is_whitespace),
                "the tag name of {tag:?} must lower to a position that is not \
                 generated code: {virtual_line:?}"
            );
        }
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
