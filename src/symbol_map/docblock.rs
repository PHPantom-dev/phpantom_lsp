//! Docblock symbol extraction helpers for the symbol map.
//!
//! This module contains functions that scan PHPDoc comment blocks for
//! type references in supported tags (`@param`, `@return`, `@var`,
//! `@template`, `@method`, etc.) and emit [`SymbolSpan`] entries with
//! correct file-level byte offsets.
//!
//! Tag detection and iteration uses the structured [`DocblockInfo`] /
//! [`TagInfo`] infrastructure from [`crate::docblock::parser`], which
//! delegates to `mago-docblock` for parsing.  Type *string* decomposition
//! (unions, intersections, generics, callables, conditionals) remains
//! structured via [`emit_type_spans`] which uses `mago-type-syntax` to
//! parse types and walk the AST with accurate span information.

use bumpalo::Bump;
use mago_database::file::FileId;
use mago_docblock::document::TagKind;
use mago_span::{HasSpan, Position, Span};
use mago_syntax::ast::*;
use mago_type_syntax::ast as type_ast;

use crate::docblock::parser::parse_docblock;
use crate::docblock::types::split_type_token;
use crate::php_type::PhpType;
use crate::types::TemplateVariance;

use super::{
    ClassRefContext, SelfStaticParentKind, SymbolKind, SymbolSpan, self_static_parent_kind,
};
use crate::util::strip_fqn_prefix;

// ─── Navigability filter ────────────────────────────────────────────────────

/// Returns `true` when a type name refers to a class/interface that the
/// user should be able to navigate to.
///
/// Uses simple string splitting instead of `PhpType::parse()` + `base_name()`
/// because this is called for every type span during symbol-map extraction
/// and must stay allocation-free.
pub(crate) fn is_navigable_type(name: &str) -> bool {
    let base = name.split('<').next().unwrap_or(name);
    let base = base.split('{').next().unwrap_or(base);
    let base = base.trim();
    if base.is_empty() {
        return false;
    }
    !crate::php_type::is_keyword_type(base)
}

// ─── Span construction helpers ──────────────────────────────────────────────

/// Construct a `ClassReference` `SymbolSpan` from a raw identifier string.
///
/// Detects whether the name is fully-qualified (leading `\`) and sets
/// `is_fqn` accordingly.  The leading `\` is stripped from the stored
/// `name` in all cases.
pub(super) fn class_ref_span(start: u32, end: u32, raw_name: &str) -> SymbolSpan {
    let is_fqn = raw_name.starts_with('\\');
    let name = strip_fqn_prefix(raw_name).to_string();
    SymbolSpan {
        start,
        end,
        kind: SymbolKind::ClassReference {
            name,
            is_fqn,
            context: ClassRefContext::Other,
        },
    }
}

/// Like [`class_ref_span`] but with an explicit [`ClassRefContext`].
pub(super) fn class_ref_span_ctx(
    start: u32,
    end: u32,
    raw_name: &str,
    ctx: ClassRefContext,
) -> SymbolSpan {
    let is_fqn = raw_name.starts_with('\\');
    let name = strip_fqn_prefix(raw_name).to_string();
    SymbolSpan {
        start,
        end,
        kind: SymbolKind::ClassReference {
            name,
            is_fqn,
            context: ctx,
        },
    }
}

// ─── Docblock text retrieval ────────────────────────────────────────────────

/// Like [`crate::docblock::get_docblock_text_for_node`] but also returns
/// the byte offset of the `/**` opening within the file.
pub fn get_docblock_text_with_offset<'a>(
    trivia: &'a [Trivia<'a>],
    content: &str,
    node: &impl HasSpan,
) -> Option<(&'a str, u32)> {
    use crate::atom::bytes_to_str;
    let node_start = node.span().start.offset;
    let candidate_idx = trivia.partition_point(|t| t.span.start.offset < node_start);
    if candidate_idx == 0 {
        return None;
    }

    let content_bytes = content.as_bytes();
    let mut covered_from = node_start;

    for i in (0..candidate_idx).rev() {
        let t = &trivia[i];
        let t_end = t.span.end.offset;

        let gap = content_bytes
            .get(t_end as usize..covered_from as usize)
            .unwrap_or(&[]);
        if !gap.iter().all(u8::is_ascii_whitespace) {
            return None;
        }

        match t.kind {
            TriviaKind::DocBlockComment => {
                return Some((bytes_to_str(t.value), t.span.start.offset));
            }
            TriviaKind::WhiteSpace
            | TriviaKind::SingleLineComment
            | TriviaKind::MultiLineComment
            | TriviaKind::HashComment => {
                covered_from = t.span.start.offset;
            }
        }
    }

    None
}

// ─── Tag classification ─────────────────────────────────────────────────────

/// `TagKind` values whose description starts with a type expression.
const TYPE_FIRST_KINDS: &[TagKind] = &[
    TagKind::Param,
    TagKind::Return,
    TagKind::Throws,
    TagKind::Var,
    TagKind::Property,
    TagKind::PropertyRead,
    TagKind::PropertyWrite,
    TagKind::Mixin,
    TagKind::Extends,
    TagKind::Implements,
    TagKind::Use,
    TagKind::TemplateExtends,
    TagKind::TemplateImplements,
    TagKind::PhpstanReturn,
    TagKind::PhpstanParam,
    TagKind::PhpstanVar,
    TagKind::PhpstanRequireExtends,
    TagKind::PhpstanRequireImplements,
    TagKind::PsalmReturn,
    TagKind::PsalmParam,
    TagKind::PsalmVar,
    TagKind::PhpstanAssert,
    TagKind::PhpstanAssertIfTrue,
    TagKind::PhpstanAssertIfFalse,
    TagKind::PsalmAssert,
    TagKind::PsalmAssertIfTrue,
    TagKind::PsalmAssertIfFalse,
];

/// Tag names (for `TagKind::Other`) whose description starts with a type.
///
/// Note: `@psalm-return`, `@psalm-param`, and `@psalm-var` are no longer
/// listed here because `mago-docblock` now maps them to dedicated
/// `TagKind::PsalmReturn` / `PsalmParam` / `PsalmVar` variants (handled
/// in `TYPE_FIRST_KINDS` above).
const TYPE_FIRST_OTHER_NAMES: &[&str] = &["phpstan-sealed"];

use crate::docblock::templates::{TEMPLATE_KINDS, variance_for};

/// Determine the template variance for a tag, if it is a template tag.
fn template_variance_for_tag(tag: &TagKind) -> Option<TemplateVariance> {
    if TEMPLATE_KINDS.contains(tag) {
        Some(variance_for(*tag))
    } else {
        None
    }
}

/// Returns `true` when the tag's description starts with a type expression.
fn is_type_first_tag(kind: &TagKind, name: &str) -> bool {
    TYPE_FIRST_KINDS.contains(kind)
        || (*kind == TagKind::Other && TYPE_FIRST_OTHER_NAMES.contains(&name))
}

// ─── Docblock tag scanning ──────────────────────────────────────────────────

/// Scan a docblock for type references in supported tags and emit
/// `SymbolSpan` entries with file-level byte offsets.
///
/// Returns a list of `@template` parameter definitions found in the
/// docblock, each as `(name, byte_offset, optional_bound, variance)`.
pub(super) fn extract_docblock_symbols(
    docblock: &str,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) -> Vec<(String, u32, Option<PhpType>, TemplateVariance)> {
    // ── Inline `{@see ...}` references ──────────────────────────────
    // These appear in free-text, not as top-level tags, so we scan the
    // raw docblock text for them.
    extract_inline_see_symbols(docblock, base_offset, spans);

    // ── Parse structured tags ───────────────────────────────────────
    let base_span = Span::new(
        FileId::zero(),
        Position::new(base_offset),
        Position::new(base_offset + docblock.len() as u32),
    );
    let Some(info) = parse_docblock(docblock, base_span) else {
        return Vec::new();
    };

    let mut template_params: Vec<(String, u32, Option<PhpType>, TemplateVariance)> = Vec::new();

    for tag in &info.tags {
        let desc_file_offset = tag.description_span.start.offset;
        let desc_start_in_docblock = (desc_file_offset - base_offset) as usize;

        // ── @see ────────────────────────────────────────────────────
        if tag.kind == TagKind::See {
            extract_see_tag_symbol(tag, spans);
            continue;
        }

        // ── @method ─────────────────────────────────────────────────
        if tag.kind == TagKind::Method || tag.kind == TagKind::PsalmMethod {
            extract_method_tag_symbols(docblock, desc_start_in_docblock, base_offset, spans);
            continue;
        }

        // ── @property variants ───────────────────────────────────────
        if matches!(
            tag.kind,
            TagKind::Property
                | TagKind::PropertyRead
                | TagKind::PropertyWrite
                | TagKind::PsalmProperty
                | TagKind::PsalmPropertyRead
                | TagKind::PsalmPropertyWrite
        ) {
            extract_property_tag_symbols(docblock, desc_start_in_docblock, base_offset, spans);
            continue;
        }

        // ── @template variants ──────────────────────────────────────
        if let Some(variance) = template_variance_for_tag(&tag.kind) {
            if let Some((name, offset, bound)) =
                extract_template_tag_symbols(docblock, desc_start_in_docblock, base_offset, spans)
            {
                template_params.push((name, offset, bound, variance));
            }
            continue;
        }

        // ── Type-first tags ─────────────────────────────────────────
        if is_type_first_tag(&tag.kind, &tag.name) {
            emit_type_first_tag(docblock, desc_start_in_docblock, base_offset, spans);
        }
    }

    template_params
}

/// Emit type spans for a tag whose description starts with a type
/// expression (e.g. `@param string $name`, `@return Collection<int>`).
///
/// Uses [`join_multiline_type`] to handle types that span continuation
/// lines and [`emit_type_spans`] to produce navigable symbol spans.
fn emit_type_first_tag(
    docblock: &str,
    desc_start_in_docblock: usize,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) {
    if desc_start_in_docblock >= docblock.len() {
        return;
    }
    // The description may start with whitespace (e.g. double-space after
    // the tag name: `@param  Type`).  Trim it and adjust the offset so
    // that `join_multiline_type` begins at the first non-whitespace byte
    // on the same line.
    let raw = &docblock[desc_start_in_docblock..];
    let first_nl = raw.find('\n').unwrap_or(raw.len());
    let first_line = &raw[..first_nl];
    // Skip leading `@phpstan-assert` / `@psalm-assert` type modifiers
    // (`!` negation, `=` exact-type) so they aren't emitted as bogus
    // class-name references (e.g. `Class '=T' not found`).
    let trimmed = first_line.trim_start().trim_start_matches(['!', '=']);
    if trimmed.is_empty() {
        return;
    }
    let leading_ws = first_line.len() - trimmed.len();
    let adjusted_start = desc_start_in_docblock + leading_ws;

    let (joined, offset_map) = join_multiline_type(docblock, adjusted_start);
    let (type_token, _remainder) = split_type_token(&joined);
    if !type_token.is_empty() {
        let mut local_spans: Vec<SymbolSpan> = Vec::new();
        emit_type_spans(type_token, 0, &mut local_spans);
        for mut sp in local_spans {
            sp.start = base_offset
                + offset_map
                    .get(sp.start as usize)
                    .copied()
                    .unwrap_or(sp.start as usize) as u32;
            sp.end = base_offset
                + offset_map
                    .get(sp.end as usize)
                    .copied()
                    .unwrap_or(sp.end as usize) as u32;
            spans.push(sp);
        }
    }
}

/// Scan a docblock for `@param $varName` tokens and return
/// `(name_without_dollar, file_byte_offset_of_dollar)` pairs.
///
/// These are used by the symbol-map extraction to emit
/// [`SymbolKind::Variable`] spans so that rename and find-references
/// cover parameter names mentioned in docblocks.
pub(super) fn extract_param_var_spans(docblock: &str, base_offset: u32) -> Vec<(String, u32)> {
    let base_span = Span::new(
        FileId::zero(),
        Position::new(base_offset),
        Position::new(base_offset + docblock.len() as u32),
    );
    let Some(info) = parse_docblock(docblock, base_span) else {
        return Vec::new();
    };

    let mut results = Vec::new();

    for tag in &info.tags {
        let is_param = matches!(
            tag.kind,
            TagKind::Param | TagKind::PhpstanParam | TagKind::PsalmParam
        );
        if !is_param {
            continue;
        }

        // The description is `TypeHint $varName desc` or just `$varName`.
        // Find the `$` in the raw source covered by description_span so
        // the file offset is accurate.
        let desc_file_start = tag.description_span.start.offset;
        let desc_in_doc_start = (desc_file_start - base_offset) as usize;
        let desc_in_doc_end =
            ((tag.description_span.end.offset - base_offset) as usize).min(docblock.len());
        let raw_desc = &docblock[desc_in_doc_start..desc_in_doc_end];

        if let Some(dollar_pos) = raw_desc.find('$') {
            let rest = &raw_desc[dollar_pos..];
            let name_end = rest[1..]
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .map(|i| i + 1)
                .unwrap_or(rest.len());
            if name_end > 1 {
                let name = rest[1..name_end].to_string();
                let file_offset = desc_file_start + dollar_pos as u32;
                results.push((name, file_offset));
            }
        }
    }

    // Also scan @return / @phpstan-return / @psalm-return tags for
    // parameter references in conditional return types, e.g.
    //   @return ($strict is true ? Result : ($fallback is true ? Result : ?Result))
    // The `$strict` and `$fallback` tokens must be renamed together with
    // the function parameters.
    for tag in &info.tags {
        let is_return = matches!(
            tag.kind,
            TagKind::Return | TagKind::PhpstanReturn | TagKind::PsalmReturn
        );
        if !is_return {
            continue;
        }

        let desc_file_start = tag.description_span.start.offset;
        let desc_in_doc_start = (desc_file_start - base_offset) as usize;
        let desc_in_doc_end =
            ((tag.description_span.end.offset - base_offset) as usize).min(docblock.len());
        let raw_desc = &docblock[desc_in_doc_start..desc_in_doc_end];

        // Find all `($varName` patterns — these are conditional type
        // subjects.  Conditionals can be nested, so scan the entire
        // description.
        let bytes = raw_desc.as_bytes();
        for i in 0..raw_desc.len().saturating_sub(1) {
            if bytes[i] == b'(' && bytes[i + 1] == b'$' {
                let dollar_pos = i + 1;
                let rest = &raw_desc[dollar_pos..];
                let name_end = rest[1..]
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .map(|j| j + 1)
                    .unwrap_or(rest.len());
                if name_end > 1 {
                    let name = rest[1..name_end].to_string();
                    let file_offset = desc_file_start + dollar_pos as u32;
                    results.push((name, file_offset));
                }
            }
        }
    }

    results
}

/// Scan a docblock for `@var Type $varName` tokens and return
/// `(name_without_dollar, file_byte_offset_of_dollar)` pairs.
///
/// These are used by the symbol-map extraction to emit
/// [`VarDefSite`](super::VarDefSite) entries for inline `@var`
/// docblocks so the forward walker treats them as variable definitions.
pub(super) fn extract_var_docblock_var_spans(
    docblock: &str,
    base_offset: u32,
) -> Vec<(String, u32)> {
    let base_span = Span::new(
        FileId::zero(),
        Position::new(base_offset),
        Position::new(base_offset + docblock.len() as u32),
    );
    let Some(info) = parse_docblock(docblock, base_span) else {
        return Vec::new();
    };

    let mut results = Vec::new();

    for tag in &info.tags {
        let is_var = matches!(
            tag.kind,
            TagKind::Var | TagKind::PhpstanVar | TagKind::PsalmVar
        );
        if !is_var {
            continue;
        }

        // The description is `TypeHint $varName desc` or just `$varName`.
        // Find the `$` in the raw source covered by description_span so
        // the file offset is accurate.
        let desc_file_start = tag.description_span.start.offset;
        let desc_in_doc_start = (desc_file_start - base_offset) as usize;
        let desc_in_doc_end =
            ((tag.description_span.end.offset - base_offset) as usize).min(docblock.len());
        let raw_desc = &docblock[desc_in_doc_start..desc_in_doc_end];

        if let Some(dollar_pos) = raw_desc.find('$') {
            let rest = &raw_desc[dollar_pos..];
            let name_end = rest[1..]
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .map(|i| i + 1)
                .unwrap_or(rest.len());
            if name_end > 1 {
                let name = rest[1..name_end].to_string();
                let file_offset = desc_file_start + dollar_pos as u32;
                results.push((name, file_offset));
            }
        }
    }

    results
}

// ─── Type span emission ─────────────────────────────────────────────────────

/// Emit `SymbolSpan` entries for a type token, splitting unions and
/// intersections and skipping scalars.
/// Build a contiguous type string from a potentially multiline docblock
/// region, starting at `start_in_docblock` (byte offset within the
/// docblock text).
///
/// Returns `(joined_text, offset_map)` where `offset_map[i]` is the byte
/// offset in the original `docblock` that corresponds to byte `i` in
/// `joined_text`.  Continuation-line prefixes (`* `) are stripped so that
/// `split_type_token` / `emit_type_spans` see a clean type string.
fn join_multiline_type(docblock: &str, start_in_docblock: usize) -> (String, Vec<usize>) {
    let mut joined = String::new();
    // offset_map[i] = byte offset in `docblock` for byte `i` in `joined`.
    // We only add the one-past-end sentinel at the very end so that
    // continuation chunks don't shift indices.
    let mut offset_map: Vec<usize> = Vec::new();

    let first_line_rest = &docblock[start_in_docblock..];
    // Take text up to (but not including) the newline on the first line.
    let first_nl = first_line_rest.find('\n').unwrap_or(first_line_rest.len());
    let first_chunk = &first_line_rest[..first_nl];
    for (i, _) in first_chunk.char_indices() {
        offset_map.push(start_in_docblock + i);
    }
    joined.push_str(first_chunk);

    // Check whether the first chunk has unclosed `<`, `(`, or `{`.
    if !crate::util::has_unclosed_delimiters(&joined) {
        // Push one-past-end sentinel.
        offset_map.push(start_in_docblock + first_chunk.len());
        return (joined, offset_map);
    }

    // Consume continuation lines.
    let mut pos = start_in_docblock + first_nl;
    while pos < docblock.len() {
        // Skip the `\n`.
        if docblock.as_bytes().get(pos) == Some(&b'\n') {
            pos += 1;
        }
        if pos >= docblock.len() {
            break;
        }

        let line_end = docblock[pos..]
            .find('\n')
            .map_or(docblock.len(), |p| pos + p);
        let raw_line = &docblock[pos..line_end];

        // Strip the leading `* ` (with optional whitespace before `*`).
        let stripped = raw_line.trim_start();
        if stripped.starts_with("*/") {
            // End of docblock.
            break;
        }
        let content_after_star = if let Some(rest) = stripped.strip_prefix('*') {
            // Skip one optional space after `*`.
            rest.strip_prefix(' ').unwrap_or(rest)
        } else {
            stripped
        };

        // If the continuation line starts with `@`, it's a new tag — stop.
        if content_after_star.trim_start().starts_with('@') {
            break;
        }

        let content_start_in_docblock = pos + (raw_line.len() - content_after_star.len());

        // Append a space to represent the line break in the joined string,
        // mapped to the newline position.
        offset_map.push(pos.saturating_sub(1));
        joined.push(' ');

        for (i, _) in content_after_star.char_indices() {
            offset_map.push(content_start_in_docblock + i);
        }
        joined.push_str(content_after_star);

        pos = line_end;

        if !crate::util::has_unclosed_delimiters(&joined) {
            break;
        }
    }

    // One-past-end sentinel so that `sp.end` lookups work.
    let last_mapped = offset_map.last().copied().unwrap_or(start_in_docblock);
    offset_map.push(last_mapped + 1);

    (joined, offset_map)
}

pub(super) fn emit_type_spans(
    type_token: &str,
    token_file_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) {
    if type_token.is_empty() {
        return;
    }

    // ── Strip PHPStan variance annotations ──────────────────────────
    // Generic type arguments may carry a variance prefix, e.g.
    // `Collection<int, covariant array{customer: Customer}>`.
    // `mago-type-syntax` does not recognise these, so we strip them
    // before parsing and build an offset map so that spans emitted
    // from the cleaned string can be translated back to the original.
    let (cleaned, variance_offset_map) = strip_variance_annotations(type_token);

    // ── Replace PHPStan `*` wildcards in generic positions ──────────
    // PHPStan supports `*` as a bivariant wildcard in generic args,
    // e.g. `Relation<TRelatedModel, *, *>`.  `mago-type-syntax` does
    // not recognise this.  Replace with `mixed` and build an offset
    // map that accounts for the 1→5 character expansion.
    let (effective_cleaned, wildcard_offset_map) = replace_star_wildcards_with_offset_map(&cleaned);
    let effective_token: &str = &effective_cleaned;

    // Parse the type string using mago-type-syntax.  The span we
    // provide starts at 0 so that all AST node offsets are relative
    // to `effective_token`.
    let parse_span = Span::new(
        FileId::zero(),
        Position::new(0),
        Position::new(effective_token.len() as u32),
    );

    let arena = Bump::new();
    let effective_token = arena.alloc_slice_copy(effective_token.as_bytes());
    match mago_type_syntax::parse_str(&arena, parse_span, effective_token) {
        Ok(ty) => {
            let mut local_spans: Vec<SymbolSpan> = Vec::new();
            emit_type_spans_from_ast(&ty, 0, &mut local_spans);
            // Map cleaned-string offsets back to original-string
            // offsets, then shift by token_file_offset.
            //
            // Two offset maps may be active:
            // 1. wildcard_offset_map: effective_token → cleaned (after
            //    variance stripping but before wildcard replacement)
            // 2. variance_offset_map: cleaned → original type_token
            for mut sp in local_spans {
                if let Some(ref map) = wildcard_offset_map {
                    sp.start = map
                        .get(sp.start as usize)
                        .copied()
                        .unwrap_or(sp.start as usize) as u32;
                    sp.end = map.get(sp.end as usize).copied().unwrap_or(sp.end as usize) as u32;
                }
                if let Some(ref map) = variance_offset_map {
                    sp.start = map
                        .get(sp.start as usize)
                        .copied()
                        .unwrap_or(sp.start as usize) as u32;
                    sp.end = map.get(sp.end as usize).copied().unwrap_or(sp.end as usize) as u32;
                }
                sp.start += token_file_offset;
                sp.end += token_file_offset;
                spans.push(sp);
            }
        }
        Err(_) => {
            // Parse failed — fall back to emitting a single span for
            // the whole token if it looks like a navigable class name.
            let trimmed = type_token.trim();
            let base = strip_fqn_prefix(trimmed)
                .split('<')
                .next()
                .unwrap_or(trimmed);
            if is_navigable_type(base) {
                let is_fqn = trimmed.starts_with('\\');
                let name = strip_fqn_prefix(trimmed).to_string();
                spans.push(SymbolSpan {
                    start: token_file_offset,
                    end: token_file_offset + trimmed.len() as u32,
                    kind: SymbolKind::ClassReference {
                        name,
                        is_fqn,
                        context: ClassRefContext::Other,
                    },
                });
            }
        }
    }
}

/// Strip `covariant ` and `contravariant ` prefixes from generic type
/// arguments so that `mago-type-syntax` can parse the type.
///
/// Returns `(cleaned_string, offset_map)`.  When no variance annotations
/// are found, `offset_map` is `None` and `cleaned_string` is the original
/// input (no allocation).  When annotations *are* stripped,
/// `offset_map[i]` gives the byte offset in the original string that
/// corresponds to byte `i` in the cleaned string, plus a one-past-end
/// sentinel.
/// Replace PHPStan `*` wildcards in generic type argument positions with
/// `mixed`, returning the cleaned string and an offset map.
///
/// The offset map translates positions in the cleaned string back to
/// positions in the input string.  When `*` (1 byte) is replaced with
/// `mixed` (5 bytes), all 5 positions in the output map back to the
/// single `*` position in the input.
///
/// Returns `(cleaned, None)` when no wildcards are found (no allocation
/// for the offset map).
fn replace_star_wildcards_with_offset_map(s: &str) -> (String, Option<Vec<usize>>) {
    use crate::php_type::is_generic_wildcard;

    if !s.contains('*') {
        return (s.to_owned(), None);
    }

    let bytes = s.as_bytes();
    let has_generic_wildcard =
        (0..bytes.len()).any(|i| bytes[i] == b'*' && is_generic_wildcard(bytes, i));

    if !has_generic_wildcard {
        return (s.to_owned(), None);
    }

    let mut cleaned = String::with_capacity(s.len() + 16);
    let mut offset_map: Vec<usize> = Vec::with_capacity(s.len() + 32);
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'*' && is_generic_wildcard(bytes, i) {
            // Replace `*` with `mixed` — all 5 output positions map
            // back to the original `*` position.
            for _ in 0.."mixed".len() {
                offset_map.push(i);
            }
            cleaned.push_str("mixed");
            i += 1;
        } else {
            offset_map.push(i);
            cleaned.push(bytes[i] as char);
            i += 1;
        }
    }

    // One-past-end sentinel.
    offset_map.push(i);

    (cleaned, Some(offset_map))
}

fn strip_variance_annotations(s: &str) -> (String, Option<Vec<usize>>) {
    // Fast path: no variance annotations at all.
    if !s.contains("covariant ") && !s.contains("contravariant ") {
        return (s.to_owned(), None);
    }

    let mut cleaned = String::with_capacity(s.len());
    let mut offset_map: Vec<usize> = Vec::with_capacity(s.len() + 1);
    let bytes = s.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        // Only strip variance keywords that appear after `<` or `,`
        // at some nesting depth (i.e. inside generic parameters).
        // We look for the pattern and check whether the preceding
        // non-whitespace character is `<` or `,`.
        let try_strip = |prefix: &str, pos: usize, src: &[u8]| -> bool {
            if pos + prefix.len() > src.len() {
                return false;
            }
            if &src[pos..pos + prefix.len()] != prefix.as_bytes() {
                return false;
            }
            // Check that the preceding non-whitespace is `<` or `,`.
            let mut j = pos;
            while j > 0 {
                j -= 1;
                if !src[j].is_ascii_whitespace() {
                    return src[j] == b'<' || src[j] == b',';
                }
            }
            false
        };

        if try_strip("covariant ", i, bytes) {
            i += "covariant ".len();
        } else if try_strip("contravariant ", i, bytes) {
            i += "contravariant ".len();
        } else {
            offset_map.push(i);
            cleaned.push(bytes[i] as char);
            i += 1;
        }
    }

    // One-past-end sentinel.
    offset_map.push(i);

    (cleaned, Some(offset_map))
}

/// Walk a `mago_type_syntax` AST node and emit [`SymbolSpan`] entries
/// for every navigable type reference (class names, `self`, `static`,
/// `parent`, `$this`).
///
/// `base_offset` is added to every AST-local offset to produce
/// file-level byte positions.
fn emit_type_spans_from_ast(
    ty: &type_ast::Type<'_>,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) {
    use crate::atom::bytes_to_str;
    match ty {
        // ── Composite types ─────────────────────────────────────────
        type_ast::Type::Union(u) => {
            emit_type_spans_from_ast(u.left, base_offset, spans);
            emit_type_spans_from_ast(u.right, base_offset, spans);
        }
        type_ast::Type::Intersection(i) => {
            emit_type_spans_from_ast(i.left, base_offset, spans);
            emit_type_spans_from_ast(i.right, base_offset, spans);
        }
        type_ast::Type::Nullable(n) => {
            emit_type_spans_from_ast(n.inner, base_offset, spans);
        }
        type_ast::Type::Parenthesized(p) => {
            emit_type_spans_from_ast(p.inner, base_offset, spans);
        }

        // ── Named / Reference types ─────────────────────────────────
        type_ast::Type::Reference(r) => {
            let name = bytes_to_str(r.identifier.value);
            let id_start = base_offset + r.identifier.span.start.offset;
            let id_end = base_offset + r.identifier.span.end.offset;

            // Emit a span for the identifier itself.
            emit_identifier_span(name, id_start, id_end, spans);

            // Recurse into generic parameters if present.
            if let Some(params) = &r.parameters {
                emit_generic_params(params, base_offset, spans);
            }
        }

        // ── Array-like types with optional generic parameters ───────
        type_ast::Type::Array(a) => {
            if let Some(params) = &a.parameters {
                emit_generic_params(params, base_offset, spans);
            }
        }
        type_ast::Type::NonEmptyArray(a) => {
            if let Some(params) = &a.parameters {
                emit_generic_params(params, base_offset, spans);
            }
        }
        type_ast::Type::AssociativeArray(a) => {
            if let Some(params) = &a.parameters {
                emit_generic_params(params, base_offset, spans);
            }
        }
        type_ast::Type::List(l) => {
            if let Some(params) = &l.parameters {
                emit_generic_params(params, base_offset, spans);
            }
        }
        type_ast::Type::NonEmptyList(l) => {
            if let Some(params) = &l.parameters {
                emit_generic_params(params, base_offset, spans);
            }
        }
        type_ast::Type::Iterable(i) => {
            if let Some(params) = &i.parameters {
                emit_generic_params(params, base_offset, spans);
            }
        }

        // ── Slice: T[] ──────────────────────────────────────────────
        type_ast::Type::Slice(s) => {
            emit_type_spans_from_ast(s.inner, base_offset, spans);
        }

        // ── Shape types ─────────────────────────────────────────────
        type_ast::Type::Shape(s) => {
            for field in &s.fields {
                emit_type_spans_from_ast(field.value, base_offset, spans);
            }
        }

        // ── Object type (with optional shape) ───────────────────────
        type_ast::Type::Object(o) => {
            if let Some(props) = &o.properties {
                for field in &props.fields {
                    emit_type_spans_from_ast(field.value, base_offset, spans);
                }
            }
        }

        // ── Callable types ──────────────────────────────────────────
        type_ast::Type::Callable(c) => {
            // Emit span for the callable keyword if it's navigable
            // (e.g. `Closure` is a class, `callable` is not).
            let kw_name = bytes_to_str(c.keyword.value);
            let kw_start = base_offset + c.keyword.span.start.offset;
            let kw_end = base_offset + c.keyword.span.end.offset;
            emit_identifier_span(kw_name, kw_start, kw_end, spans);

            // Recurse into parameter types and return type.
            if let Some(spec) = &c.specification {
                for param in &spec.parameters.entries {
                    if let Some(param_type) = &param.parameter_type {
                        emit_type_spans_from_ast(param_type, base_offset, spans);
                    }
                }
                if let Some(ret) = &spec.return_type {
                    emit_type_spans_from_ast(ret.return_type, base_offset, spans);
                }
            }
        }

        // ── Conditional types ───────────────────────────────────────
        type_ast::Type::Conditional(c) => {
            // The subject is a variable ($param) — skip it.
            // Recurse into the condition, then, and otherwise types.
            emit_type_spans_from_ast(c.target, base_offset, spans);
            emit_type_spans_from_ast(c.then, base_offset, spans);
            emit_type_spans_from_ast(c.otherwise, base_offset, spans);
        }

        // ── class-string / interface-string / enum-string / trait-string ─
        type_ast::Type::ClassString(c) => {
            if let Some(param) = &c.parameter {
                emit_type_spans_from_ast(&param.entry.inner, base_offset, spans);
            }
        }
        type_ast::Type::InterfaceString(i) => {
            if let Some(param) = &i.parameter {
                emit_type_spans_from_ast(&param.entry.inner, base_offset, spans);
            }
        }
        type_ast::Type::EnumString(e) => {
            if let Some(param) = &e.parameter {
                emit_type_spans_from_ast(&param.entry.inner, base_offset, spans);
            }
        }
        type_ast::Type::TraitString(t) => {
            if let Some(param) = &t.parameter {
                emit_type_spans_from_ast(&param.entry.inner, base_offset, spans);
            }
        }

        // ── key-of / value-of ───────────────────────────────────────
        type_ast::Type::KeyOf(k) => {
            emit_type_spans_from_ast(&k.parameter.entry.inner, base_offset, spans);
        }
        type_ast::Type::ValueOf(v) => {
            emit_type_spans_from_ast(&v.parameter.entry.inner, base_offset, spans);
        }

        // ── Index access: T[K] ─────────────────────────────────────
        type_ast::Type::IndexAccess(i) => {
            emit_type_spans_from_ast(i.target, base_offset, spans);
            emit_type_spans_from_ast(i.index, base_offset, spans);
        }

        // ── int-mask / int-mask-of ──────────────────────────────────
        type_ast::Type::IntMask(m) => {
            for entry in &m.parameters.entries {
                emit_type_spans_from_ast(&entry.inner, base_offset, spans);
            }
        }
        type_ast::Type::IntMaskOf(m) => {
            emit_type_spans_from_ast(&m.parameter.entry.inner, base_offset, spans);
        }

        // ── properties-of ───────────────────────────────────────────
        type_ast::Type::PropertiesOf(p) => {
            emit_type_spans_from_ast(&p.parameter.entry.inner, base_offset, spans);
        }

        // ── Negated / Posited literals ──────────────────────────────
        type_ast::Type::Negated(_) | type_ast::Type::Posited(_) => {
            // Numeric literals — not navigable.
        }

        // ── Variable ($this) ────────────────────────────────────────
        type_ast::Type::Variable(v) if v.value == b"$this" => {
            let start = base_offset + v.span.start.offset;
            let end = base_offset + v.span.end.offset;
            spans.push(SymbolSpan {
                start,
                end,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::This),
            });
        }
        // Other variables (parameter names leaked from @param) are skipped.
        type_ast::Type::Variable(_) => {}

        // ── Member / Alias references ───────────────────────────────
        type_ast::Type::MemberReference(_) | type_ast::Type::AliasReference(_) => {
            // These are rare PHPStan types — not navigable in our system.
        }

        // ── Keyword types (int, string, bool, void, etc.) ───────────
        // All keyword types are non-navigable *except* `static`, `self`,
        // and `parent` which should produce SelfStaticParent spans.
        type_ast::Type::Mixed(k)
        | type_ast::Type::NonEmptyMixed(k)
        | type_ast::Type::Null(k)
        | type_ast::Type::Void(k)
        | type_ast::Type::Never(k)
        | type_ast::Type::Resource(k)
        | type_ast::Type::ClosedResource(k)
        | type_ast::Type::OpenResource(k)
        | type_ast::Type::True(k)
        | type_ast::Type::False(k)
        | type_ast::Type::Bool(k)
        | type_ast::Type::Float(k)
        | type_ast::Type::Int(k)
        | type_ast::Type::PositiveInt(k)
        | type_ast::Type::NegativeInt(k)
        | type_ast::Type::NonPositiveInt(k)
        | type_ast::Type::NonNegativeInt(k)
        | type_ast::Type::String(k)
        | type_ast::Type::StringableObject(k)
        | type_ast::Type::ArrayKey(k)
        | type_ast::Type::Numeric(k)
        | type_ast::Type::Scalar(k)
        | type_ast::Type::NumericString(k)
        | type_ast::Type::NonEmptyString(k)
        | type_ast::Type::NonEmptyLowercaseString(k)
        | type_ast::Type::LowercaseString(k)
        | type_ast::Type::NonEmptyUppercaseString(k)
        | type_ast::Type::UppercaseString(k)
        | type_ast::Type::TruthyString(k)
        | type_ast::Type::NonFalsyString(k)
        | type_ast::Type::UnspecifiedLiteralInt(k)
        | type_ast::Type::UnspecifiedLiteralString(k)
        | type_ast::Type::UnspecifiedLiteralFloat(k)
        | type_ast::Type::NonEmptyUnspecifiedLiteralString(k) => {
            // `static`, `self`, and `parent` are parsed as keywords by
            // mago but should still produce SelfStaticParent spans.
            let name = bytes_to_str(k.value);
            if let Some(ssp_kind) = self_static_parent_kind(name) {
                let start = base_offset + k.span.start.offset;
                let end = base_offset + k.span.end.offset;
                spans.push(SymbolSpan {
                    start,
                    end,
                    kind: SymbolKind::SelfStaticParent(ssp_kind),
                });
            }
            // All other keywords (int, string, void, etc.) are non-navigable.
        }

        // ── Literal types ───────────────────────────────────────────
        type_ast::Type::LiteralInt(_)
        | type_ast::Type::LiteralFloat(_)
        | type_ast::Type::LiteralString(_) => {
            // Literals are not navigable.
        }

        // ── int range ───────────────────────────────────────────────
        type_ast::Type::IntRange(_) => {
            // int<min, max> — not navigable.
        }

        // ── Catch-all (non_exhaustive) ──────────────────────────────
        _ => {}
    }
}

/// Emit a span for a type identifier (class name, or self/static/parent).
///
/// Checks the `NON_NAVIGABLE` list and emits either a `ClassReference` or
/// `SelfStaticParent` span as appropriate.
fn emit_identifier_span(name: &str, start: u32, end: u32, spans: &mut Vec<SymbolSpan>) {
    // Handle `self`, `static`, `parent` — they're class-like but get
    // a special span kind.
    if let Some(ssp_kind) = self_static_parent_kind(name) {
        spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::SelfStaticParent(ssp_kind),
        });
        return;
    }

    // Check navigability (strips leading `\` for the check).
    let check_name = strip_fqn_prefix(name).trim();
    if is_navigable_type(check_name) {
        let is_fqn = name.starts_with('\\');
        let display_name = strip_fqn_prefix(name).trim().to_string();
        spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::ClassReference {
                name: display_name,
                is_fqn,
                context: ClassRefContext::Other,
            },
        });
    }
}

/// Recurse into generic type parameters (`<T, U, V>`).
fn emit_generic_params(
    params: &type_ast::GenericParameters<'_>,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) {
    for entry in &params.entries {
        emit_type_spans_from_ast(&entry.inner, base_offset, spans);
    }
}

// ─── @template tag extraction ───────────────────────────────────────────────

/// Handle `@template` (and variants) tags whose description has the form:
/// `T of BoundType`
///
/// `desc_start_in_docblock` is the byte offset within `docblock` where
/// the tag's description begins.  This is derived from the structured
/// `description_span` so that file-level offsets are accurate.
///
/// Returns `(name, byte_offset, optional_bound)` so the caller can
/// record a [`super::TemplateParamDef`].
fn extract_template_tag_symbols(
    docblock: &str,
    desc_start_in_docblock: usize,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) -> Option<(String, u32, Option<PhpType>)> {
    let desc = docblock.get(desc_start_in_docblock..)?;
    // Take only the first line of the description (template tags are
    // always single-line).
    let first_line = desc.split('\n').next().unwrap_or(desc);
    let trimmed = first_line.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let leading_ws = first_line.len() - trimmed.len();

    // The first non-whitespace token is the parameter name (e.g. `T`, `TNode`).
    let param_end = trimmed
        .find(|c: char| c.is_whitespace())
        .unwrap_or(trimmed.len());

    let param_name = &trimmed[..param_end];
    let param_file_offset = base_offset + (desc_start_in_docblock + leading_ws) as u32;

    let after_param = &trimmed[param_end..];
    let after_param_trimmed = after_param.trim_start();

    // Check for `of` keyword.
    if !after_param_trimmed.starts_with("of ") && !after_param_trimmed.starts_with("of\t") {
        return Some((param_name.to_string(), param_file_offset, None));
    }

    // Skip `of` and whitespace to get to the bound type.
    let after_of = &after_param_trimmed[2..]; // skip "of"
    let after_of_trimmed = after_of.trim_start();
    if after_of_trimmed.is_empty() {
        return Some((param_name.to_string(), param_file_offset, None));
    }

    // Compute the offset of the bound type within the docblock.
    let bound_offset_in_desc = trimmed.len() - after_of_trimmed.len();
    let bound_start_in_docblock = desc_start_in_docblock + leading_ws + bound_offset_in_desc;

    let (type_token, _remainder) = split_type_token(after_of_trimmed);
    let bound = if !type_token.is_empty() {
        emit_type_spans(
            type_token,
            base_offset + bound_start_in_docblock as u32,
            spans,
        );
        Some(PhpType::parse(type_token))
    } else {
        None
    };

    Some((param_name.to_string(), param_file_offset, bound))
}

// ─── @method tag extraction ─────────────────────────────────────────────────

/// Handle `@method` tags whose description has the form:
/// `[static] ReturnType methodName(ParamType $p, ...)`
///
/// `desc_start_in_docblock` is the byte offset within `docblock` where
/// the tag's description begins.
fn extract_method_tag_symbols(
    docblock: &str,
    desc_start_in_docblock: usize,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) {
    let desc = match docblock.get(desc_start_in_docblock..) {
        Some(d) => d,
        None => return,
    };
    // Take only the first line (method tags are single-line).
    let first_line = desc.split('\n').next().unwrap_or(desc);
    let trimmed = first_line.trim_start();
    if trimmed.is_empty() {
        return;
    }
    let leading_ws = first_line.len() - trimmed.len();

    let mut rest = trimmed;
    let mut rest_offset_in_docblock = desc_start_in_docblock + leading_ws;

    // Skip optional `static` keyword.
    let mut is_static = false;
    if rest.starts_with("static ") || rest.starts_with("static\t") {
        is_static = true;
        let skip = "static".len();
        let after_static = rest[skip..].trim_start();
        let whitespace_len = rest.len() - skip - after_static.len();
        rest_offset_in_docblock += skip + whitespace_len;
        rest = after_static;
    }

    if rest.is_empty() {
        return;
    }

    // Extract return type.
    let (return_type, remainder) = split_type_token(rest);

    // When the method has no explicit return type (e.g. `@method foo()`),
    // `split_type_token` consumes `foo()` as the "type" because the
    // parentheses are treated as a grouped type unit.  Detect this by
    // checking if `return_type` contains `(` and `remainder` is empty.
    // In that case, re-split at the method-name `(`.
    let (return_type, remainder): (&str, &str) =
        if remainder.is_empty() && return_type.contains('(') {
            // The "type" is actually `name(params)`.  Split at the first
            // `(` that follows a valid identifier to separate the method
            // name from its parameters.
            if let Some(paren) = return_type.find('(') {
                let before = &return_type[..paren];
                // Only re-split if the text before `(` looks like a plain
                // identifier (no type operators).
                let is_plain_ident = !before.contains('|')
                    && !before.contains('&')
                    && !before.contains('<')
                    && !before.contains('{')
                    && !before.contains(' ');
                if is_plain_ident {
                    ("", return_type)
                } else {
                    (return_type, remainder)
                }
            } else {
                (return_type, remainder)
            }
        } else {
            (return_type, remainder)
        };

    if !return_type.is_empty() {
        emit_type_spans(
            return_type,
            base_offset + rest_offset_in_docblock as u32,
            spans,
        );
    }

    // After the return type, find the `(` for parameter list.
    if let Some(paren_pos) = remainder.find('(') {
        // Emit MemberDeclaration span for the method name.
        let method_name = remainder[..paren_pos].trim();
        if !method_name.is_empty() {
            // Calculate the absolute offset of the method name within the
            // docblock.  `remainder` is a substring of `rest` that starts
            // after `return_type`.  Its first byte is at:
            //   rest_offset_in_docblock + return_type.len()
            let remainder_start_in_docblock = rest_offset_in_docblock + return_type.len();
            let trimmed_before_paren = remainder[..paren_pos].trim_start();
            let ws_before_name = remainder[..paren_pos].len() - trimmed_before_paren.len();
            let name_start_in_docblock = remainder_start_in_docblock + ws_before_name;
            let name_file_start = base_offset + name_start_in_docblock as u32;
            let name_file_end = name_file_start + method_name.len() as u32;
            spans.push(SymbolSpan {
                start: name_file_start,
                end: name_file_end,
                kind: SymbolKind::MemberDeclaration {
                    name: method_name.to_string(),
                    is_static,
                },
            });
        }
        let close = remainder[paren_pos..].find(')');
        if let Some(close_pos) = close {
            let inner = &remainder[paren_pos + 1..paren_pos + close_pos];
            let inner_offset_in_docblock = rest_offset_in_docblock
                + return_type.len()
                + (remainder.len() - rest[return_type.len()..].len())
                + paren_pos
                + 1;

            // Simple comma-split at depth 0 for parameters.
            let mut depth = 0i32;
            let mut param_start = 0usize;

            for (i, ch) in inner.char_indices() {
                match ch {
                    '<' | '(' | '{' => depth += 1,
                    '>' | ')' | '}' => depth -= 1,
                    ',' if depth == 0 => {
                        let param = inner[param_start..i].trim();
                        emit_method_param_type(
                            param,
                            inner_offset_in_docblock,
                            param_start,
                            base_offset,
                            spans,
                        );
                        param_start = i + 1;
                    }
                    _ => {}
                }
            }
            // Last parameter.
            let param = inner[param_start..].trim();
            emit_method_param_type(
                param,
                inner_offset_in_docblock,
                param_start,
                base_offset,
                spans,
            );
        }
    }
}

// ─── @property tag symbol extraction ───────────────────────────────────────

/// Extract a `MemberDeclaration` span from `@property`, `@property-read`,
/// `@property-write`, and their `@psalm-` equivalents.
///
/// Format: `@property Type $name` or `@property $name` (no type).
/// The emitted `MemberDeclaration` name does **not** include the `$` prefix.
fn extract_property_tag_symbols(
    docblock: &str,
    desc_start_in_docblock: usize,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) {
    if desc_start_in_docblock >= docblock.len() {
        return;
    }
    let raw = &docblock[desc_start_in_docblock..];
    let first_nl = raw.find('\n').unwrap_or(raw.len());
    let first_line = &raw[..first_nl];
    let trimmed = first_line.trim_start();
    if trimmed.is_empty() {
        return;
    }
    let leading_ws = first_line.len() - trimmed.len();
    let adjusted_start = desc_start_in_docblock + leading_ws;

    // Split off the type (if any).  The property name `$name` is in the
    // remainder after the type.
    let (type_token, remainder) = split_type_token(trimmed);

    // Emit type spans for the property type (if present).
    if !type_token.is_empty() {
        emit_type_spans(type_token, base_offset + adjusted_start as u32, spans);
    }

    // Find `$name` in the remainder.
    let remainder_trimmed = remainder.trim_start();
    if remainder_trimmed.is_empty() {
        return;
    }
    let Some(dollar_pos) = remainder_trimmed.find('$') else {
        return;
    };
    let after_dollar = &remainder_trimmed[dollar_pos + '$'.len_utf8()..];
    // The name ends at the next whitespace or end of string.
    let name_end = after_dollar
        .find(|c: char| c.is_whitespace())
        .unwrap_or(after_dollar.len());
    let prop_name = &after_dollar[..name_end];
    if prop_name.is_empty() {
        return;
    }

    // Calculate absolute offset of the property name (excluding `$`).
    let ws_before_remainder = remainder.len() - remainder_trimmed.len();
    let name_start_in_docblock =
        adjusted_start + type_token.len() + ws_before_remainder + dollar_pos + '$'.len_utf8();
    let name_file_start = base_offset + name_start_in_docblock as u32;
    let name_file_end = name_file_start + prop_name.len() as u32;

    spans.push(SymbolSpan {
        start: name_file_start,
        end: name_file_end,
        kind: SymbolKind::MemberDeclaration {
            name: prop_name.to_string(),
            is_static: false,
        },
    });
}

// ─── @see tag symbol extraction ─────────────────────────────────────────────

/// Extract a symbol reference from a structured `@see` tag.
///
/// The tag's `description_span` gives the file-level offset of the
/// reference token.
fn extract_see_tag_symbol(tag: &crate::docblock::parser::TagInfo, spans: &mut Vec<SymbolSpan>) {
    let desc = tag.description.trim();
    if desc.is_empty() {
        return;
    }
    let reference = desc.split_whitespace().next().unwrap_or("");
    if reference.is_empty() {
        return;
    }
    // Compute the file offset: description_span.start + any leading whitespace.
    let raw_desc = &tag.description;
    let leading_ws = raw_desc.len() - raw_desc.trim_start().len();
    let file_offset = tag.description_span.start.offset + leading_ws as u32;
    emit_see_reference(reference, file_offset, spans);
}

/// Scan raw docblock text for inline `{@see ...}` references.
///
/// These appear in free-text (descriptions, not top-level tags) and must
/// be found by scanning the raw string since `mago-docblock` does not
/// expose inline tag positions with sufficient granularity.
fn extract_inline_see_symbols(docblock: &str, base_offset: u32, spans: &mut Vec<SymbolSpan>) {
    let mut search_from = 0;
    while let Some(open) = docblock[search_from..].find("{@see ") {
        let abs_open = search_from + open;
        let after_tag = abs_open + 6; // length of "{@see "
        if let Some(close) = docblock[after_tag..].find('}') {
            let reference = docblock[after_tag..after_tag + close].trim();
            if !reference.is_empty() {
                // The reference token starts after `{@see `.
                let ref_start = after_tag
                    + (docblock[after_tag..after_tag + close].len()
                        - docblock[after_tag..after_tag + close].trim_start().len());
                let first_token = reference.split_whitespace().next().unwrap_or("");
                if !first_token.is_empty() {
                    emit_see_reference(first_token, base_offset + ref_start as u32, spans);
                }
            }
            search_from = after_tag + close + 1;
        } else {
            break;
        }
    }
}

/// Parse a single `@see` reference token and emit the appropriate symbol span.
///
/// Supported forms:
/// - `ClassName` → `ClassReference`
/// - `\Fully\Qualified\Name` → `ClassReference` (FQN)
/// - `ClassName::method()` → `MemberAccess` (method call)
/// - `ClassName::$property` → `MemberAccess` (static property)
/// - `ClassName::CONSTANT` → `MemberAccess` (static constant)
/// - `ClassName#method()` → `MemberAccess` (legacy phpDocumentor instance
///   member fragment syntax)
/// - `function()` → `FunctionCall` (standalone function, no `::` or `#`)
/// - `http://...` / `https://...` → skipped (URLs)
fn emit_see_reference(reference: &str, file_offset: u32, spans: &mut Vec<SymbolSpan>) {
    // Skip URLs.
    if reference.starts_with("http://") || reference.starts_with("https://") {
        return;
    }

    // Strip trailing `()` if present (used on both methods and functions).
    let reference = reference.strip_suffix("()").unwrap_or(reference);

    // `@see` references that contain `\` are almost always fully-qualified
    // class names (e.g. `@see App\Models\User`).  Without a leading `\`,
    // `class_ref_span` would set `is_fqn = false`, causing downstream
    // consumers to prepend the current file's namespace and produce a
    // doubled name like `App\Models\App\Models\User`.  Treat any
    // backslash-containing reference as FQN by prepending `\`.
    // `file_offset` points at the original (pre-prefix) token, so the number
    // of synthetic characters we prepend must be subtracted back out of every
    // offset computed on the lengthened string, otherwise the emitted spans
    // are shifted one byte to the right.
    let owned_reference;
    let (reference, prefix_len) = if reference.contains('\\') && !reference.starts_with('\\') {
        owned_reference = format!("\\{reference}");
        (&owned_reference as &str, 1u32)
    } else {
        (reference, 0u32)
    };

    // Check for `Class::member` form.
    if let Some(sep_pos) = reference.find("::") {
        let class_part = &reference[..sep_pos];
        let member_part = &reference[sep_pos + 2..];

        if class_part.is_empty() || member_part.is_empty() {
            return;
        }

        // Accept regular class names and self/static/parent.
        let clean_class = class_part.trim_start_matches('\\');
        let is_self_like = self_static_parent_kind(clean_class).is_some();
        if !is_self_like && !is_navigable_type(clean_class) {
            return;
        }

        // Emit a ClassReference or SelfStaticParent span for the class
        // portion. Lengths and the separator position were measured on the
        // prefixed string, so undo the synthetic prefix to land on the
        // original source bytes.
        let class_start = file_offset;
        let class_end = file_offset + class_part.len() as u32 - prefix_len;
        // Pass `class_part` (which keeps any leading `\`, including the
        // synthetic one prepended above) so the emitted ClassReference
        // carries the correct `is_fqn` flag. Passing the stripped
        // `clean_class` would drop the flag and make downstream
        // consumers re-prefix the current namespace, doubling it.
        emit_identifier_span(class_part, class_start, class_end, spans);

        // Emit a MemberAccess span for the member portion.
        let member_start = file_offset + sep_pos as u32 + 2 - prefix_len;
        let is_property = member_part.starts_with('$');
        let member_name = if is_property {
            &member_part[1..] // strip $
        } else {
            member_part
        };
        if !member_name.is_empty() {
            let member_end = member_start + member_part.len() as u32;
            spans.push(SymbolSpan {
                start: member_start,
                end: member_end,
                kind: SymbolKind::MemberAccess {
                    subject_text: clean_class.to_string(),
                    member_name: member_name.to_string(),
                    is_static: true,
                    is_method_call: false,
                    is_docblock_reference: true,
                    is_array_callable: false,
                },
            });
        }
    } else if let Some(sep_pos) = reference.find('#') {
        // Legacy phpDocumentor fragment syntax: `Class#member` refers to
        // an instance property or method, unlike `Class::member`.
        let class_part = &reference[..sep_pos];
        let member_part = &reference[sep_pos + 1..];

        if class_part.is_empty() || member_part.is_empty() {
            return;
        }

        let clean_class = class_part.trim_start_matches('\\');
        let is_self_like = self_static_parent_kind(clean_class).is_some();
        if !is_self_like && !is_navigable_type(clean_class) {
            return;
        }

        let class_start = file_offset;
        let class_end = file_offset + class_part.len() as u32 - prefix_len;
        emit_identifier_span(class_part, class_start, class_end, spans);

        let member_start = file_offset + sep_pos as u32 + 1 - prefix_len;
        let member_end = member_start + member_part.len() as u32;
        spans.push(SymbolSpan {
            start: member_start,
            end: member_end,
            kind: SymbolKind::MemberAccess {
                subject_text: clean_class.to_string(),
                member_name: member_part.to_string(),
                is_static: false,
                is_method_call: false,
                is_docblock_reference: true,
                is_array_callable: false,
            },
        });
    } else {
        // No `::` or `#` — either a class name or a standalone function.
        // If it looks like a class (starts with uppercase or `\`),
        // emit as ClassReference; otherwise skip.
        let clean = reference.trim_start_matches('\\');
        let self_like = self_static_parent_kind(clean);
        if clean.is_empty() || (self_like.is_none() && !is_navigable_type(clean)) {
            return;
        }

        if self_like.is_some() {
            let start = file_offset;
            let end = file_offset + reference.len() as u32 - prefix_len;
            emit_identifier_span(clean, start, end, spans);
            return;
        }

        // Class names start with uppercase; function names start with
        // lowercase.  PHP convention, not enforced, but a good heuristic.
        let first_char = clean.chars().next().unwrap_or('a');
        if first_char.is_ascii_uppercase() {
            let start = file_offset;
            let end = file_offset + reference.len() as u32 - prefix_len;
            spans.push(class_ref_span(start, end, reference));
        } else {
            // Lowercase first char — treat as function reference.
            let start = file_offset;
            let end = file_offset + reference.len() as u32 - prefix_len;
            spans.push(SymbolSpan {
                start,
                end,
                kind: SymbolKind::FunctionCall {
                    name: clean.to_string(),
                    is_definition: false,
                },
            });
        }
    }
}

/// Emit a type span for a single parameter in a `@method` tag's parameter list.
///
/// `inner_offset_in_docblock` is the byte offset of the opening `(` + 1
/// within the raw docblock string.  `param_start_in_inner` is the byte
/// offset of this parameter's text within the parenthesized list.
fn emit_method_param_type(
    param: &str,
    inner_offset_in_docblock: usize,
    param_start_in_inner: usize,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) {
    if param.is_empty() {
        return;
    }
    // A parameter looks like `TypeHint $varName` or just `$varName`.
    if let Some(dollar_pos) = param.find('$') {
        let type_part = param[..dollar_pos].trim();
        if !type_part.is_empty() {
            let type_start_in_param = param.find(type_part).unwrap_or(0);
            let (type_token, _) = split_type_token(type_part);
            if !type_token.is_empty() {
                let file_offset = base_offset
                    + (inner_offset_in_docblock + param_start_in_inner + type_start_in_param)
                        as u32;
                emit_type_spans(type_token, file_offset, spans);
            }
        }
    }
}
