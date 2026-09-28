//! Block directives that do not pair up.
//!
//! Blade maps every directive to PHP on its own, so `@foreach (…) … @endif`
//! compiles happily into a `foreach (…): … endif;` that PHP then refuses to
//! parse, and a block nobody closes produces a template that renders only
//! part of itself. Neither is reported against the template: the failure
//! surfaces in the compiled cache file, at a line the author never wrote.
//!
//! [`crate::blade::balance`] walks the template's directive stream with a
//! stack of open blocks; this turns what it finds into reports anchored on
//! the offending directive in the Blade source.

use tower_lsp::lsp_types::Diagnostic;

use crate::Backend;
use crate::blade::balance::{self, Imbalance};

use super::blade_imbalance::{BlockImbalance, report_block_imbalances};

/// A closing directive that closes something other than the block it sits
/// in.
const MISMATCHED_CODE: &str = "mismatched_blade_directive";
/// A closing directive with no open block at all.
const UNEXPECTED_CODE: &str = "unexpected_blade_directive";
/// A block the template never closes.
const UNCLOSED_CODE: &str = "unclosed_blade_directive";

impl BlockImbalance for Imbalance {
    fn span(&self) -> &std::ops::Range<usize> {
        Imbalance::span(self)
    }

    fn code_and_message<F: Fn(&std::ops::Range<usize>) -> u32>(
        &self,
        line_of: F,
    ) -> (&'static str, String) {
        match self {
            Imbalance::Mismatched {
                found,
                expected,
                opener,
                opener_span,
                ..
            } => {
                let line = line_of(opener_span);
                (
                    MISMATCHED_CODE,
                    format!(
                        "Expected @{expected} to close the @{opener} on line {line}, found @{found}"
                    ),
                )
            }
            Imbalance::Unexpected { found, opener, .. } => (
                UNEXPECTED_CODE,
                format!("@{found} closes nothing: no @{opener} is open here"),
            ),
            Imbalance::Unclosed {
                opener, expected, ..
            } => (
                UNCLOSED_CODE,
                format!("@{opener} is never closed: this block needs a matching @{expected}"),
            ),
        }
    }
}

impl Backend {
    /// Check that the block directives of the template at `uri` pair up.
    ///
    /// Like the signature collector this reads the raw Blade source rather
    /// than the virtual PHP: block structure is Blade's own, and the
    /// ranges are the template's from the start, so nothing has to go back
    /// through the source map.
    pub(super) fn collect_blade_directive_diagnostics(&self, uri: &str, out: &mut Vec<Diagnostic>) {
        if !self.is_blade_file(uri) {
            return;
        }
        // The open buffer is shared rather than copied: this runs on every
        // pass over the template, and the source is only read from.
        let Some(source) = self.get_file_content_arc(uri) else {
            return;
        };

        report_block_imbalances(&source, balance::check(&source), out);
    }
}
