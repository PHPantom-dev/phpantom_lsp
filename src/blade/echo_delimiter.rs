//! Hover and go-to-definition for the `{{ … }}` echo delimiters, which
//! compile to an implicit `e()` call the template never spells out.

use tower_lsp::lsp_types::{
    Hover, HoverContents, Location, MarkupContent, MarkupKind, Position, Range,
};

/// Column of the `{{`/`}}` escaped-echo delimiter the cursor is on, if any.
///
/// Shared by [`Backend::blade_echo_delimiter_hover`] and
/// [`Backend::blade_echo_delimiter_definition`] so the two features agree on
/// exactly which cursor positions count as "on the delimiter".
fn blade_echo_delimiter_col(line: &str, col: usize) -> Option<usize> {
    // Check if cursor is on `{{` (escaped echo open)
    if col < line.len()
        && line.get(col..col + 2) == Some("{{")
        && line.get(col..col + 3) != Some("{!!")
    {
        return Some(col);
    }
    // Also match if cursor is on the second `{` of `{{`
    if col > 0
        && line.get(col - 1..col + 1) == Some("{{")
        && (col < 2 || line.get(col - 1..col + 2) != Some("{!!"))
    {
        return Some(col - 1);
    }
    // `}}` closing delimiter
    if col < line.len()
        && line.get(col..col + 2) == Some("}}")
        && (col == 0 || line.as_bytes().get(col - 1) != Some(&b'!'))
    {
        return Some(col);
    }
    if col > 0
        && line.get(col - 1..col + 1) == Some("}}")
        && (col < 2 || line.as_bytes().get(col - 2) != Some(&b'!'))
    {
        return Some(col - 1);
    }

    None
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
        let line = content.lines().nth(position.line as usize)?;
        let start_col = blade_echo_delimiter_col(line, position.character as usize)?;
        Some(self.blade_e_hover(
            Position {
                line: position.line,
                character: start_col as u32,
            },
            2,
        ))
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
        let line = content.lines().nth(position.line as usize)?;
        blade_echo_delimiter_col(line, position.character as usize)?;
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
