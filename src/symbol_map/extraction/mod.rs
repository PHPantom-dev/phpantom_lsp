//! AST extraction for the symbol map.
//!
//! This module walks the `mago_syntax` AST and emits [`SymbolSpan`],
//! [`VarDefSite`], [`TemplateParamDef`], and [`CallSite`] entries for
//! every navigable symbol occurrence.  The entry point is
//! [`extract_symbol_map`].

use mago_syntax::cst::*;

use super::docblock::{
    class_ref_span, class_ref_span_ctx, extract_docblock_symbols, extract_param_var_spans,
    extract_var_docblock_var_spans, get_docblock_text_with_offset, is_navigable_type,
};
use super::{
    CallSite, ClassRefContext, SelfStaticParentKind, SymbolKind, SymbolMap, SymbolSpan,
    TemplateParamDef, UntypedClosureSite, VarDefKind, VarDefSite,
};
use crate::atom::bytes_to_str;
use crate::util::strip_fqn_prefix;

// ─── Extraction context ─────────────────────────────────────────────────────

/// Bundles the mutable accumulators and read-only context threaded through
/// every `extract_from_*` function.
///
/// Before this struct existed, each extractor took 7–8 parameters (the five
/// `Vec`s plus `trivias`, `content`, and sometimes `scope_start`).  Grouping
/// them here eliminates the `#[allow(clippy::too_many_arguments)]` annotations
/// that were required on 19 functions and makes it trivial to add new
/// accumulated data in the future without touching every call site.
struct ExtractionCtx<'a> {
    /// Navigable symbol spans (class refs, member accesses, variables, …).
    spans: Vec<SymbolSpan>,
    /// Variable definition sites (assignments, parameters, foreach, …).
    var_defs: Vec<VarDefSite>,
    /// Scope ranges `(start, end)` for functions, methods, closures, etc.
    scopes: Vec<(u32, u32)>,
    /// Scope start offsets of arrow functions (inheriting scopes).
    arrow_fn_scopes: Vec<u32>,
    /// Body boundaries `(body_start, body_end)` for closures and arrow fns.
    /// For closures the body start is the `{` offset; for arrow functions
    /// it is the `=>` token offset.  Used by signature help suppression.
    body_scopes: Vec<(u32, u32)>,
    /// Narrowing block boundaries `(start, end)` for if-body, elseif-body,
    /// else-body, match-arm, and switch-case blocks.  Used by the
    /// diagnostic subject cache to determine whether two variable accesses
    /// are in the same narrowing context.  Accesses in the same block get
    /// the same instanceof narrowing applied and can share a cache entry.
    narrowing_blocks: Vec<(u32, u32)>,
    /// Offsets of `assert($var instanceof ...)` expression statements.
    /// Used as sequential narrowing boundaries in the diagnostic cache.
    assert_narrowing_offsets: Vec<u32>,
    /// `@template` parameter definitions with their scoping ranges.
    template_defs: Vec<TemplateParamDef>,
    /// Call-site records for signature help and conditional return types.
    call_sites: Vec<CallSite>,
    /// Ranges where `break` is valid (loops and `switch`).
    breakable_scopes: Vec<(u32, u32)>,
    /// Ranges where `continue` is valid (loops only).
    loop_scopes: Vec<(u32, u32)>,
    /// Ranges of `switch` bodies (where `case/default` labels are valid).
    switch_scopes: Vec<(u32, u32)>,
    /// Ranges of static method bodies `(start_offset, end_offset)`.
    /// Used to detect whether `$this` is unavailable at a given offset.
    static_method_scopes: Vec<(u32, u32)>,
    /// Ranges of non-static (instance) method bodies.
    instance_method_scopes: Vec<(u32, u32)>,
    /// Trivia (comments, whitespace) from the parsed program.
    trivias: &'a [Trivia<'a>],
    /// The full source text of the file being extracted.
    content: &'a str,
    /// Closures and arrow functions passed as arguments to callable-typed
    /// parameters, used by inlay hints.
    untyped_closure_sites: Vec<UntypedClosureSite>,
    /// Current conditional nesting depth (if/else, switch, while, for, etc.).
    /// Incremented when entering a conditional block, decremented when leaving.
    cond_nesting_depth: u16,
    /// Stack of block-end offsets for each conditional nesting level.
    /// The top of the stack is the end of the innermost conditional block.
    cond_block_end_stack: Vec<u32>,
    /// Whether the file imports from `Illuminate\Container\Attributes\`
    /// (checked once lazily, cached for all attribute inspections).
    has_laravel_container_attrs: Option<bool>,
}

mod class_like;
mod expressions;
mod keywords;
mod laravel;
mod statements;
mod subject_text;

use class_like::*;
use expressions::*;
use keywords::*;
use laravel::*;
use statements::*;
use subject_text::*;

// ─── AST extraction ─────────────────────────────────────────────────────────

/// Build a [`SymbolMap`] from a parsed PHP program.
///
/// Walks every statement recursively and emits [`SymbolSpan`] entries for
/// every navigable symbol occurrence.
pub(crate) fn extract_symbol_map(program: &Program<'_>, content: &str) -> SymbolMap {
    let mut ctx = ExtractionCtx {
        spans: Vec::new(),
        var_defs: Vec::new(),
        scopes: Vec::new(),
        arrow_fn_scopes: Vec::new(),
        body_scopes: Vec::new(),
        narrowing_blocks: Vec::new(),
        assert_narrowing_offsets: Vec::new(),
        template_defs: Vec::new(),
        call_sites: Vec::new(),
        breakable_scopes: Vec::new(),
        loop_scopes: Vec::new(),
        switch_scopes: Vec::new(),
        static_method_scopes: Vec::new(),
        instance_method_scopes: Vec::new(),
        trivias: program.trivia.as_slice(),
        content,
        untyped_closure_sites: Vec::new(),
        cond_nesting_depth: 0,
        cond_block_end_stack: Vec::new(),
        has_laravel_container_attrs: None,
    };

    for stmt in program.statements.iter() {
        extract_from_statement(stmt, &mut ctx, 0);
    }

    // ── Sweep all docblock trivia for floating references ───────────
    // Docblocks attached to classes, functions, methods, properties, and
    // certain statements are already processed during the AST walk above.
    // However, docblocks in other positions (e.g. inline `/** @see ... */`
    // inside array literals or after expressions) are never visited.
    // Scan every docblock trivia entry and extract symbols; the dedup
    // step below removes any duplicates from already-processed docblocks.
    for t in program.trivia.iter() {
        if t.kind == TriviaKind::DocBlockComment {
            let _tpl = extract_docblock_symbols(
                bytes_to_str(t.value),
                t.span.start.offset,
                &mut ctx.spans,
            );
        }
    }

    // Emit comment spans for all comment trivia so semantic tokens
    // can highlight comments in Blade files.  For docblock comments,
    // also emit keyword spans for PHPDoc tags.
    //
    // Multi-line block comments are split into one span per line so that
    // the semantic token layer can emit correct per-line lengths without
    // any post-processing.  The LSP protocol requires token `length` to
    // describe characters on a single line only.
    for t in program.trivia.iter() {
        if t.kind.is_comment() {
            let mut byte_cursor = t.span.start.offset as usize;
            for line_text in bytes_to_str(t.value).split('\n') {
                // `line_text` may end with '\r' on Windows; strip it for
                // length calculation but keep the byte advance correct.
                let display = line_text.trim_end_matches('\r');
                let display_len = display.len() as u32;
                if display_len > 0 {
                    ctx.spans.push(SymbolSpan {
                        start: byte_cursor as u32,
                        end: byte_cursor as u32 + display_len,
                        kind: SymbolKind::Comment,
                    });
                }
                // Advance past this segment plus the '\n' (line_text
                // includes '\r' if present, so +1 covers just the LF).
                byte_cursor += line_text.len() + 1;
            }
            if t.kind == TriviaKind::DocBlockComment {
                emit_phpdoc_tag_keywords(
                    bytes_to_str(t.value),
                    t.span.start.offset,
                    &mut ctx.spans,
                );
            }
        }
    }

    // Sort by start offset for binary search.
    ctx.spans.sort_by_key(|s| s.start);

    // Deduplicate overlapping spans (keep the first / most specific).
    ctx.spans
        .dedup_by(|b, a| a.start == b.start && a.end == b.end);

    // Sort var_defs by (scope_start, offset) for efficient lookup.
    ctx.var_defs.sort_by(|a, b| {
        a.scope_start
            .cmp(&b.scope_start)
            .then(a.offset.cmp(&b.offset))
    });

    // Sort scopes by start offset.
    ctx.scopes.sort_by_key(|s| s.0);

    // Sort narrowing blocks by start offset.
    ctx.narrowing_blocks.sort_by_key(|s| s.0);

    // Sort assert-narrowing offsets.
    ctx.assert_narrowing_offsets.sort();

    // Sort template_defs by name_offset for binary search / reverse scan.
    ctx.template_defs.sort_by_key(|d| d.name_offset);

    // Sort call_sites by args_start for reverse-scan lookup.
    ctx.call_sites.sort_by_key(|cs| cs.args_start);
    ctx.breakable_scopes.sort_by_key(|s| s.0);
    ctx.loop_scopes.sort_by_key(|s| s.0);
    ctx.switch_scopes.sort_by_key(|s| s.0);
    ctx.static_method_scopes.sort_by_key(|s| s.0);

    let mut member_access_indices: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (idx, span) in ctx.spans.iter().enumerate() {
        if let SymbolKind::MemberAccess { member_name, .. } = &span.kind {
            member_access_indices
                .entry(member_name.clone())
                .or_default()
                .push(idx);
        }
    }

    SymbolMap {
        spans: ctx.spans,
        member_access_indices,
        var_defs: ctx.var_defs,
        scopes: ctx.scopes,
        arrow_fn_scopes: ctx.arrow_fn_scopes,
        body_scopes: ctx.body_scopes,
        narrowing_blocks: ctx.narrowing_blocks,
        assert_narrowing_offsets: ctx.assert_narrowing_offsets,
        template_defs: ctx.template_defs,
        call_sites: ctx.call_sites,
        breakable_scopes: ctx.breakable_scopes,
        loop_scopes: ctx.loop_scopes,
        switch_scopes: ctx.switch_scopes,
        static_method_scopes: ctx.static_method_scopes,
        instance_method_scopes: ctx.instance_method_scopes,
        untyped_closure_sites: ctx.untyped_closure_sites,
    }
}
