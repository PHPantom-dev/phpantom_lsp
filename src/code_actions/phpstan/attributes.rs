//! The attributes written on the method or property a PHPStan diagnostic
//! points at, read from the AST: where a new attribute line goes, whether
//! an attribute is already there, and the edit that takes one away.
//!
//! Shared by the actions that add `#[\Override]` or
//! `#[\ReturnTypeWillChange]` and the one that removes `#[\Override]`.

use mago_span::HasSpan;
use mago_syntax::cst::attribute::AttributeList;
use mago_syntax::cst::sequence::Sequence;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::TextEdit;

use crate::atom::{bytes_to_str, last_segment};
use crate::code_actions::indent_of_line_at;
use crate::text_position::{byte_range_to_lsp_range, find_line_start, line_start_byte_offset};

/// One `#[…]` list on a member.
struct WrittenList {
    /// From `#[` to `]`.
    span: (usize, usize),
    /// Each attribute's name (its last segment, `Override` for
    /// `\Override`) and span, arguments included.
    attributes: Vec<(String, usize, usize)>,
}

/// A method or property and the attributes written on it.
pub(super) struct AttributedMember {
    /// The start of the line its declaration starts on: its first
    /// attribute, modifier or keyword.
    pub(super) line_start: usize,
    /// The indentation of that line.
    pub(super) indent: String,
    lists: Vec<WrittenList>,
}

impl AttributedMember {
    /// Whether an attribute named `name` is written on the member.  PHP
    /// reads class names case-insensitively, attribute names included.
    pub(super) fn has_attribute(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    fn find(&self, name: &str) -> Option<(&WrittenList, usize)> {
        self.lists.iter().find_map(|list| {
            let index = list
                .attributes
                .iter()
                .position(|(written, _, _)| written.eq_ignore_ascii_case(name))?;
            Some((list, index))
        })
    }

    /// The edit that takes the attribute `name` away.
    ///
    /// The only attribute in its list takes the whole `#[…]` with it, and
    /// the line too when nothing else is written on it.  An attribute
    /// beside others in a list goes with the comma that separates it from
    /// a neighbour.
    pub(super) fn remove_attribute_edit(&self, content: &str, name: &str) -> Option<TextEdit> {
        let (list, index) = self.find(name)?;
        let (start, end) = if list.attributes.len() == 1 {
            let (start, end) = list.span;
            let line_start = find_line_start(content, start);
            let line_end = content[end..]
                .find('\n')
                .map_or(content.len(), |at| end + at);
            let alone = content[line_start..start].trim().is_empty()
                && content[end..line_end].trim().is_empty();
            if alone {
                (line_start, (line_end + 1).min(content.len()))
            } else {
                // Whatever follows on the line keeps the list's place.
                let rest = &content[end..line_end];
                (start, end + (rest.len() - rest.trim_start().len()))
            }
        } else if let Some(&(_, next_start, _)) = list.attributes.get(index + 1) {
            (list.attributes[index].1, next_start)
        } else {
            (list.attributes[index - 1].2, list.attributes[index].2)
        };
        Some(TextEdit {
            range: byte_range_to_lsp_range(content, start, end),
            new_text: String::new(),
        })
    }
}

/// The method or property declared at `line`, which is where PHPStan
/// reports a member's own problems.
pub(super) fn attributed_member_at(content: &str, line: usize) -> Option<AttributedMember> {
    let line_start = line_start_byte_offset(content, line);
    let text = content.get(line_start..)?.lines().next()?;
    let offset = (line_start + text.len() - text.trim_start().len()) as u32;
    crate::parser::with_parsed_program(content, "phpstan_member_attributes", |program, content| {
        crate::code_actions::cursor_context::innermost_at(program, offset, |node| {
            let (span, attribute_lists) = match node {
                Node::Method(method) => (method.span(), &method.attribute_lists),
                Node::PlainProperty(property) => (property.span(), &property.attribute_lists),
                Node::HookedProperty(property) => (property.span(), &property.attribute_lists),
                _ => return None,
            };
            let start = span.start.offset as usize;
            let line_start = find_line_start(content, start);
            Some(AttributedMember {
                line_start,
                indent: indent_of_line_at(content, start).to_string(),
                lists: written_lists(attribute_lists),
            })
        })
    })
}

fn written_lists(attribute_lists: &Sequence<'_, AttributeList<'_>>) -> Vec<WrittenList> {
    attribute_lists
        .iter()
        .map(|list| {
            let span = list.span();
            WrittenList {
                span: (span.start.offset as usize, span.end.offset as usize),
                attributes: list
                    .attributes
                    .iter()
                    .map(|attribute| {
                        let span = attribute.span();
                        (
                            bytes_to_str(last_segment(attribute.name.value())).to_string(),
                            span.start.offset as usize,
                            span.end.offset as usize,
                        )
                    })
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn removed(content: &str, line: usize, name: &str) -> String {
        let member = attributed_member_at(content, line).expect("a member on the line");
        let edit = member
            .remove_attribute_edit(content, name)
            .expect("the attribute is there");
        crate::text_position::apply_text_edits(content, &[edit])
    }

    #[test]
    fn finds_the_member_from_any_line_of_its_declaration() {
        let content = "<?php\nclass Foo {\n    #[Route('/x')]\n    #[\\Override]\n    public function bar(): void {}\n}\n";
        for line in 2..=4 {
            let member = attributed_member_at(content, line).unwrap();
            assert_eq!(member.line_start, content.find("    #[Route").unwrap());
            assert_eq!(member.indent, "    ");
            assert!(member.has_attribute("Override"));
            assert!(member.has_attribute("override"));
            assert!(!member.has_attribute("Deprecated"));
        }
    }

    #[test]
    fn an_attribute_in_a_string_or_comment_is_not_written_on_the_member() {
        let content = "<?php\nclass Foo {\n    // #[\\Override]\n    public function bar(): string { return '#[Override]'; }\n}\n";
        let member = attributed_member_at(content, 3).unwrap();
        assert!(!member.has_attribute("Override"));
        assert_eq!(member.line_start, content.find("    public").unwrap());
    }

    #[test]
    fn removing_the_only_attribute_takes_its_line() {
        let content = "<?php\nclass Foo {\n    #[\\Override]\n    public $bar;\n}\n";
        assert_eq!(
            removed(content, 3, "Override"),
            "<?php\nclass Foo {\n    public $bar;\n}\n"
        );
    }

    #[test]
    fn removing_an_attribute_on_the_declaration_line_keeps_the_declaration() {
        let content = "<?php\nclass Foo {\n    #[Override] public function bar(): void {}\n}\n";
        assert_eq!(
            removed(content, 2, "Override"),
            "<?php\nclass Foo {\n    public function bar(): void {}\n}\n"
        );
    }

    #[test]
    fn removing_one_attribute_of_a_list_keeps_the_others() {
        let content = "<?php\nclass Foo {\n    #[Deprecated, \\Override, Pure]\n    public function bar(): void {}\n}\n";
        assert_eq!(
            removed(content, 3, "Override"),
            "<?php\nclass Foo {\n    #[Deprecated, Pure]\n    public function bar(): void {}\n}\n"
        );
        let content = "<?php\nclass Foo {\n    #[Deprecated, Override]\n    public function bar(): void {}\n}\n";
        assert_eq!(
            removed(content, 3, "Override"),
            "<?php\nclass Foo {\n    #[Deprecated]\n    public function bar(): void {}\n}\n"
        );
    }
}
