//! Shared reporting for block-imbalance checks.
//!
//! [`crate::blade::balance::Imbalance`] (directive pairing) and
//! [`crate::blade::component_tags::TagImbalance`] (component tag pairing)
//! both describe the same three shapes a block structure can go wrong in:
//! a closer that closes the wrong thing, a closer with nothing open to
//! close, and a block nobody ever closes. [`BlockImbalance`] lets
//! [`report_block_imbalances`] turn either kind into diagnostics with one
//! loop instead of two copies of it.

use std::ops::Range;

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

/// One report from a block-structure scan (directive or component tag).
pub(super) trait BlockImbalance {
    /// The range the diagnostic is anchored on: the offending
    /// closer/opener.
    fn span(&self) -> &Range<usize>;

    /// The diagnostic code and message for this imbalance.
    ///
    /// `line_of` turns a byte span into the 1-based source line it starts
    /// on; only the mismatched-closer message uses it, to cite the line
    /// the still-open block began on.
    fn code_and_message<F: Fn(&Range<usize>) -> u32>(&self, line_of: F) -> (&'static str, String);
}

/// Turn every imbalance the scan found into a diagnostic anchored on its
/// span, using `source` to translate byte offsets into LSP positions.
pub(super) fn report_block_imbalances<T: BlockImbalance>(
    source: &str,
    imbalances: impl IntoIterator<Item = T>,
    out: &mut Vec<Diagnostic>,
) {
    let range_of = |span: &Range<usize>| {
        crate::text_position::byte_range_to_lsp_range(source, span.start, span.end)
    };

    for imbalance in imbalances {
        let (code, message) = imbalance.code_and_message(|span| range_of(span).start.line + 1);
        out.push(super::helpers::make_diagnostic(
            range_of(imbalance.span()),
            DiagnosticSeverity::ERROR,
            code,
            message,
        ));
    }
}
