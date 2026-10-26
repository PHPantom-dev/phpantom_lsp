//! Moving a code action's edits from the virtual PHP they were planned
//! against back into a Blade template, all or nothing.
//!
//! Every collector plans against the virtual PHP, because that is what the
//! symbol map and the type engine describe, but most of that text is not
//! the template's: the prologue, the wrapper the body sits in, and the PHP
//! each directive, echo, and component tag lowers to.  An edit is only
//! moved back when it touches text the template wrote, and an action with
//! one edit that does not is dropped whole, since applying the rest would
//! leave a refactoring half done.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Position, Range};

use super::source_map::BladeSourceMap;
use crate::Backend;

/// A template, the virtual PHP it lowers to, and the map between them,
/// for checking and translating the edits of the actions offered on it.
pub(crate) struct TemplateText {
    uri: String,
    template: Arc<String>,
    template_lines: Vec<usize>,
    virtual_php: Arc<String>,
    virtual_lines: Vec<usize>,
    map: Arc<BladeSourceMap>,
    /// The virtual positions an import is written at, chosen by
    /// [`super::use_block`].
    import_anchors: Vec<Position>,
}

impl Backend {
    /// The template `uri` names with its recorded lowering, or `None` when
    /// it is not a template, was never lowered, or was edited since: the
    /// lowering is refreshed in the background, and an edit moved through a
    /// lowering of older text lands on the wrong template text.
    pub(crate) fn template_text(&self, uri: &str) -> Option<TemplateText> {
        if !self.is_blade_file(uri) {
            return None;
        }
        let virtual_php = self.blade_virtual_php_arc(uri)?;
        let map = self.blade_source_map(uri)?;
        let template = self.get_file_content_arc(uri)?;
        if !map.lowered_from(&template) {
            return None;
        }
        Some(TemplateText::new(uri, template, virtual_php, map))
    }
}

impl TemplateText {
    fn new(
        uri: &str,
        template: Arc<String>,
        virtual_php: Arc<String>,
        map: Arc<BladeSourceMap>,
    ) -> Self {
        let import_anchors = super::use_block::analyze_template_use_block(&template, Some(&map))
            .template
            .map(|block| block.anchors().collect())
            .unwrap_or_default();
        TemplateText {
            uri: uri.to_string(),
            template_lines: line_starts(&template),
            template,
            virtual_lines: line_starts(&virtual_php),
            virtual_php,
            map,
            import_anchors,
        }
    }

    /// The template's URI.
    pub(crate) fn uri(&self) -> &str {
        &self.uri
    }

    /// The template's own text.
    pub(crate) fn template(&self) -> &str {
        &self.template
    }

    /// Whether the template wrote the virtual PHP between the byte offsets
    /// `start` and `end`, rather than the lowering generating it.
    pub(crate) fn wrote_bytes(&self, start: usize, end: usize) -> bool {
        let (Some(start), Some(end)) = (
            position_at(&self.virtual_php, &self.virtual_lines, start),
            position_at(&self.virtual_php, &self.virtual_lines, end),
        ) else {
            return false;
        };
        self.wrote(Range { start, end }).is_some()
    }

    /// The template range behind the virtual `range`, when the template
    /// wrote exactly the text the virtual PHP holds there.
    ///
    /// A CRLF template's line breaks come out bare in the virtual PHP, so
    /// they compare equal to the template's.
    fn wrote(&self, range: Range) -> Option<Range> {
        let translated = Range {
            start: self.map.try_php_to_blade(range.start)?,
            end: self.map.try_php_to_blade(range.end)?,
        };
        let virtual_text = slice(&self.virtual_php, &self.virtual_lines, range)?;
        let template_text = slice(&self.template, &self.template_lines, translated)?;
        let same = if template_text.contains('\r') {
            template_text.replace("\r\n", "\n") == virtual_text
        } else {
            template_text == virtual_text
        };
        same.then_some(translated)
    }

    /// The template position an insertion at the virtual `position` goes
    /// to, when it lands between text the template wrote.
    ///
    /// The position has to come back to itself through the template, and
    /// the characters either side of it on its line have to be the
    /// template's own: a statement inserted where `@php(`'s argument starts
    /// would split the directive from it, and one inserted where an echo's
    /// generated `echo` starts would land in HTML.
    fn insertion_point(&self, position: Position) -> Option<Position> {
        let translated = self.map.try_php_to_blade(position)?;
        if self.map.blade_to_php(translated) != position {
            return None;
        }
        let in_template = neighbours(&self.template, &self.template_lines, translated)?;
        let in_virtual = neighbours(&self.virtual_php, &self.virtual_lines, position)?;
        if in_template != in_virtual {
            return None;
        }
        // The character before has to be the one the virtual PHP has there,
        // not just an equal one the lowering generated.
        if position.character > 0 {
            let before = |text: &str, lines: &[usize], at: Position| {
                let offset = offset_at(text, lines, at)?;
                let len = text[..offset].chars().next_back()?.len_utf16() as u32;
                Some(Position {
                    character: at.character - len,
                    ..at
                })
            };
            let template_before = before(&self.template, &self.template_lines, translated)?;
            let virtual_before = before(&self.virtual_php, &self.virtual_lines, position)?;
            if self.map.blade_to_php(template_before) != virtual_before {
                return None;
            }
        }
        Some(translated)
    }

    /// The template range an edit planned at the virtual `range` applies
    /// to, or `None` when the edit touches text the template did not write.
    ///
    /// An import is the one edit written in Blade rather than PHP, at one
    /// of the positions [`super::use_block`] chose for it, which need not
    /// lie between template text: a template's first line is often a
    /// directive.
    pub(crate) fn translate_edit(&self, range: Range, new_text: &str) -> Option<Range> {
        if range.start != range.end {
            return self.wrote(range);
        }
        let position = if self.import_anchors.contains(&range.start) && is_import(new_text) {
            let translated = self.map.try_php_to_blade(range.start)?;
            offset_at(&self.template, &self.template_lines, translated)?;
            translated
        } else {
            self.insertion_point(range.start)?
        };
        Some(Range {
            start: position,
            end: position,
        })
    }
}

/// Whether `new_text` imports a name, as the `@use` directive or as the
/// `use` statement written inside a block of PHP the template opens with.
fn is_import(new_text: &str) -> bool {
    let text = new_text.trim_start();
    text.starts_with("@use(") || text.starts_with("use ")
}

/// The byte offset each line of `text` starts at.
fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect()
}

/// The bytes of line `line`, without its terminator.
fn line_bounds(text: &str, lines: &[usize], line: u32) -> Option<(usize, usize)> {
    let start = *lines.get(line as usize)?;
    let mut end = lines
        .get(line as usize + 1)
        .map_or(text.len(), |next| next - 1);
    if text[start..end].ends_with('\r') {
        end -= 1;
    }
    Some((start, end))
}

/// The byte offset of `position`, or `None` when the position is past the
/// end of its line or of the text, or splits a character.
fn offset_at(text: &str, lines: &[usize], position: Position) -> Option<usize> {
    let (start, end) = line_bounds(text, lines, position.line)?;
    let mut units = 0;
    for (at, ch) in text[start..end].char_indices() {
        if units == position.character {
            return Some(start + at);
        }
        units += ch.len_utf16() as u32;
    }
    (units == position.character).then_some(end)
}

/// The position of the byte offset `offset`.
fn position_at(text: &str, lines: &[usize], offset: usize) -> Option<Position> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    let line = lines.partition_point(|&start| start <= offset) - 1;
    let character = text[lines[line]..offset]
        .chars()
        .map(|ch| ch.len_utf16() as u32)
        .sum();
    Some(Position {
        line: line as u32,
        character,
    })
}

/// The text `range` covers.
fn slice<'a>(text: &'a str, lines: &[usize], range: Range) -> Option<&'a str> {
    let start = offset_at(text, lines, range.start)?;
    let end = offset_at(text, lines, range.end)?;
    text.get(start..end)
}

/// The characters on either side of `position` on its line.
fn neighbours(
    text: &str,
    lines: &[usize],
    position: Position,
) -> Option<(Option<char>, Option<char>)> {
    let (start, end) = line_bounds(text, lines, position.line)?;
    let at = offset_at(text, lines, position)?;
    Some((
        text[start..at].chars().next_back(),
        text[at..end].chars().next(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blade::preprocessor::preprocess;

    fn at(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn lowered(template: &str) -> TemplateText {
        let (php, map) = preprocess(template);
        TemplateText::new(
            "file:///page.blade.php",
            Arc::new(template.to_string()),
            Arc::new(php),
            Arc::new(map),
        )
    }

    /// The virtual position template line `line`, column `character`
    /// lowers to.
    fn virtual_at(text: &TemplateText, line: u32, character: u32) -> Position {
        text.map.blade_to_php(at(line, character))
    }

    fn insertion(position: Position) -> Range {
        Range::new(position, position)
    }

    #[test]
    fn a_replacement_of_the_templates_own_php_moves_back() {
        let text = lowered("@php\n    $total = strlen('abc');\n@endphp\n");
        let virtual_range = Range::new(virtual_at(&text, 1, 13), virtual_at(&text, 1, 26));
        assert_eq!(
            text.translate_edit(virtual_range, "$length"),
            Some(Range::new(at(1, 13), at(1, 26)))
        );
    }

    #[test]
    fn a_replacement_of_generated_php_is_rejected() {
        let text = lowered("<p>{{ $title }}</p>\n");
        let title = virtual_at(&text, 0, 7);
        let line_start = Position {
            character: 0,
            ..title
        };
        // The span from the line's start runs over the `echo e(` the echo
        // lowers to.
        assert_eq!(text.translate_edit(Range::new(line_start, title), ""), None);
    }

    #[test]
    fn an_insertion_between_the_templates_own_php_moves_back() {
        let text = lowered("@php\n    $total = 1;\n@endphp\n");
        assert_eq!(
            text.translate_edit(insertion(virtual_at(&text, 1, 4)), "$x = 2;\n    "),
            Some(insertion(at(1, 4)))
        );
    }

    #[test]
    fn an_insertion_where_generated_php_starts_is_rejected() {
        // The statement an echo lowers to starts at its generated `echo`.
        let text = lowered("<p>{{ strlen('abc') }}</p>\n");
        let echo_line = virtual_at(&text, 0, 0).line;
        let line = text.virtual_php.lines().nth(echo_line as usize).unwrap();
        let echo = line.find("echo").unwrap() as u32;
        assert_eq!(
            text.translate_edit(insertion(at(echo_line, echo)), "$x = 1;\n"),
            None
        );

        // The statement a `@php(...)` one-liner holds starts at its argument.
        let text = lowered("@php($total = 1)\n");
        let statement_line = virtual_at(&text, 0, 0).line;
        let line = text
            .virtual_php
            .lines()
            .nth(statement_line as usize)
            .unwrap();
        let statement = line.find('(').unwrap() as u32;
        assert_eq!(
            text.translate_edit(insertion(at(statement_line, statement)), "$x = 1;\n"),
            None
        );
    }

    #[test]
    fn an_edit_outside_the_templates_lines_is_rejected() {
        let text = lowered("@php\n    $total = 1;\n@endphp\n");
        // The prologue.
        assert_eq!(text.translate_edit(insertion(at(1, 0)), "$x = 1;\n"), None);
        // The wrapper's closing brace, after the template's last line.
        let closing = text.virtual_php.lines().count() as u32 - 1;
        assert_eq!(
            text.translate_edit(insertion(at(closing, 0)), "$x = 1;\n"),
            None
        );
    }

    #[test]
    fn an_import_goes_where_the_use_block_put_it() {
        let text = lowered("@if ($show)\n    {{ $x }}\n@endif\n");
        let top = text.import_anchors[0];
        assert_eq!(
            text.translate_edit(insertion(top), "@use('App\\Models\\Widget')\n"),
            Some(insertion(at(0, 0)))
        );
        assert_eq!(
            text.translate_edit(insertion(top), "$x = 1;\n"),
            None,
            "only an import is written at an import's position"
        );
    }

    #[test]
    fn a_span_the_lowering_wrote_is_not_the_templates() {
        let text = lowered("<p>{{ $title }}</p>\n");
        let php = &text.virtual_php;
        let errors = php.find("\\Illuminate\\Support\\ViewErrorBag").unwrap();
        assert!(!text.wrote_bytes(errors, errors + 30));
        let title = php.rfind("$title").unwrap();
        assert!(text.wrote_bytes(title, title + "$title".len()));
    }

    #[test]
    fn offsets_and_positions_agree_on_every_character() {
        let text = "a\r\nbé😀c\n\nd";
        let lines = line_starts(text);
        for (offset, _) in text
            .char_indices()
            .filter(|(_, ch)| !matches!(ch, '\r' | '\n'))
        {
            let position = position_at(text, &lines, offset).expect("a position");
            assert_eq!(offset_at(text, &lines, position), Some(offset));
        }
        assert_eq!(offset_at(text, &lines, at(0, 2)), None, "past the line");
        assert_eq!(
            offset_at(text, &lines, at(1, 3)),
            None,
            "inside a surrogate pair"
        );
        assert_eq!(offset_at(text, &lines, at(4, 0)), None, "past the text");
        assert_eq!(
            slice(text, &lines, Range::new(at(0, 0), at(1, 1))),
            Some("a\r\nb")
        );
    }
}
