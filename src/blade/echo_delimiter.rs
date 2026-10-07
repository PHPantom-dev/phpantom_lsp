//! Hover and go-to-definition for the `{{ … }}` and `{!! … !!}` echo
//! delimiters. The first compile to an implicit `e()` call the template
//! never spells out and the second to a bare `echo`, so neither has PHP of
//! its own for a request on it to land on.

use tower_lsp::lsp_types::{
    Hover, HoverContents, Location, MarkupContent, MarkupKind, Position, Range,
};

use super::directive_completion::{Mode, mode_at};
use crate::text_position::{offset_to_position, position_to_byte_offset};

/// An echo delimiter the cursor is on.
struct EchoDelimiter {
    /// Byte offset of its first character.
    start: usize,
    /// Its length in characters, which for these ASCII delimiters is also
    /// its length in bytes and in UTF-16 units.
    len: u32,
    /// Whether it belongs to a raw `{!! … !!}` echo rather than an escaped
    /// `{{ … }}` one.
    raw: bool,
}

/// What an echo is delimited with: each delimiter's text, whether it
/// belongs to a raw echo, and the scanner [`Mode`] that holds where it
/// starts. An opener starts in template markup and a closer in the code of
/// the echo it ends.
const DELIMITERS: [(&str, bool, Mode); 4] = [
    ("{{", false, Mode::Html),
    ("}}", false, Mode::UntilMarkerInCode("}}")),
    ("{!!", true, Mode::Html),
    ("!!}", true, Mode::UntilMarkerInCode("!!}")),
];

/// The echo delimiter the cursor is on, if any.
///
/// Shared by [`Backend::blade_echo_delimiter_hover`] and
/// [`Backend::blade_echo_delimiter_definition`] so the two features agree on
/// exactly which cursor positions count as "on the delimiter". Reads
/// [`mode_at`] rather than peeking at surrounding characters, so a delimiter
/// inside a `{{-- --}}` comment, a `@verbatim` block, or an `@`-escaped
/// `@{{ … }}` or `@{!! … !!}` — none of which compile to an echo — is
/// correctly excluded.
fn blade_echo_delimiter_at(content: &str, offset: usize) -> Option<EchoDelimiter> {
    // The cursor is on a delimiter when it is on any one of its characters,
    // so one starts at the cursor or up to two characters before it, and
    // still covers the cursor.
    (0..3)
        .filter_map(|back| offset.checked_sub(back))
        .find_map(|start| {
            let rest = content.get(start..)?;
            // `{{--` opens a comment, not an echo, and in `{{!!` the raw
            // echo opens at the second `{`, so the first is a literal brace.
            if rest.starts_with("{{--") || rest.starts_with("{{!!") {
                return None;
            }
            let (text, raw, _) = DELIMITERS.iter().find(|(text, _, mode)| {
                offset < start + text.len()
                    && rest.starts_with(text)
                    && mode_at(content, start) == *mode
            })?;
            Some(EchoDelimiter {
                start,
                len: text.len() as u32,
                raw: *raw,
            })
        })
}

impl crate::Backend {
    /// If the cursor is on a Blade echo delimiter, return a hover describing
    /// what it compiles to: the implicit `e()` call of `{{`/`}}`, or the
    /// unescaped output of `{!!`/`!!}`.
    pub(crate) fn blade_echo_delimiter_hover(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<Hover> {
        let content = self.get_file_content(uri)?;
        let offset = position_to_byte_offset(&content, position);
        let delimiter = blade_echo_delimiter_at(&content, offset)?;
        let start = offset_to_position(&content, delimiter.start);
        Some(Hover {
            contents: if delimiter.raw {
                blade_raw_echo_contents()
            } else {
                self.blade_escaped_echo_contents()
            },
            range: Some(Range {
                start,
                end: Position {
                    line: start.line,
                    character: start.character + delimiter.len,
                },
            }),
        })
    }

    /// If the cursor is on a Blade echo delimiter, return the
    /// go-to-definition target for what it compiles to, so it agrees with
    /// [`Self::blade_echo_delimiter_hover`] on the same position instead of
    /// falling through to whatever PHP expression the blade-to-PHP offset
    /// mapping happens to land on.
    ///
    /// A `{{`/`}}` leads to the implicit `e()` call. A `{!!`/`!!}` compiles
    /// to a bare `echo`, which has no declaration to lead to.
    ///
    /// Returns `Some(None)` (suppressing go-to-definition, rather than
    /// disagreeing with the hover) when the cursor is on the delimiter but
    /// there is nothing navigable behind it: a raw echo, or an `e()` that
    /// only resolved from an embedded stub. Returns `None` when the cursor is
    /// not on a delimiter at all, so the caller can fall through to ordinary
    /// go-to-definition.
    pub(crate) fn blade_echo_delimiter_definition(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<Option<Location>> {
        let content = self.get_file_content(uri)?;
        let offset = position_to_byte_offset(&content, position);
        let delimiter = blade_echo_delimiter_at(&content, offset)?;
        Some(if delimiter.raw {
            None
        } else {
            self.resolve_function_definition(&["e".to_string()])
        })
    }

    /// Build hover content for `{{ }}` (escaped echo via `e()`).
    fn blade_escaped_echo_contents(&self) -> HoverContents {
        // Try to resolve the actual `e()` function from the project/stubs.
        let empty_use_map = std::collections::HashMap::new();
        let loader = self.function_loader_with(None, &empty_use_map, &None);
        if let Some(func) = loader("e", 0) {
            crate::hover::hover_for_function(&func, None, None, false).contents
        } else {
            HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "Blade escaped echo. Output is passed through `e()` (`htmlspecialchars`).\n\n\
                    ```php\n<?php\nfunction e(mixed $value, bool $doubleEncode = true): string;\n```"
                    .to_string(),
            })
        }
    }
}

/// Build hover content for `{!! !!}` (raw echo, with no `e()` around it).
fn blade_raw_echo_contents() -> HoverContents {
    HoverContents::Markup(MarkupContent {
        kind: MarkupKind::Markdown,
        value: "Blade raw echo. Output is not escaped: unlike `{{ }}`, it is not passed through \
            `e()` (`htmlspecialchars`)."
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(start, len, raw)` of the delimiter a cursor on byte `offset` of
    /// `content` is on.
    fn delimiter_at(content: &str, offset: usize) -> Option<(usize, u32, bool)> {
        blade_echo_delimiter_at(content, offset).map(|d| (d.start, d.len, d.raw))
    }

    /// The cursor is on a delimiter when it is on any one of its
    /// characters, whichever echo form the delimiter belongs to.
    #[test]
    fn every_character_of_a_delimiter_is_on_it() {
        for (content, delimiter, start, raw) in [
            ("{{ $a }}", "{{", 0, false),
            ("{{ $a }}", "}}", 6, false),
            ("{{$a}}", "{{", 0, false),
            ("{{$a}}", "}}", 4, false),
            ("{!! $a !!}", "{!!", 0, true),
            ("{!! $a !!}", "!!}", 7, true),
            ("{!!$a!!}", "{!!", 0, true),
            ("{!!$a!!}", "!!}", 5, true),
            ("<p>{!!$a!!}</p>", "{!!", 3, true),
            ("<p>{!!$a!!}</p>", "!!}", 8, true),
        ] {
            assert_eq!(&content[start..start + delimiter.len()], delimiter);
            for offset in start..start + delimiter.len() {
                assert_eq!(
                    delimiter_at(content, offset),
                    Some((start, delimiter.len() as u32, raw)),
                    "offset {offset} of {content:?} is on {delimiter:?}"
                );
            }
        }
    }

    /// The expression between the delimiters, and the text on either side
    /// of the echo, are not on a delimiter.
    #[test]
    fn the_text_around_a_delimiter_is_not_on_it() {
        for (content, offsets) in [
            ("<p>{{ $a }}</p>", vec![0, 1, 2, 5, 6, 7, 8, 11, 12, 13, 14]),
            ("<p>{!!$a!!}</p>", vec![0, 1, 2, 6, 7, 11, 12, 13, 14]),
            (
                "<p>{!! $a !!}</p>",
                vec![0, 1, 2, 6, 7, 8, 9, 13, 14, 15, 16],
            ),
        ] {
            for offset in offsets {
                assert_eq!(
                    delimiter_at(content, offset),
                    None,
                    "offset {offset} of {content:?} is not on a delimiter"
                );
            }
        }
    }

    /// Two echoes side by side: the cursor is on whichever delimiter it
    /// is on, not on the one before it.
    #[test]
    fn adjacent_echoes_keep_their_own_delimiters() {
        let content = "{!!$a!!}{!!$b!!}{{$c}}{{$d}}";
        for (offset, expected) in [
            (7, (5, 3, true)),
            (8, (8, 3, true)),
            (15, (13, 3, true)),
            (16, (16, 2, false)),
            (21, (20, 2, false)),
            (22, (22, 2, false)),
        ] {
            assert_eq!(
                delimiter_at(content, offset),
                Some(expected),
                "offset {offset} of {content:?}"
            );
        }
    }

    /// A lookalike that does not compile to an echo is not a delimiter: the
    /// text of a `{{-- --}}` comment, a `@verbatim` block, an `@`-escaped
    /// echo of either form, and a PHP block or tag is not Blade's to
    /// compile.
    #[test]
    fn a_lookalike_outside_a_real_echo_is_not_a_delimiter() {
        for (content, lookalike) in [
            ("{{-- {{ $a }} --}}", "{{ $a"),
            ("{{-- {!! $a !!} --}}", "{!! $a"),
            ("{{-- {!! $a !!} --}}", "!!} --"),
            ("@verbatim {!! $a !!} @endverbatim", "{!!"),
            ("@verbatim {!! $a !!} @endverbatim", "!!}"),
            ("@{!! $a !!}", "{!!"),
            ("@{!! $a !!}", "!!}"),
            ("@{{ $a }}", "{{"),
            ("@php echo '{!! $a !!}'; @endphp", "{!!"),
            ("<?php echo '{!! $a !!}'; ?>", "!!}"),
        ] {
            let at = content
                .find(lookalike)
                .expect("the lookalike is in the content");
            let len = lookalike.chars().take_while(|c| "{}!".contains(*c)).count();
            for offset in at..at + len {
                assert_eq!(
                    delimiter_at(content, offset),
                    None,
                    "offset {offset} of {content:?} is inside {lookalike:?}, which is no echo"
                );
            }
        }
    }

    /// Blade matches echo tags longest-opening-first, so `{{!!$a!!}}` is a
    /// literal `{`, a raw echo, and a literal `}`. The raw echo keeps its
    /// own delimiters and the braces around it are not delimiters at all,
    /// whether or not an `@` sits in front of them, which escapes only an
    /// echo it comes directly before.
    #[test]
    fn a_raw_echo_inside_literal_braces_keeps_its_own_delimiters() {
        // Every delimiter of the template, as `(start, len, raw)`.
        for (content, delimiters) in [
            ("{{!!$a!!}}", vec![(1, 3, true), (6, 3, true)]),
            ("{{!! $a !!}}", vec![(1, 3, true), (8, 3, true)]),
            ("<p>{{!!$a!!}}</p>", vec![(4, 3, true), (9, 3, true)]),
            ("@{{!!$a!!}}", vec![(2, 3, true), (7, 3, true)]),
            (
                "{{$a}}{{!!$b!!}}{{$c}}",
                vec![
                    (0, 2, false),
                    (4, 2, false),
                    (7, 3, true),
                    (12, 3, true),
                    (16, 2, false),
                    (20, 2, false),
                ],
            ),
        ] {
            for offset in 0..=content.len() {
                let expected = delimiters
                    .iter()
                    .find(|(start, len, _)| (*start..start + *len as usize).contains(&offset))
                    .copied();
                assert_eq!(
                    delimiter_at(content, offset),
                    expected,
                    "offset {offset} of {content:?}"
                );
            }
        }
    }

    /// Neither end of a comment is an echo delimiter, though both are
    /// written with braces.
    #[test]
    fn the_ends_of_a_comment_are_not_delimiters() {
        let comment = "{{-- note --}}";
        for offset in 0..comment.len() {
            assert_eq!(delimiter_at(comment, offset), None, "offset {offset}");
        }
    }

    /// An echo ends only at its own terminator: a `}}` inside a raw echo
    /// and a `!!}` inside an escaped one are part of its expression.
    #[test]
    fn a_terminator_of_the_other_form_is_not_a_delimiter() {
        let raw = "{!! $a }} !!}";
        let escaped = "{{ $a !!} }}";
        for offset in [7, 8] {
            assert_eq!(delimiter_at(raw, offset), None);
        }
        assert_eq!(delimiter_at(raw, 10), Some((10, 3, true)));
        for offset in [6, 7, 8] {
            assert_eq!(delimiter_at(escaped, offset), None);
        }
        assert_eq!(delimiter_at(escaped, 10), Some((10, 2, false)));
    }

    /// Multi-byte text before a delimiter and inside the echo shifts the
    /// byte offsets, never the delimiter's own.
    #[test]
    fn multibyte_text_does_not_confuse_the_offsets() {
        let content = "æøå {!!$ø!!}";
        let opener = content.find("{!!").unwrap();
        assert_eq!(delimiter_at(content, opener + 1), Some((opener, 3, true)));
        let closer = content.find("!!}").unwrap();
        assert_eq!(delimiter_at(content, closer + 2), Some((closer, 3, true)));
        // The `ø` just before the closer.
        assert_eq!(delimiter_at(content, closer - 2), None);
    }
}
