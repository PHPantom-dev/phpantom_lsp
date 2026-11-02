//! Unbalanced `<x-…>` / `<livewire:…>` component tags, and tags that name
//! no component at all.
//!
//! A component tag's body is a block the same way a directive's is:
//! `<x-alert>`…`</x-alert>` compiles independently of whatever closes it,
//! so a tag closed by the wrong name renders the rest of the template
//! inside it, and a stray closing tag renders nothing at all. Neither is
//! reported today.
//!
//! [`crate::blade::component_tags::tag_imbalances`] walks the tag stream
//! with a stack of open tags the same way [`crate::blade::balance`] walks
//! the directive stream; this turns what it finds into reports anchored
//! on the offending tag in the Blade source.
//!
//! A tag that pairs up fine can still name nothing (`<x-buttons.bakc />`),
//! which Laravel only finds out about when the template compiles.
//! [`crate::blade::unknown_components`] decides which names provably
//! resolve to no class and no template.

use tower_lsp_server::gen_lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::Backend;
use crate::blade::component_tags::{TagImbalance, TagKind, tag_imbalances, tag_spans};

use super::blade_imbalance::{BlockImbalance, report_block_imbalances};

/// A closing tag that closes something other than the tag it sits in.
const MISMATCHED_CODE: &str = "mismatched_blade_component_tag";
/// A closing tag with no open tag at all.
const UNEXPECTED_CODE: &str = "unexpected_blade_component_tag";
/// A tag the template never closes.
const UNCLOSED_CODE: &str = "unclosed_blade_component_tag";
/// An `<x-…>` tag no class or template answers to.
pub(crate) const UNKNOWN_BLADE_CODE: &str = "unknown_blade_component";
/// A `<livewire:…>` tag no component answers to.
pub(crate) const UNKNOWN_LIVEWIRE_CODE: &str = "unknown_livewire_component";

/// The code and message reported for a `kind` tag named `name` that names
/// nothing.
pub(crate) fn unknown_component_report(kind: TagKind, name: &str) -> (&'static str, String) {
    match kind {
        TagKind::Blade => (
            UNKNOWN_BLADE_CODE,
            format!("Unknown Blade component: '{name}'"),
        ),
        TagKind::Livewire => (
            UNKNOWN_LIVEWIRE_CODE,
            format!("Unknown Livewire component: '{name}'"),
        ),
    }
}

/// A tag's opening or closing spelling, for messages: `<x-alert>` /
/// `</x-alert>`, `<livewire:counter>` / `</livewire:counter>`.
fn tag_text(kind: TagKind, name: &str, closing: bool) -> String {
    format!(
        "<{slash}{prefix}{name}>",
        slash = if closing { "/" } else { "" },
        prefix = kind.prefix(),
    )
}

impl BlockImbalance for TagImbalance {
    fn span(&self) -> &std::ops::Range<usize> {
        TagImbalance::span(self)
    }

    fn code_and_message<F: Fn(&std::ops::Range<usize>) -> u32>(
        &self,
        line_of: F,
    ) -> (&'static str, String) {
        match self {
            TagImbalance::Mismatched {
                found_kind,
                found,
                opener_kind,
                opener,
                opener_span,
                ..
            } => {
                let line = line_of(opener_span);
                let opener_open = tag_text(*opener_kind, opener, false);
                let opener_close = tag_text(*opener_kind, opener, true);
                let found_close = tag_text(*found_kind, found, true);
                (
                    MISMATCHED_CODE,
                    format!(
                        "Expected {opener_close} to close the {opener_open} on line {line}, found {found_close}"
                    ),
                )
            }
            TagImbalance::Unexpected {
                found_kind, found, ..
            } => {
                let found_close = tag_text(*found_kind, found, true);
                let found_open = tag_text(*found_kind, found, false);
                (
                    UNEXPECTED_CODE,
                    format!("{found_close} closes nothing: no {found_open} is open here"),
                )
            }
            TagImbalance::Unclosed {
                opener_kind,
                opener,
                ..
            } => {
                let opener_open = tag_text(*opener_kind, opener, false);
                let opener_close = tag_text(*opener_kind, opener, true);
                (
                    UNCLOSED_CODE,
                    format!("{opener_open} is never closed: this needs a matching {opener_close}"),
                )
            }
        }
    }
}

impl Backend {
    /// Check that the component tags of the template at `uri` pair up.
    ///
    /// Like the directive-balance check this reads the raw Blade source
    /// rather than the virtual PHP: a tag's body is Blade markup, and the
    /// ranges are the template's from the start, so nothing has to go
    /// back through the source map.
    pub(super) fn collect_blade_component_tag_diagnostics(
        &self,
        uri: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if !self.is_blade_file(uri) {
            return;
        }
        let Some(source) = self.get_file_content_arc(uri) else {
            return;
        };

        report_block_imbalances(&source, tag_imbalances(&source), out);
    }

    /// Report the component tags of the template at `uri` that name no
    /// component the project has.
    ///
    /// Each occurrence is reported on the tag's name, so every misspelling
    /// is visible where it is written, and a closing tag (which repeats the
    /// name) is left to the opening tag's report.
    pub(super) fn collect_blade_unknown_component_diagnostics(
        &self,
        uri: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if !self.is_blade_file(uri) {
            return;
        }
        let Some(source) = self.get_file_content_arc(uri) else {
            return;
        };
        let tags = tag_spans(&source);
        if tags.is_empty() {
            return;
        }
        let index = self.component_index(tags.iter().any(|tag| tag.kind == TagKind::Livewire));
        for tag in tags {
            if !index.is_unknown(self, tag.kind, &tag.name) {
                continue;
            }
            let (code, message) = unknown_component_report(tag.kind, &tag.name);
            out.push(super::helpers::make_diagnostic(
                crate::text_position::byte_range_to_lsp_range(
                    &source,
                    tag.name_span.start,
                    tag.name_span.end,
                ),
                DiagnosticSeverity::Warning,
                code,
                message,
            ));
        }
    }
}
