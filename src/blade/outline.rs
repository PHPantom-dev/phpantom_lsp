//! The structure a Blade template shows in an editor's outline.
//!
//! A template's landmarks are not PHP declarations. What an author
//! navigates by is the sections and stacks it fills or leaves open, and
//! the components it renders — none of which the virtual PHP the
//! preprocessor emits carries in a shape the PHP outline could read: a
//! section name is a string argument to a marker call, and a component
//! tag is HTML that never reaches the virtual PHP as a name at all.
//!
//! This reads the raw Blade source instead, the way [`super::balance`]
//! and [`super::component_tags`] do, so every span it reports is already
//! in the template's own coordinates.

use tower_lsp::lsp_types::SymbolKind;

use crate::Backend;

use super::balance::{Span, block_pairs};
use super::blocks::{BlockRole, analyse};
use super::component_tags::{TagKind, tag_spans};

/// One entry of a template's outline, in raw Blade byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutlineEntry {
    /// What the outline lists it as: a section or stack name, or a
    /// component tag as it is written.
    pub(crate) name: String,
    /// The greyed-out annotation beside the name: the directive a name
    /// was written with, or the class (or template) a component tag
    /// resolves to.
    pub(crate) detail: Option<String>,
    pub(crate) kind: SymbolKind,
    /// The whole region the entry stands for, so entries written inside
    /// it nest under it.
    pub(crate) span: Span,
    /// The name's own bytes, which is where selecting the entry puts the
    /// cursor.
    pub(crate) selection: Span,
}

impl Backend {
    /// Every landmark in a Blade template, in document order.
    ///
    /// Sections and stacks come out as regions spanning their whole
    /// block, so the components and directives written inside one nest
    /// under it once the caller builds the tree.
    pub(crate) fn blade_outline(&self, content: &str) -> Vec<OutlineEntry> {
        let mut entries = named_block_entries(content);
        entries.extend(self.component_tag_entries(content));
        entries.sort_by(|a, b| {
            a.span
                .start
                .cmp(&b.span.start)
                .then(b.span.end.cmp(&a.span.end))
        });
        entries
    }

    /// The component tags a template renders, annotated with what each
    /// one resolves to: the class behind it, or, for an anonymous
    /// component, the template Laravel renders in its place.
    ///
    /// A tag nothing answers for keeps its bare name and no annotation —
    /// an outline is not the place to report that a component is
    /// missing, and the tag is still where the author wrote it.
    fn component_tag_entries(&self, content: &str) -> Vec<OutlineEntry> {
        let tags = tag_spans(content);
        if tags.is_empty() {
            return Vec::new();
        }
        let anonymous = self.anonymous_component_namespaces();
        tags.into_iter()
            .map(|tag| {
                let detail = match tag.kind {
                    TagKind::Livewire => self.livewire_component_fqn(&tag.name),
                    TagKind::Blade => self
                        .blade_component_fqn(&tag.name)
                        .or_else(|| self.anonymous_component_view(&tag.name, &anonymous)),
                };
                OutlineEntry {
                    name: format!("{}{}", &tag.kind.opening()[1..], tag.name),
                    detail,
                    kind: SymbolKind::CLASS,
                    span: tag.span,
                    selection: tag.name_span,
                }
            })
            .collect()
    }
}

/// The sections and stacks a template writes, each spanning the block it
/// opens where it opens one.
///
/// `@hasSection` and its cousins are left out: they ask about a name
/// rather than standing for a region of the template, and the block they
/// open is a condition whose contents belong to whatever encloses it.
fn named_block_entries(content: &str) -> Vec<OutlineEntry> {
    let blocks = analyse(content).blocks;
    if blocks.is_empty() {
        return Vec::new();
    }
    let pairs = block_pairs(content);
    blocks
        .into_iter()
        .filter(|block| block.role != BlockRole::Check)
        .map(|block| {
            // The pair whose argument list holds this name is the block
            // this directive opened; a directive that opens none (a
            // `@yield`, a `@stack`, a two-argument `@section`) has no
            // body, and stands for the directive and its name alone.
            let span = pairs
                .iter()
                .find(|pair| {
                    pair.args
                        .as_ref()
                        .is_some_and(|args| args.contains(&block.name_span.start))
                })
                .map(|pair| pair.opener.start..pair.closer.end)
                .unwrap_or(block.directive_span.start..block.name_span.end + 1);
            OutlineEntry {
                name: block.name,
                detail: Some(content[block.directive_span].to_string()),
                kind: SymbolKind::NAMESPACE,
                span,
                selection: block.name_span,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The outline as `(name, detail, start, end)`, for readable
    /// assertions. Component tags resolve to nothing without a project
    /// behind them, which the integration tests cover instead.
    fn outline(content: &str) -> Vec<(String, Option<String>, usize, usize)> {
        let backend = Backend::new_test();
        backend
            .blade_outline(content)
            .into_iter()
            .map(|entry| (entry.name, entry.detail, entry.span.start, entry.span.end))
            .collect()
    }

    #[test]
    fn a_section_spans_its_whole_block() {
        let blade = "@section('content')\n<p>hi</p>\n@endsection\n";
        assert_eq!(
            outline(blade),
            [(
                "content".to_string(),
                Some("@section".to_string()),
                0,
                blade.len() - 1
            )]
        );
    }

    /// `@show` publishes the section as well as closing it, so the block
    /// it ends is still the section's own.
    #[test]
    fn a_section_closed_by_show_spans_its_whole_block() {
        let blade = "@section('sidebar')\n<p>hi</p>\n@show\n";
        assert_eq!(outline(blade)[0].3, blade.len() - 1);
    }

    #[test]
    fn a_yield_stands_for_itself() {
        let blade = "<div>@yield('content')</div>\n";
        let found = outline(blade);
        assert_eq!(found[0].0, "content");
        assert_eq!(found[0].1, Some("@yield".to_string()));
        assert_eq!(&blade[found[0].2..found[0].3], "@yield('content'");
    }

    /// The two-argument form supplies its content on the spot, so there
    /// is no block for it to span.
    #[test]
    fn an_inline_section_stands_for_itself() {
        let blade = "@section('title', 'Home')\n@section('body')\nhi\n@endsection\n";
        let found = outline(blade);
        assert_eq!(found.len(), 2);
        assert!(found[0].3 < found[1].2, "the inline section ends first");
    }

    #[test]
    fn a_push_and_a_stack_are_both_listed() {
        let blade = "@stack('scripts')\n@push('scripts')\n<script></script>\n@endpush\n";
        let names: Vec<String> = outline(blade).into_iter().map(|entry| entry.0).collect();
        assert_eq!(names, ["scripts", "scripts"]);
    }

    /// A condition on a section name is not a region of the template.
    #[test]
    fn a_has_section_check_is_not_listed() {
        assert!(outline("@hasSection('nav')\n<p>hi</p>\n@endif\n").is_empty());
    }

    /// Blade never reads a directive written in a comment, and neither
    /// does the outline.
    #[test]
    fn a_section_in_a_comment_is_not_listed() {
        assert!(outline("{{-- @section('content') --}}\n").is_empty());
    }

    #[test]
    fn a_component_tag_is_listed_by_the_tag_as_written() {
        let blade = "<x-alert>\n<p>hi</p>\n</x-alert>\n";
        assert_eq!(
            outline(blade),
            [("x-alert".to_string(), None, 0, blade.len() - 1)]
        );
    }

    #[test]
    fn a_livewire_tag_is_listed_with_its_prefix() {
        let found = outline("<livewire:counter />\n");
        assert_eq!(found[0].0, "livewire:counter");
    }

    /// Everything is ordered by where it starts, with the enclosing
    /// entry ahead of what it encloses, so the caller can nest by
    /// containment in one pass.
    #[test]
    fn entries_come_back_outermost_first() {
        let blade = "@section('content')\n<x-alert />\n@endsection\n<x-note />\n";
        let names: Vec<String> = outline(blade).into_iter().map(|entry| entry.0).collect();
        assert_eq!(names, ["content", "x-alert", "x-note"]);
    }
}
