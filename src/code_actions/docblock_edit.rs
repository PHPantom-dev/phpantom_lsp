//! Shared docblock-location helpers for code actions.
//!
//! Several PHPStan quickfixes need to locate the `/** … */` block that sits
//! directly above a class, function, method, or property signature so they
//! can rewrite or remove tags inside it.  This module owns what they share:
//! [`DocblockAbove`], the block as the byte span of its lines with edits
//! that rewrite it or drop lines from it; [`FunctionDocblock`], the block of
//! a function or method found through the parsed file (or where a new one
//! goes), with edits that set its `@return` and add a `@throws`; and
//! [`ClassDocblock`], the same for a class declaration, with an edit that
//! adds a tag.

use std::borrow::Borrow;

use mago_span::HasSpan;
use mago_syntax::cst::{Class, Program};
use tower_lsp::lsp_types::TextEdit;

use crate::code_actions::cursor_context::{FunctionKinds, find_enclosing_function};
use crate::docblock::strip_docblock_line_delimiters;
use crate::docblock::type_strings::split_type_token;
use crate::php_type::PhpType;
use crate::text_position::{
    byte_range_to_lsp_range, line_ending, line_start_byte_offset, lines_with_offsets,
};

/// Information about a docblock found above a given line.
pub(crate) struct DocblockAbove {
    /// Byte offset of the start of the docblock (first char of the `/**`
    /// line, including leading whitespace).
    pub(crate) start: usize,
    /// Byte offset just past the end of the docblock (past the `*/` line,
    /// including its trailing newline).
    pub(crate) end: usize,
    /// The raw docblock text including indentation.
    pub(crate) text: String,
}

impl DocblockAbove {
    /// The block whose `/**` starts at `open` and whose `*/` ends at
    /// `close`, spanning its whole lines, when nothing but indentation
    /// shares those lines with it.
    fn on_own_lines(content: &str, open: usize, close: usize) -> Option<Self> {
        let start = content[..open].rfind('\n').map_or(0, |nl| nl + 1);
        let end = content[close..]
            .find('\n')
            .map_or(content.len(), |nl| close + nl + 1);
        let alone = content[start..open].trim().is_empty() && content[close..end].trim().is_empty();
        alone.then(|| Self {
            start,
            end,
            text: content[start..end].to_string(),
        })
    }

    /// Each line of the block, without its terminator, and the byte offset
    /// in the file it starts at.
    fn lines(&self) -> impl Iterator<Item = (usize, &str)> {
        lines_with_offsets(&self.text).map(|(at, line)| (self.start + at, line))
    }

    /// A `TextEdit` that replaces this docblock with `lines`, joined by the
    /// file's line terminator, keeping its trailing newline.
    pub(crate) fn rewrite_edit<S: Borrow<str>>(&self, content: &str, lines: &[S]) -> TextEdit {
        let eol = line_ending(content);
        let mut new_text = lines.join(eol);
        if self.text.ends_with('\n') && !new_text.ends_with('\n') {
            new_text.push_str(eol);
        }
        self.replace_edit(content, new_text)
    }

    /// A `TextEdit` that rewrites this docblock without the lines at
    /// `lines_to_remove` (indices into its lines).
    ///
    /// A docblock left with nothing but its delimiters and blank `*` lines
    /// is removed entirely, along with its trailing newline.
    pub(crate) fn remove_lines_edit(&self, content: &str, lines_to_remove: &[usize]) -> TextEdit {
        let kept: Vec<&str> = self
            .text
            .lines()
            .enumerate()
            .filter(|(i, _)| !lines_to_remove.contains(i))
            .map(|(_, line)| line)
            .collect();
        let has_content = kept
            .iter()
            .any(|line| !strip_docblock_line_delimiters(line.trim()).is_empty());
        if !has_content && kept.len() <= 3 {
            return self.replace_edit(content, String::new());
        }
        self.rewrite_edit(content, &kept)
    }

    fn replace_edit(&self, content: &str, new_text: String) -> TextEdit {
        TextEdit {
            range: byte_range_to_lsp_range(content, self.start, self.end),
            new_text,
        }
    }
}

/// Find the docblock immediately above the given line.
///
/// The diagnostic line is the function/method/property signature.  The
/// docblock (if any) sits directly above it, possibly separated by blank
/// lines or attribute (`#[…]`) lines.
/// A new docblock at `indent` holding `lines`, each written as ` * line`
/// (a bare ` *` for an empty one), every line ending with `eol`.
pub(crate) fn render_docblock<S: AsRef<str>>(indent: &str, lines: &[S], eol: &str) -> String {
    let mut doc = format!("{indent}/**{eol}");
    for line in lines {
        let line = line.as_ref();
        doc.push_str(indent);
        doc.push_str(if line.is_empty() { " *" } else { " * " });
        doc.push_str(line);
        doc.push_str(eol);
    }
    doc.push_str(indent);
    doc.push_str(" */");
    doc.push_str(eol);
    doc
}

pub(crate) fn find_docblock_above_line(content: &str, line: usize) -> Option<DocblockAbove> {
    let lines: Vec<&str> = content.lines().collect();
    if line == 0 || line > lines.len() {
        return None;
    }

    // Walk backward from the line before the diagnostic to find `*/`.
    let mut doc_end_line = None;
    for i in (0..line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.ends_with("*/") {
            doc_end_line = Some(i);
            break;
        }
        // Attributes (#[...]) can appear between docblock and declaration.
        if trimmed.starts_with("#[") {
            continue;
        }
        // Anything else means no docblock.
        break;
    }

    let end_line = doc_end_line?;

    // Walk backward from end_line to find `/**`.
    let mut doc_start_line = None;
    for i in (0..=end_line).rev() {
        let trimmed = lines[i].trim();
        if trimmed.contains("/**") {
            doc_start_line = Some(i);
            break;
        }
        // Should be a `*`-prefixed line or end-of-docblock.
        if !trimmed.starts_with('*') && !trimmed.ends_with("*/") {
            break;
        }
    }

    let start_line = doc_start_line?;

    // Convert line numbers to byte offsets.  Counting the bytes of each
    // line and adding one for its terminator would be a line short per
    // `\r\n`, so the offsets come from the line index instead.
    let start_byte = line_start_byte_offset(content, start_line);
    // The docblock ends where the line after its last one begins, so the
    // trailing newline is part of the span and deleting it takes the
    // whole line with it.
    let end_byte = line_start_byte_offset(content, end_line + 1).min(content.len());

    Some(DocblockAbove {
        start: start_byte,
        end: end_byte,
        text: content.get(start_byte..end_byte).unwrap_or("").to_string(),
    })
}

/// Byte offset of the start of the `/** … */` block sitting above the
/// declaration whose first line starts at `decl_line_start`, if there is one.
///
/// Works entirely in byte offsets, so the result is safe to feed straight
/// into a deletion edit regardless of line endings.  Blank lines between the
/// docblock and the declaration are tolerated and included in the returned
/// span, so deleting `[start, decl_line_end)` removes both.
pub(crate) fn docblock_start_above_offset(content: &str, decl_line_start: usize) -> Option<usize> {
    // Walk backwards over blank lines to the line that closes the docblock.
    let mut line_start = decl_line_start;
    let closing_line = loop {
        let prev = prev_line_start(content, line_start)?;
        let trimmed = content.get(prev..line_start)?.trim();
        if trimmed.ends_with("*/") {
            break prev;
        }
        if !trimmed.is_empty() {
            return None;
        }
        line_start = prev;
    };

    // Walk back over the docblock's own lines to its `/**` opener.
    let mut line_start = closing_line;
    loop {
        let line_end = content[line_start..]
            .find('\n')
            .map_or(content.len(), |i| line_start + i);
        let trimmed = content.get(line_start..line_end)?.trim();
        if trimmed.starts_with("/**") {
            return Some(line_start);
        }
        if !trimmed.starts_with('*') {
            return None;
        }
        line_start = prev_line_start(content, line_start)?;
    }
}

/// Start offset of the line preceding the one starting at `line_start`,
/// or `None` when `line_start` is already the start of the file.
fn prev_line_start(content: &str, line_start: usize) -> Option<usize> {
    if line_start == 0 {
        return None;
    }
    let bytes = content.as_bytes();
    // `line_start - 1` is the newline that terminates the previous line.
    let mut pos = line_start - 1;
    while pos > 0 && bytes[pos - 1] != b'\n' {
        pos -= 1;
    }
    Some(pos)
}

/// Where a new docblock goes above a declaration: the start of its first
/// line, at that line's indentation.
struct DocblockPlacement {
    /// Byte offset of the start of the declaration's first line (its first
    /// attribute, modifier or keyword).
    insert_at: usize,
    /// Indentation of the declaration's first line.
    indent: String,
}

impl DocblockPlacement {
    /// The placement above the declaration whose first token starts at
    /// `decl_start`.
    fn above(content: &str, decl_start: usize) -> Self {
        let insert_at = content[..decl_start].rfind('\n').map_or(0, |nl| nl + 1);
        let indent = content[insert_at..decl_start]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        Self { insert_at, indent }
    }

    /// The edit that adds the tag line `tag` to `docblock`: before its
    /// closing line, after a blank `*` line when the line before it holds a
    /// tag other than those in `joins`; spreading a one-line docblock over
    /// several; or as a new docblock above the declaration when there is
    /// none.
    fn add_tag_edit(
        &self,
        content: &str,
        docblock: Option<&DocblockAbove>,
        tag: &str,
        joins: &[&str],
    ) -> TextEdit {
        let indent = &self.indent;
        let eol = line_ending(content);
        let Some(docblock) = docblock else {
            return insert_edit(
                content,
                self.insert_at,
                render_docblock(indent, &[tag], eol),
            );
        };

        let lines: Vec<(usize, &str)> = docblock.lines().collect();
        let (close_start, close_line) = lines[lines.len() - 1];
        if lines.len() == 1 {
            let inner = close_line
                .trim()
                .strip_prefix("/**")
                .and_then(|s| s.strip_suffix("*/"))
                .map(str::trim)
                .unwrap_or("");
            let lines: &[&str] = if inner.is_empty() {
                &[tag]
            } else {
                &[inner, "", tag]
            };
            // The rewritten docblock replaces the closing line, whose
            // line break stays.
            let mut new_doc = render_docblock(indent, lines, eol);
            new_doc.truncate(new_doc.len() - eol.len());
            return TextEdit {
                range: byte_range_to_lsp_range(
                    content,
                    close_start,
                    close_start + close_line.len(),
                ),
                new_text: new_doc,
            };
        }

        let previous = lines[lines.len() - 2]
            .1
            .trim()
            .trim_start_matches('*')
            .trim();
        let needs_separator =
            previous.starts_with('@') && !joins.iter().any(|join| previous.starts_with(join));
        let mut insert_text = String::new();
        if needs_separator {
            insert_text.push_str(&format!("{indent} *{eol}"));
        }
        insert_text.push_str(&format!("{indent} * {tag}{eol}"));
        insert_edit(content, close_start, insert_text)
    }
}

/// The `/** … */` block the parser attaches to `decl`: the one preceding its
/// first attribute, modifier or keyword with nothing but whitespace and
/// comments in between.
///
/// A block that shares a line with code is not one an edit can rewrite
/// line by line, so the declaration counts as having none.
fn docblock_of(program: &Program<'_>, content: &str, decl: &impl HasSpan) -> Option<DocblockAbove> {
    crate::symbol_map::docblock::get_docblock_text_with_offset(
        program.trivia.as_slice(),
        content,
        decl,
    )
    .and_then(|(text, open)| {
        DocblockAbove::on_own_lines(content, open as usize, open as usize + text.len())
    })
}

/// The docblock of a class declaration, or where one goes when it has none.
pub(crate) struct ClassDocblock {
    /// The block above the declaration, when it has one.
    pub(crate) docblock: Option<DocblockAbove>,
    placement: DocblockPlacement,
}

impl ClassDocblock {
    /// The docblock of `class`, read from the `program` parsed from
    /// `content`.
    pub(crate) fn of(program: &Program<'_>, content: &str, class: &Class<'_>) -> Self {
        Self {
            docblock: docblock_of(program, content, class),
            placement: DocblockPlacement::above(content, class.span().start.offset as usize),
        }
    }

    /// The edit that adds the tag line `tag`, set apart from a tag that
    /// closes the docblock.
    pub(crate) fn add_tag_edit(&self, content: &str, tag: &str) -> TextEdit {
        self.placement
            .add_tag_edit(content, self.docblock.as_ref(), tag, &[])
    }
}

/// The docblock of a function or method, or where one goes when it has
/// none.
pub(crate) struct FunctionDocblock {
    /// The block above the declaration, when it has one.
    pub(crate) docblock: Option<DocblockAbove>,
    placement: DocblockPlacement,
}

impl FunctionDocblock {
    /// The docblock of the function-like declaration spanning `decl`, read
    /// from the `program` parsed from `content`.
    pub(crate) fn of(program: &Program<'_>, content: &str, decl: mago_span::Span) -> Self {
        Self {
            docblock: docblock_of(program, content, &decl),
            placement: DocblockPlacement::above(content, decl.start.offset as usize),
        }
    }

    /// The line of the `@return` tag: the byte offset it starts at and its
    /// text.
    pub(crate) fn return_tag(&self) -> Option<(usize, &str)> {
        self.docblock
            .as_ref()?
            .lines()
            .find(|(_, line)| line.contains("@return"))
    }

    /// The type the `@return` tag gives, as written.
    pub(crate) fn return_type(&self) -> Option<PhpType> {
        let (_, line) = self.return_tag()?;
        let after_return = &line[line.find("@return")? + "@return".len()..];
        let (type_token, _) = split_type_token(after_return.trim_start());
        (!type_token.is_empty()).then(|| PhpType::parse(type_token))
    }

    /// The edit that rewrites the type of the `@return` tag to
    /// `return_type`, keeping the description after it.
    pub(crate) fn replace_return_type_edit(
        &self,
        content: &str,
        return_type: &str,
    ) -> Option<TextEdit> {
        let (line_start, line) = self.return_tag()?;
        let at_pos = line.find("@return")?;
        let after_return = &line[at_pos + "@return".len()..];
        let type_start = after_return
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(after_return.len());
        let (_, description) = split_type_token(&after_return[type_start..]);
        Some(TextEdit {
            range: byte_range_to_lsp_range(content, line_start, line_start + line.len()),
            new_text: format!("{}@return {}{}", &line[..at_pos], return_type, description),
        })
    }

    /// The edit that deletes the line of the `@return` tag.
    pub(crate) fn remove_return_tag_edit(&self, content: &str) -> Option<TextEdit> {
        let (line_start, line) = self.return_tag()?;
        let line_end = line_start + line.len();
        let next_line = content[line_end..]
            .find('\n')
            .map_or(content.len(), |nl| line_end + nl + 1);
        Some(TextEdit {
            range: byte_range_to_lsp_range(content, line_start, next_line),
            new_text: String::new(),
        })
    }

    /// The edit that adds a `@return return_type` tag, after any `@return`
    /// or `@throws` tag that closes the docblock and set apart from any
    /// other.
    pub(crate) fn add_return_tag_edit(&self, content: &str, return_type: &str) -> TextEdit {
        self.add_tag_edit(
            content,
            &format!("@return {return_type}"),
            &["@return", "@throws"],
        )
    }

    /// The edit that adds a `@throws name` tag, after any `@throws` tag that
    /// closes the docblock and set apart from any other.
    pub(crate) fn add_throws_tag_edit(&self, content: &str, name: &str) -> TextEdit {
        self.add_tag_edit(content, &format!("@throws {name}"), &["@throws"])
    }

    fn add_tag_edit(&self, content: &str, tag: &str, joins: &[&str]) -> TextEdit {
        self.placement
            .add_tag_edit(content, self.docblock.as_ref(), tag, joins)
    }
}

fn insert_edit(content: &str, offset: usize, new_text: String) -> TextEdit {
    TextEdit {
        range: byte_range_to_lsp_range(content, offset, offset),
        new_text,
    }
}

/// The docblock of the innermost function-like declaration of `kinds`
/// whose span contains `offset`, as [`docblock_of`] reads it.
fn function_docblock_at(
    content: &str,
    offset: usize,
    kinds: FunctionKinds,
) -> Option<FunctionDocblock> {
    crate::parser::with_parsed_program(content, "function_docblock_at", |program, content| {
        let decl = find_enclosing_function(program, offset as u32, kinds)?.span();
        Some(FunctionDocblock::of(program, content, decl))
    })
}

/// [`function_docblock_at`] for the first non-blank character of `line`,
/// for a diagnostic reported on a function's declaration or on a statement
/// in its body.
pub(crate) fn function_docblock_at_line(
    content: &str,
    line: usize,
    kinds: FunctionKinds,
) -> Option<FunctionDocblock> {
    let line_start = line_start_byte_offset(content, line);
    let text = content.get(line_start..)?.lines().next()?;
    let first = text.find(|c: char| !c.is_whitespace())?;
    function_docblock_at(content, line_start + first, kinds)
}
