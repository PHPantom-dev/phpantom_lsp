//! AST extraction for the symbol map.
//!
//! This module walks the `mago_syntax` AST and emits [`SymbolSpan`],
//! [`VarDefSite`], [`TemplateParamDef`], and [`CallSite`] entries for
//! every navigable symbol occurrence.  The entry point is
//! [`extract_symbol_map`].

use mago_syntax::cst::*;

use super::docblock::{
    class_ref_span, class_ref_span_ctx, extract_docblock_symbols,
    extract_docblock_symbols_covering, get_docblock_text_with_offset, is_navigable_type,
};
use super::{
    CallSite, ClassRefContext, DocblockMemberRef, SelfStaticParentKind, SubjectText, SymbolKind,
    SymbolMap, SymbolSpan, TemplateParamDef, UntypedClosureSite, VarDefKind, VarDefSite,
    ViewReceiverClass, ViewReceiverSite,
};
use crate::atom::{bytes_to_str, literal_bytes_to_str};
use crate::names::OwnedResolvedNames;
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
    /// Semantic names resolved by `mago-names` for this file. Production
    /// indexing supplies this so Laravel-specific syntax does not guess from
    /// an alias or a namespace-local homonym. Syntax-only tests omit it.
    resolved_names: Option<&'a OwnedResolvedNames>,
    /// Laravel strings whose meaning depends on a workspace symbol being
    /// absent. They are activated after the batch publishes its declarations.
    conditional_laravel_spans: Vec<super::ConditionalLaravelStringSpan>,
    /// Closures and arrow functions passed as arguments to callable-typed
    /// parameters, used by inlay hints.
    untyped_closure_sites: Vec<UntypedClosureSite>,
    /// Render sites whose receiver only a type settles, left for
    /// `Backend::typed_receiver_view_spans` to confirm.
    view_receiver_sites: Vec<ViewReceiverSite>,
    /// The model argument of each authorization check that named one.
    gate_subjects: Vec<crate::symbol_map::GateSubject>,
    /// Current conditional nesting depth (if/else, switch, while, for, etc.).
    /// Incremented when entering a conditional block, decremented when leaving.
    cond_nesting_depth: u16,
    /// Stack of block-end offsets for each conditional nesting level.
    /// The top of the stack is the end of the innermost conditional block.
    cond_block_end_stack: Vec<u32>,
    /// Whether the file imports from `Illuminate\Container\Attributes\`
    /// (checked once lazily, cached for all attribute inspections).
    has_laravel_container_attrs: Option<bool>,
    /// Whether the file imports from `PHPUnit\Framework\Attributes\`, cached
    /// the same way as [`Self::has_laravel_container_attrs`].
    has_phpunit_attrs: Option<bool>,
    /// Whether the members currently being extracted belong to a class that
    /// syntactically extends an Artisan console command (or is `#[AsCommand]`
    /// decorated).  Gates recognition of `$this->call('cmd')` /
    /// `$this->callSilently('cmd')` as command-name references.
    in_console_command: bool,
    /// Whether the members currently being extracted belong to a class that
    /// syntactically extends `Illuminate\Mail\Mailable`.  Gates recognition
    /// of `$this->view('emails.shipped')` and of `new Content(view: …)` as
    /// view-name references.
    in_mailable: bool,
    /// The `@coversDefaultClass` of the class whose members are currently
    /// being extracted.  It is what gives `@covers ::name` on a test method
    /// a subject; without one that shape names a global function.
    covers_default_class: Option<crate::atom::Atom>,
}

impl<'a> ExtractionCtx<'a> {
    fn resolved_name_at(&self, offset: u32) -> Option<&'a str> {
        self.resolved_names.and_then(|names| names.get(offset))
    }

    /// Move Laravel string spans emitted since `start` into the dormant
    /// candidate list. The tail is normally one span, so removing in place
    /// avoids allocating a temporary vector on this already-rare path.
    fn defer_laravel_spans_since(
        &mut self,
        start: usize,
        dependency: super::LaravelStringDependency,
    ) {
        let spans = &mut self.spans;
        let candidates = &mut self.conditional_laravel_spans;
        for span in spans.drain(start..) {
            debug_assert!(matches!(&span.kind, SymbolKind::LaravelStringKey { .. }));
            candidates.push(super::ConditionalLaravelStringSpan::new(dependency, span));
        }
    }
}

mod class_like;
mod expressions;
mod keywords;
mod laravel;
mod phpunit;
mod statements;
mod subject_text;

use class_like::*;
use expressions::*;
use keywords::*;
use laravel::*;
use phpunit::*;
use statements::*;
use subject_text::*;

/// Descend generically through a node the typed dispatchers have no arm for,
/// re-entering the typed extractors at the next expression or statement
/// boundary so new AST variants cannot create blind spots.
///
/// The explicit dispatchers thread real scope state (`scope_start`,
/// conditional nesting, the various `*_scopes` vectors); this fallback does
/// not reproduce it.  It only rescues variants that would otherwise contribute
/// nothing at all, so partial coverage (symbol spans, but no keyword emission
/// or var-def sites) is the intended result for those.
fn descend_unhandled<'a>(node: Node<'a, 'a>, ctx: &mut ExtractionCtx<'a>, scope_start: u32) {
    node.visit_children(|child| match child {
        Node::Expression(e) => extract_from_expression(e, ctx, scope_start),
        Node::Statement(s) => extract_from_statement(s, ctx, scope_start),
        other => descend_unhandled(other, ctx, scope_start),
    });
}

// ─── AST extraction ─────────────────────────────────────────────────────────

/// Build a [`SymbolMap`] from a parsed PHP program.
///
/// Walks every statement recursively and emits [`SymbolSpan`] entries for
/// every navigable symbol occurrence.
#[cfg(test)]
pub(crate) fn extract_symbol_map(program: &Program<'_>, content: &str) -> SymbolMap {
    extract_symbol_map_inner(program, content, None)
}

/// Build a [`SymbolMap`] using the semantic names produced by `mago-names`.
#[cfg(test)]
pub(crate) fn extract_symbol_map_with_resolved_names(
    program: &Program<'_>,
    content: &str,
    resolved_names: &OwnedResolvedNames,
) -> SymbolMap {
    let mut map = extract_symbol_map_inner(program, content, Some(resolved_names));
    map.refresh_conditional_laravel_spans(None, |_| false);
    map
}

/// Build a semantic map while retaining workspace-dependent Laravel spans as
/// dormant candidates for the publication lifecycle to settle.
pub(crate) fn extract_symbol_map_for_index(
    program: &Program<'_>,
    content: &str,
    resolved_names: &OwnedResolvedNames,
) -> SymbolMap {
    extract_symbol_map_inner(program, content, Some(resolved_names))
}

fn extract_symbol_map_inner(
    program: &Program<'_>,
    content: &str,
    resolved_names: Option<&OwnedResolvedNames>,
) -> SymbolMap {
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
        resolved_names,
        conditional_laravel_spans: Vec::new(),
        untyped_closure_sites: Vec::new(),
        view_receiver_sites: Vec::new(),
        gate_subjects: Vec::new(),
        cond_nesting_depth: 0,
        cond_block_end_stack: Vec::new(),
        has_laravel_container_attrs: None,
        has_phpunit_attrs: None,
        in_console_command: false,
        in_mailable: false,
        covers_default_class: None,
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
            let _found = extract_docblock_symbols(
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

    ctx.scopes.sort_by_key(|s| s.0);
    ctx.narrowing_blocks.sort_by_key(|s| s.0);
    ctx.assert_narrowing_offsets.sort();

    // Sort template_defs by name_offset for binary search / reverse scan.
    ctx.template_defs.sort_by_key(|d| d.name_offset);

    // Sort call_sites by args_start for reverse-scan lookup.
    ctx.call_sites.sort_by_key(|cs| cs.args_start);
    ctx.breakable_scopes.sort_by_key(|s| s.0);
    ctx.loop_scopes.sort_by_key(|s| s.0);
    ctx.switch_scopes.sort_by_key(|s| s.0);
    ctx.static_method_scopes.sort_by_key(|s| s.0);
    ctx.view_receiver_sites.sort_by_key(|s| s.start);

    let mut member_access_indices: crate::atom::AtomMap<Vec<usize>> =
        crate::atom::AtomMap::default();
    for (idx, span) in ctx.spans.iter().enumerate() {
        if let SymbolKind::MemberAccess { member_name, .. } = &span.kind {
            member_access_indices
                .entry(*member_name)
                .or_default()
                .push(idx);
        }
    }

    SymbolMap {
        spans: ctx.spans,
        conditional_laravel_spans: ctx.conditional_laravel_spans,
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
        view_receiver_sites: ctx.view_receiver_sites,
        gate_subjects: ctx.gate_subjects,
        source_len: u32::try_from(content.len()).unwrap_or(u32::MAX),
    }
}
