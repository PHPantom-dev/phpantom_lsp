//! Hover and go-to-definition for the `{{ … }}` echo delimiters, which
//! compile to an implicit `e()` call the template never spells out.

use tower_lsp::lsp_types::{
    Hover, HoverContents, Location, MarkupContent, MarkupKind, Position, Range,
};

use super::directive_completion::{Mode, mode_at};
use crate::text_position::{offset_to_position, position_to_byte_offset};

/// Byte offset of the `{{`/`}}` escaped-echo delimiter the cursor is on, if
/// any.
///
/// Shared by [`Backend::blade_echo_delimiter_hover`] and
/// [`Backend::blade_echo_delimiter_definition`] so the two features agree on
/// exactly which cursor positions count as "on the delimiter". Reads
/// [`mode_at`] rather than peeking at surrounding characters, so a `{{`/`}}`
/// inside a `{{-- --}}` comment, a `@verbatim` block, or an `@`-escaped
/// `@{{ … }}` — none of which compile to an echo — is correctly excluded.
fn blade_echo_delimiter_offset(content: &str, offset: usize) -> Option<usize> {
    let candidates = if offset > 0 {
        [Some(offset), Some(offset - 1)]
    } else {
        [Some(offset), None]
    };

    candidates.into_iter().flatten().find(|&start| {
        (content.get(start..start + 2) == Some("{{")
            && content.get(start..start + 3) != Some("{!!")
            && content.get(start..start + 4) != Some("{{--")
            && mode_at(content, start) == Mode::Html)
            || (content.get(start..start + 2) == Some("}}")
                && mode_at(content, start) == Mode::UntilMarkerInCode("}}"))
    })
}

impl crate::Backend {
    /// If the cursor is on a `{{` or `}}` Blade echo delimiter, return a
    /// hover describing the implicit `e()` call the delimiter compiles to.
    pub(crate) fn blade_echo_delimiter_hover(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<Hover> {
        let content = self.get_file_content(uri)?;
        let offset = position_to_byte_offset(&content, position);
        let start_offset = blade_echo_delimiter_offset(&content, offset)?;
        Some(self.blade_e_hover(offset_to_position(&content, start_offset), 2))
    }

    /// If the cursor is on a `{{` or `}}` Blade echo delimiter, return the
    /// go-to-definition target for the implicit `e()` call, so it agrees
    /// with [`Self::blade_echo_delimiter_hover`] on the same position
    /// instead of falling through to whatever PHP expression the
    /// blade-to-PHP offset mapping happens to land on.
    ///
    /// Returns `Some(None)` (suppressing go-to-definition, rather than
    /// disagreeing with the hover) when the cursor is on the delimiter but
    /// `e()` itself has no navigable declaration (e.g. it only resolved
    /// from an embedded stub). Returns `None` when the cursor is not on the
    /// delimiter at all, so the caller can fall through to ordinary
    /// go-to-definition.
    pub(crate) fn blade_echo_delimiter_definition(
        &self,
        uri: &str,
        position: Position,
    ) -> Option<Option<Location>> {
        let content = self.get_file_content(uri)?;
        let offset = position_to_byte_offset(&content, position);
        blade_echo_delimiter_offset(&content, offset)?;
        Some(self.resolve_function_definition(&["e".to_string()]))
    }

    /// Build hover content for `{{ }}` (escaped echo via `e()`).
    fn blade_e_hover(&self, start: Position, len: u32) -> Hover {
        // Try to resolve the actual `e()` function from the project/stubs.
        let empty_use_map = std::collections::HashMap::new();
        let loader = self.function_loader_with(None, &empty_use_map, &None);
        let content = if let Some(func) = loader("e", 0) {
            crate::hover::hover_for_function(&func, None, None, false).contents
        } else {
            HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "Blade escaped echo. Output is passed through `e()` (`htmlspecialchars`).\n\n\
                    ```php\n<?php\nfunction e(mixed $value, bool $doubleEncode = true): string;\n```"
                    .to_string(),
            })
        };
        Hover {
            contents: content,
            range: Some(Range {
                start,
                end: Position {
                    line: start.line,
                    character: start.character + len,
                },
            }),
        }
    }
}
