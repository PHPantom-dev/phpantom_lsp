use mago_syntax::cst::*;

use super::*;

// ─── Keyword helper ─────────────────────────────────────────────────────────

/// Emit a keyword span.
pub(super) fn emit_keyword(kw: &keyword::Keyword<'_>, ctx: &mut ExtractionCtx<'_>) {
    let start = kw.span.start.offset;
    let end = kw.span.end.offset;
    if end > start {
        ctx.spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::Keyword,
        });
    }
}

/// Emit keyword spans for PHPDoc tags (`@var`, `@param`, `@return`, etc.)
/// found inside a docblock comment.
pub(super) fn emit_phpdoc_tag_keywords(text: &str, base_offset: u32, spans: &mut Vec<SymbolSpan>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let tag_start = i;
            i += 1;
            // Consume alphabetic chars and hyphens (for @psalm-suppress, @phpstan-ignore, etc.)
            while i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'-') {
                i += 1;
            }
            let tag_len = i - tag_start;
            if tag_len > 1 {
                spans.push(SymbolSpan {
                    start: base_offset + tag_start as u32,
                    end: base_offset + i as u32,
                    kind: SymbolKind::Keyword,
                });
            }
        } else {
            i += 1;
        }
    }
}
