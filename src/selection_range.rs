//! Selection range handler for `textDocument/selectionRange`.
//!
//! "Smart select" / expand selection.  Given a cursor position, returns a
//! nested chain of ranges from innermost to outermost (e.g. identifier →
//! expression → statement → block → function → class → file).  AST-aware
//! selection ranges produce much tighter expansions than word/line/block.
//!
//! The implementation parses the file with `mago_syntax`, then walks the AST
//! generically via [`Node::visit_children`], collecting the span of every node
//! on the path from the program root down to the cursor.  Because the walk is
//! generic over the untyped [`Node`] enum, every current and future AST variant
//! contributes a selection level automatically; there are no per-variant match
//! arms that a new syntax node could slip past.  A small set of *synthetic*
//! spans (brace interiors of class-like bodies and `match` expressions, which
//! are not the span of any single node) is added on top.  The collected spans
//! are sorted outermost-first and linked into the `SelectionRange` list that
//! the LSP protocol expects.

use mago_allocator::LocalArena;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{Position, Range, SelectionRange};

use crate::Backend;
use crate::text_position::{offset_to_position, position_to_offset};

// ─── Public entry point ─────────────────────────────────────────────────────

impl Backend {
    /// Compute selection ranges for the given positions in the file.
    pub fn handle_selection_range(
        &self,
        content: &str,
        positions: &[Position],
    ) -> Option<Vec<SelectionRange>> {
        let arena = LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());

        let mut results = Vec::with_capacity(positions.len());

        for pos in positions {
            let offset = position_to_offset(content, *pos);

            // Collect all spans that contain the cursor, from the AST walk.
            let mut spans: Vec<(u32, u32)> = Vec::new();

            // Add the whole-file span as the outermost range.
            spans.push((0u32, content.len() as u32));

            collect(Node::Program(program), offset, &mut spans);

            // Deduplicate identical spans and sort outermost-first (largest
            // span first).  When two spans have the same length, the one
            // starting earlier comes first.  The generic walk visits wrapper
            // nodes whose span equals their only child's, so duplicates are
            // expected and removed here.
            spans.sort_unstable();
            spans.dedup();
            spans.sort_by(|a, b| {
                let len_a = a.1.saturating_sub(a.0);
                let len_b = b.1.saturating_sub(b.0);
                len_b.cmp(&len_a).then(a.0.cmp(&b.0))
            });

            let selection_range = build_selection_range(content, &spans);
            results.push(selection_range);
        }

        Some(results)
    }
}

// ─── Generic node descent ───────────────────────────────────────────────────

/// Descend into the subtree containing the cursor, pushing the span of every
/// node on the cursor path.  Pruning to the subtree that contains `offset`
/// keeps the walk linear in the depth of the cursor rather than the size of
/// the file.
fn collect(node: Node<'_, '_>, offset: u32, spans: &mut Vec<(u32, u32)>) {
    let span = node.span();
    if offset < span.start.offset || offset > span.end.offset {
        return; // prune: only descend into the subtree containing the cursor
    }
    spans.push((span.start.offset, span.end.offset));
    push_synthetic_spans(&node, offset, spans);
    node.visit_children(|child| collect(child, offset, spans));
}

/// Push spans that are *not* the span of any single AST node and therefore
/// cannot be derived from the generic descent.  These are the brace interiors
/// of class-like bodies (`class`, `interface`, `trait`, `enum`, anonymous
/// classes) and `match` expressions: the node itself spans the whole
/// declaration (header included), so the `{ … }` region needs its own span to
/// remain a selection level.
///
/// Block interiors (`{ … }` statement blocks), parameter lists, argument
/// lists, and `switch` bodies are *not* synthetic — each is its own node whose
/// span already equals its delimiter pair, so the generic descent contributes
/// them automatically.
fn push_synthetic_spans(node: &Node<'_, '_>, offset: u32, spans: &mut Vec<(u32, u32)>) {
    match node {
        Node::Class(class) => push_brace_pair(class.left_brace, class.right_brace, offset, spans),
        Node::Interface(iface) => {
            push_brace_pair(iface.left_brace, iface.right_brace, offset, spans)
        }
        Node::Trait(trait_def) => {
            push_brace_pair(trait_def.left_brace, trait_def.right_brace, offset, spans)
        }
        Node::Enum(enum_def) => {
            push_brace_pair(enum_def.left_brace, enum_def.right_brace, offset, spans)
        }
        Node::AnonymousClass(anon) => {
            push_brace_pair(anon.left_brace, anon.right_brace, offset, spans)
        }
        Node::Match(match_expr) => {
            push_brace_pair(match_expr.left_brace, match_expr.right_brace, offset, spans)
        }
        _ => {}
    }
}

/// Push a brace-delimited range (left_brace..right_brace) if it contains the cursor.
fn push_brace_pair(
    left: mago_span::Span,
    right: mago_span::Span,
    offset: u32,
    spans: &mut Vec<(u32, u32)>,
) {
    let start = left.start.offset;
    let end = right.end.offset;
    if start <= offset && offset <= end {
        spans.push((start, end));
    }
}

// ─── Linked-list builder ────────────────────────────────────────────────────

/// Build a `SelectionRange` linked list from a list of spans sorted
/// outermost-first.
fn build_selection_range(content: &str, spans: &[(u32, u32)]) -> SelectionRange {
    if spans.is_empty() {
        let range = Range::new(Position::new(0, 0), Position::new(0, 0));
        return SelectionRange {
            range,
            parent: None,
        };
    }

    // Start from the outermost and wrap inward.
    let mut current = to_selection_range(content, spans[0], None);

    for &span in &spans[1..] {
        current = to_selection_range(content, span, Some(current));
    }

    current
}

fn to_selection_range(
    content: &str,
    span: (u32, u32),
    parent: Option<SelectionRange>,
) -> SelectionRange {
    let start = offset_to_position(content, span.0 as usize);
    let end = offset_to_position(content, span.1 as usize);
    SelectionRange {
        range: Range::new(start, end),
        parent: parent.map(Box::new),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "selection_range_tests.rs"]
mod tests;
