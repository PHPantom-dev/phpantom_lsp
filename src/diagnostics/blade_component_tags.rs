//! Unbalanced `<x-…>` / `<livewire:…>` component tags.
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

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::Backend;
use crate::blade::component_tags::{TagImbalance, TagKind, tag_imbalances};

/// A closing tag that closes something other than the tag it sits in.
const MISMATCHED_CODE: &str = "mismatched_blade_component_tag";
/// A closing tag with no open tag at all.
const UNEXPECTED_CODE: &str = "unexpected_blade_component_tag";
/// A tag the template never closes.
const UNCLOSED_CODE: &str = "unclosed_blade_component_tag";

/// A tag's opening or closing spelling, for messages: `<x-alert>` /
/// `</x-alert>`, `<livewire:counter>` / `</livewire:counter>`.
fn tag_text(kind: TagKind, name: &str, closing: bool) -> String {
    format!(
        "<{slash}{prefix}{name}>",
        slash = if closing { "/" } else { "" },
        prefix = kind.prefix(),
    )
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
        let range_of = |span: &std::ops::Range<usize>| {
            crate::text_position::byte_range_to_lsp_range(&source, span.start, span.end)
        };

        for imbalance in tag_imbalances(&source) {
            let (code, message) = match &imbalance {
                TagImbalance::Mismatched {
                    found_kind,
                    found,
                    opener_kind,
                    opener,
                    opener_span,
                    ..
                } => {
                    let line = range_of(opener_span).start.line + 1;
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
                        format!(
                            "{opener_open} is never closed: this needs a matching {opener_close}"
                        ),
                    )
                }
            };
            out.push(super::helpers::make_diagnostic(
                range_of(imbalance.span()),
                DiagnosticSeverity::ERROR,
                code,
                message,
            ));
        }
    }
}
