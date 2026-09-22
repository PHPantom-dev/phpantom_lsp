//! Docblock symbol extraction helpers for the symbol map.
//!
//! This module scans PHPDoc comment blocks for the symbols the symbol map
//! cares about — type references in `@param`, `@return`, `@var`, `@extends`
//! and friends, the members `@method` and `@property` declare, `@template`
//! parameters, `@see` references, and the `$variables` a docblock names — and
//! emits [`SymbolSpan`] entries with file-level byte offsets.
//!
//! It works from the PHPDoc CST produced by `mago-phpdoc-syntax`, via
//! [`with_docblock_cst`].  The parser is anchored at the docblock's own
//! position in the file, so every type, identifier and variable node already
//! carries the offset we need, including nodes buried inside a type written
//! across continuation lines.  Nothing here re-derives tag structure from the
//! raw text: the grammar has already split type from variable from prose.

use mago_database::file::FileId;
use mago_phpdoc_syntax::cst::r#type as type_ast;
use mago_phpdoc_syntax::cst::{
    AssertPattern, Element, MethodTagValue, PropertyTagValue, Tag, TagValue,
    TemplateTagValue as CstTemplateTagValue, TemplateTagValueVariance, Text, TextSegment, Variable,
};
use mago_span::{HasSpan, Position, Span};
use mago_syntax::cst::*;

use crate::docblock::{TagKind, tag_kind};

use crate::docblock::parser::{type_text, with_docblock_cst};
use crate::php_type::PhpType;
use crate::types::TemplateVariance;

use super::{
    ClassRefContext, DocblockMemberRef, SelfStaticParentKind, SubjectText, SymbolKind, SymbolSpan,
    TemplateParamDef, self_static_parent_kind,
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
    // A hyphenated spelling no keyword covers is a pseudo-type we have no model
    // for, not a class — PHP identifiers cannot contain `-`, so there is
    // nothing to navigate to and nothing to report as missing.
    !crate::php_type::is_keyword_type(base) && !crate::php_type::is_unmodelled_pseudo_type(base)
}

// ─── Span construction helpers ──────────────────────────────────────────────

/// Construct a `ClassReference` `SymbolSpan` from a raw identifier string.
///
/// Detects whether the name is fully-qualified (leading `\`) and sets
/// `is_fqn` accordingly.  The leading `\` is stripped from the stored
/// `name` in all cases.
pub(super) fn class_ref_span(start: u32, end: u32, raw_name: &str) -> SymbolSpan {
    let is_fqn = raw_name.starts_with('\\');
    let name = crate::atom::atom(strip_fqn_prefix(raw_name));
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
    let name = crate::atom::atom(strip_fqn_prefix(raw_name));
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

// ─── Docblock tag scanning ──────────────────────────────────────────────────

/// What a docblock declares beyond the [`SymbolSpan`] entries that
/// [`extract_docblock_symbols`] pushes directly.
#[derive(Debug, Default)]
pub(super) struct DocblockSymbols {
    /// `@template` parameter definitions, as
    /// `(name, offset of the name token, bound, variance)`.
    pub templates: Vec<(String, u32, Option<PhpType>, TemplateVariance)>,
    /// Inline `<T of Bound>` template parameters declared on an individual
    /// `@method` tag.
    ///
    /// Unlike [`Self::templates`], each entry already carries its own scope
    /// (the `@method` tag's own span) rather than deferring to the caller:
    /// the scope must be confined to that one tag, since a different
    /// `@method` tag in the same docblock may reuse the same template name
    /// for something unrelated.
    pub method_templates: Vec<TemplateParamDef>,
    /// The parameters this docblock names: the variable of a `@param` tag, and
    /// the subject of any conditional type (`$strict` in
    /// `($strict is true ? A : B)`).
    ///
    /// Each entry is `(name_without_dollar, offset_of_the_dollar)`.  Callers
    /// turn them into [`SymbolKind::Variable`] spans and `DocblockParam`
    /// definition sites, so rename and find-references cover parameter names
    /// mentioned in docblocks.
    pub param_vars: Vec<(String, u32)>,
    /// The variables an inline `@var Type $name` declares, in the same shape
    /// as [`Self::param_vars`].
    pub var_vars: Vec<(String, u32)>,
    /// The class named by PHPUnit's `@coversDefaultClass`, if this docblock
    /// carries one.  A test class's default flows down to the docblocks of
    /// its methods, where `@covers ::name` then means "that class's method"
    /// rather than "the global function".
    pub covers_default_class: Option<crate::atom::Atom>,
}

/// Where a docblock walk writes what it finds.
struct DocblockSink<'a> {
    spans: &'a mut Vec<SymbolSpan>,
    found: &'a mut DocblockSymbols,
}

/// Scan a docblock for the symbols the symbol map needs and emit
/// `SymbolSpan` entries with file-level byte offsets.
pub(super) fn extract_docblock_symbols(
    docblock: &str,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) -> DocblockSymbols {
    extract_docblock_symbols_covering(docblock, base_offset, spans, None)
}

/// [`extract_docblock_symbols`] for a docblock that sits inside a class
/// carrying `@coversDefaultClass`.
///
/// `inherited_default` is that class's default; a `@coversDefaultClass` in
/// this docblock itself takes precedence over it.
pub(super) fn extract_docblock_symbols_covering(
    docblock: &str,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
    inherited_default: Option<crate::atom::Atom>,
) -> DocblockSymbols {
    let mut found = DocblockSymbols {
        // Read up-front rather than when the tag is walked: PHPUnit does not
        // care whether `@coversDefaultClass` precedes or follows the
        // `@covers ::member` tags it gives a subject to.
        covers_default_class: find_covers_default_class(docblock).or(inherited_default),
        ..DocblockSymbols::default()
    };
    let mut sink = DocblockSink {
        spans,
        found: &mut found,
    };

    with_docblock_cst(docblock, docblock_span(docblock, base_offset), |document| {
        for element in document.elements.iter() {
            match element {
                Element::Tag(tag) => emit_tag_symbols(tag, docblock, base_offset, &mut sink),
                // Free text ahead of the first tag, e.g. `Wraps {@see Foo}.`
                Element::Text(text) => {
                    scan_text_for_inline_see(text, docblock, base_offset, sink.spans);
                }
                Element::Code(_) => {}
            }
        }
    });

    found
}

/// The span a docblock occupies in the file, which anchors the PHPDoc parser
/// so that every node it produces reports a file offset.
fn docblock_span(docblock: &str, base_offset: u32) -> Span {
    Span::new(
        FileId::zero(),
        Position::new(base_offset),
        Position::new(base_offset + docblock.len() as u32),
    )
}

/// Emit the symbols one tag declares.
///
/// Tags the grammar could not parse at all yield nothing: their type text is
/// not a type, so guessing at a class name from it would only produce a
/// reference that resolves to nothing.
fn emit_tag_symbols(tag: &Tag<'_>, docblock: &str, base_offset: u32, sink: &mut DocblockSink<'_>) {
    match &tag.value {
        // ── Tags that lead with a type ──────────────────────────────
        TagValue::Param(value) => {
            emit_type_symbols(value.r#type, sink);
            if let Some(parameter) = value.parameter {
                push_variable_reference(&parameter, &mut sink.found.param_vars);
            }
        }
        TagValue::TypelessParam(value) => {
            push_variable_reference(&value.parameter, &mut sink.found.param_vars);
        }
        TagValue::Return(value) | TagValue::RealReturn(value) => {
            emit_type_symbols(value.r#type, sink);
        }
        TagValue::Var(value) => {
            emit_type_symbols(value.r#type, sink);
            if let Some(variable) = value.variable {
                push_variable_reference(&variable, &mut sink.found.var_vars);
            }
        }
        TagValue::Throws(value) => emit_type_symbols(value.r#type, sink),
        TagValue::Mixin(value) => emit_type_symbols(value.r#type, sink),
        TagValue::Extends(value) => emit_type_symbols(value.r#type, sink),
        TagValue::Implements(value) => emit_type_symbols(value.r#type, sink),
        TagValue::Use(value) => emit_type_symbols(value.r#type, sink),
        TagValue::RequireExtends(value) => emit_type_symbols(value.r#type, sink),
        TagValue::RequireImplements(value) => emit_type_symbols(value.r#type, sink),
        TagValue::Sealed(value) => emit_type_symbols(value.r#type, sink),
        TagValue::Assert(value)
        | TagValue::AssertIfTrue(value)
        | TagValue::AssertIfFalse(value) => {
            if let AssertPattern::Type(asserted) = value.pattern {
                emit_type_symbols(asserted, sink);
            }
        }

        // ── Type alias declarations ─────────────────────────────────
        // The alias name itself is not a class and gets no span, but the
        // definition behind it is an ordinary type, and the class an
        // import names is an ordinary reference to it.
        TagValue::TypeAlias(value) => emit_type_symbols(value.r#type, sink),
        TagValue::TypeAliasImport(value) => {
            let span = value.imported_from.span();
            emit_identifier_span(
                crate::atom::bytes_to_str(value.imported_from.value),
                span.start.offset,
                span.end.offset,
                sink.spans,
            );
        }

        // ── Tags that declare a member or a template parameter ──────
        TagValue::Method(value) => emit_method_tag_symbols(value, docblock, base_offset, sink),
        TagValue::Property(value)
        | TagValue::PropertyRead(value)
        | TagValue::PropertyWrite(value) => emit_property_tag_symbols(value, sink),
        TagValue::Template(value) => emit_template_tag_symbols(value, docblock, base_offset, sink),

        // ── Tags the grammar keeps as free text ─────────────────────
        TagValue::Generic(text) => match tag_kind(tag) {
            TagKind::See => emit_see_tag_symbol(&text.value, docblock, base_offset, sink.spans),
            // `@coversDefaultClass` only ever names a class, so the plain
            // `@see` shape covers it; the default it establishes was read
            // before the walk started.
            TagKind::Covers | TagKind::Uses | TagKind::CoversDefaultClass => {
                let default_class = sink.found.covers_default_class;
                emit_covers_tag_symbol(
                    &text.value,
                    docblock,
                    base_offset,
                    default_class,
                    sink.spans,
                );
            }
            _ => {}
        },
        _ => {}
    }

    // A `{@see ...}` can turn up in the free-text description of any tag
    // shape that has one (`@param Type $x see {@see Foo}`, `@deprecated in
    // favour of {@see Bar}`, ...), not just in a `@see` tag's own value.
    if let Some(description) = tag_description(&tag.value) {
        scan_text_for_inline_see(&description, docblock, base_offset, sink.spans);
    }
}

/// Emit the return type, name and parameter types of a `@method` tag.
fn emit_method_tag_symbols(
    value: &MethodTagValue<'_>,
    docblock: &str,
    base_offset: u32,
    sink: &mut DocblockSink<'_>,
) {
    if let Some(return_type) = value.return_type {
        emit_type_symbols(return_type, sink);
    }

    sink.spans.push(SymbolSpan {
        start: value.name.span.start.offset,
        end: value.name.span.end.offset,
        kind: SymbolKind::MemberDeclaration {
            name: crate::atom::atom_bytes(value.name.value),
            is_static: value.is_static(),
        },
    });

    if let Some(templates) = value.templates {
        // Scoped to this tag's own span (return type through parameter
        // list), not the whole class docblock: a different `@method` tag
        // in the same docblock may declare an unrelated template under
        // the same name.
        let scope = value.span();
        for entry in templates.entries.iter() {
            let (name, name_offset, bound, variance) =
                template_tag_value_parts(&entry.template, docblock, base_offset, sink);
            sink.found.method_templates.push(TemplateParamDef {
                name_offset,
                name,
                bound,
                variance,
                scope_start: scope.start.offset,
                scope_end: scope.end.offset,
            });
        }
    }

    for parameter in value.parameters.entries.iter() {
        if let Some(declared) = parameter.r#type {
            emit_type_symbols(declared, sink);
        }
    }
}

/// Emit the type and the member name of a `@property` tag (or one of its
/// `-read` / `-write` variants).
fn emit_property_tag_symbols(value: &PropertyTagValue<'_>, sink: &mut DocblockSink<'_>) {
    if let Some(declared) = value.r#type {
        emit_type_symbols(declared, sink);
    }

    let Some((name, dollar_offset)) = variable_name_and_offset(&value.variable) else {
        return;
    };
    sink.spans.push(SymbolSpan {
        start: dollar_offset + 1,
        end: dollar_offset + 1 + name.len() as u32,
        kind: SymbolKind::MemberDeclaration {
            name: crate::atom::atom(name),
            is_static: false,
        },
    });
}

/// Record a `@template` declaration and emit the spans of its bound.
fn emit_template_tag_symbols(
    value: &CstTemplateTagValue<'_>,
    docblock: &str,
    base_offset: u32,
    sink: &mut DocblockSink<'_>,
) {
    let parts = template_tag_value_parts(value, docblock, base_offset, sink);
    sink.found.templates.push(parts);
}

/// Read a `TemplateTagValue`'s name, bound and variance, emitting spans for
/// the bound's own type references along the way.
///
/// Shared by `@template` tags and the inline `<T of Bound>` list a
/// `@method` tag may carry — both use the same PHPDoc grammar node.
fn template_tag_value_parts(
    value: &CstTemplateTagValue<'_>,
    docblock: &str,
    base_offset: u32,
    sink: &mut DocblockSink<'_>,
) -> (String, u32, Option<PhpType>, TemplateVariance) {
    let bound = value.bound.map(|bound| {
        emit_type_symbols(bound.r#type, sink);
        PhpType::parse(&type_text(docblock, base_offset, bound.r#type.span()))
    });

    let variance = match value.variance {
        TemplateTagValueVariance::Invariant => TemplateVariance::Invariant,
        TemplateTagValueVariance::Covariant => TemplateVariance::Covariant,
        TemplateTagValueVariance::Contravariant => TemplateVariance::Contravariant,
    };

    (
        crate::atom::bytes_to_str(value.name.value).to_owned(),
        value.name.span.start.offset,
        bound,
        variance,
    )
}

/// Emit the symbol an `@see` tag references.
///
/// The PHPDoc grammar keeps `@see` as free text, so the reference is the first
/// whitespace-delimited token of it; the text node supplies the file offset.
fn emit_see_tag_symbol(
    text: &Text<'_>,
    docblock: &str,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) {
    let start = text.span.start.offset.saturating_sub(base_offset) as usize;
    let end = (text.span.end.offset.saturating_sub(base_offset) as usize).min(docblock.len());
    let Some(raw) = docblock.get(start..end) else {
        return;
    };
    let trimmed = raw.trim_start();
    let Some(reference) = trimmed.split_whitespace().next() else {
        return;
    };

    let offset = text.span.start.offset + (raw.len() - trimmed.len()) as u32;
    emit_see_reference(reference, offset, spans);
}

// ─── PHPUnit coverage metadata ──────────────────────────────────────────────

const COVERS_DEFAULT_CLASS_TAG: &str = "@coversDefaultClass";

/// The class named by a `@coversDefaultClass` tag in `docblock`.
///
/// Read straight off the raw text rather than the PHPDoc CST so that the
/// answer is available before the tag walk begins, whichever order the tags
/// were written in.  The `contains` check keeps docblocks without the tag —
/// which is nearly all of them — to a single substring search.
fn find_covers_default_class(docblock: &str) -> Option<crate::atom::Atom> {
    let index = docblock.find(COVERS_DEFAULT_CLASS_TAG)?;
    let after_tag = &docblock[index + COVERS_DEFAULT_CLASS_TAG.len()..];
    // A tag's value ends at the newline; without this a bare
    // `@coversDefaultClass` would adopt the next line's `*` or tag name.
    let line = after_tag.split(['\n', '\r']).next()?;
    let name = line.split_whitespace().next()?;
    if name.is_empty() || name == "*" {
        return None;
    }
    Some(crate::atom::atom(name))
}

/// Emit the symbol a PHPUnit `@covers` / `@uses` / `@coversDefaultClass` tag
/// references.
///
/// Like `@see`, these tags reach us as free text, so the reference is the
/// first whitespace-delimited token.
fn emit_covers_tag_symbol(
    text: &Text<'_>,
    docblock: &str,
    base_offset: u32,
    default_class: Option<crate::atom::Atom>,
    spans: &mut Vec<SymbolSpan>,
) {
    let start = text.span.start.offset.saturating_sub(base_offset) as usize;
    let end = (text.span.end.offset.saturating_sub(base_offset) as usize).min(docblock.len());
    let Some(raw) = docblock.get(start..end) else {
        return;
    };
    let trimmed = raw.trim_start();
    let Some(reference) = trimmed.split_whitespace().next() else {
        return;
    };

    let offset = text.span.start.offset + (raw.len() - trimmed.len()) as u32;
    // `@covers Foo` and `@covers Foo::bar()` share their emitter with `@see`,
    // so the class reference comes back tagged as an ordinary one.  Retagging
    // whatever this tag just emitted keeps the coverage-specific knowledge in
    // one place instead of threading a context through the `@see` path.
    let first_new = spans.len();
    emit_covers_reference(reference, offset, default_class, spans);
    retag_as_covers_target(&mut spans[first_new..]);
}

/// Mark every reference among `spans` as PHPUnit coverage metadata.
///
/// Besides tagging the class references, this undoes two of the `@see`
/// emitter's assumptions.  A bare lowercase name may be a member of the
/// enclosing class under `@see`, but a coverage target that names no class is
/// a *global* function and PHPUnit spells the test class's own members
/// `::name` instead.  And where `@see` tolerates a target that resolves to
/// nothing, because the tag legally carries prose, coverage metadata names a
/// code unit PHPUnit insists exists.
pub(super) fn retag_as_covers_target(spans: &mut [SymbolSpan]) {
    for span in spans {
        match &mut span.kind {
            SymbolKind::ClassReference { context, .. } => {
                *context = ClassRefContext::CoversTarget;
            }
            SymbolKind::FunctionCall {
                is_docblock_reference,
                ..
            } => *is_docblock_reference = false,
            SymbolKind::MemberAccess { docblock_ref, .. } => {
                *docblock_ref = DocblockMemberRef::Coverage;
            }
            _ => {}
        }
    }
}

/// Emit the spans for one PHPUnit coverage target.
///
/// Everything that names a class (`Foo`, `\App\Foo`, `Foo::bar`) has the same
/// shape as an `@see` reference and is handed to that emitter.  What is unique
/// to `@covers` is the leading-`::` form: `@covers ::name` is a *global
/// function*, unless a `@coversDefaultClass` is in scope, in which case it is
/// that class's method.
fn emit_covers_reference(
    reference: &str,
    file_offset: u32,
    default_class: Option<crate::atom::Atom>,
    spans: &mut Vec<SymbolSpan>,
) {
    // PHPUnit accepts the target with or without a trailing `()`.
    let reference = reference.strip_suffix("()").unwrap_or(reference);

    let Some(member) = reference.strip_prefix("::") else {
        // `Foo::<public>` and friends are PHPUnit 4's member-visibility
        // selectors, not member names.  Keep the class navigable and drop
        // the selector.
        if let Some((class_part, member_part)) = reference.split_once("::")
            && member_part.starts_with('<')
        {
            emit_see_reference(class_part, file_offset, spans);
            return;
        }
        emit_see_reference(reference, file_offset, spans);
        return;
    };

    if member.is_empty() || member.starts_with('<') {
        return;
    }

    let member_start = file_offset + 2;
    let member_end = member_start + member.len() as u32;
    let kind = match default_class {
        Some(class) => SymbolKind::MemberAccess {
            subject_text: SubjectText::owned(class.to_string()),
            member_name: crate::atom::atom(member),
            is_static: true,
            is_method_call: false,
            docblock_ref: DocblockMemberRef::Coverage,
            is_array_callable: false,
            is_nullsafe: false,
        },
        None => SymbolKind::FunctionCall {
            name: crate::atom::atom(member.trim_start_matches('\\')),
            is_definition: false,
            // Without a `@coversDefaultClass` this shape is PHPUnit's spelling
            // for a *global* function, not shorthand for a member of the test
            // class, so the enclosing class must not be consulted for it.
            is_docblock_reference: false,
        },
    };

    spans.push(SymbolSpan {
        start: member_start,
        end: member_end,
        kind,
    });
}

/// The name and `$` offset of a variable token, with the `$` stripped from the
/// name.
fn variable_name_and_offset<'a>(variable: &Variable<'a>) -> Option<(&'a str, u32)> {
    let name = crate::atom::bytes_to_str(variable.value).strip_prefix('$')?;

    (!name.is_empty()).then_some((name, variable.span.start.offset))
}

/// Record a reference to a `$variable` named in a docblock.
fn push_variable_reference(variable: &Variable<'_>, into: &mut Vec<(String, u32)>) {
    if let Some((name, dollar_offset)) = variable_name_and_offset(variable) {
        into.push((name.to_owned(), dollar_offset));
    }
}

// ─── Type span emission ─────────────────────────────────────────────────────

/// Walk a PHPDoc type node and emit [`SymbolSpan`] entries for every navigable
/// type reference (class names, `self`, `static`, `parent`, `$this`), plus a
/// reference for the `$parameter` a conditional type is keyed on.
///
/// Node spans are already file offsets, so nothing has to be adjusted here.
fn emit_type_symbols(ty: &type_ast::Type<'_>, sink: &mut DocblockSink<'_>) {
    use crate::atom::bytes_to_str;
    match ty {
        // ── Composite types ─────────────────────────────────────────
        type_ast::Type::Union(u) => {
            emit_type_symbols(u.left, sink);
            emit_type_symbols(u.right, sink);
        }
        type_ast::Type::Intersection(i) => {
            emit_type_symbols(i.left, sink);
            emit_type_symbols(i.right, sink);
        }
        type_ast::Type::Nullable(n) => {
            emit_type_symbols(n.inner, sink);
        }
        type_ast::Type::Parenthesized(p) => {
            emit_type_symbols(p.inner, sink);
        }

        // ── Named / Reference types ─────────────────────────────────
        type_ast::Type::Reference(r) => {
            let name = crate::php_type::reference_kind_name(&r.kind);
            let id_span = r.kind.span();
            let id_start = id_span.start.offset;
            let id_end = id_span.end.offset;

            emit_identifier_span(name, id_start, id_end, sink.spans);

            if let Some(params) = &r.parameters {
                emit_generic_params(params, sink);
            }
        }

        // ── Array-like types with optional generic parameters ───────
        type_ast::Type::Array(a) => {
            if let Some(params) = &a.parameters {
                emit_generic_params(params, sink);
            }
        }
        type_ast::Type::NonEmptyArray(a) => {
            if let Some(params) = &a.parameters {
                emit_generic_params(params, sink);
            }
        }
        type_ast::Type::AssociativeArray(a) => {
            if let Some(params) = &a.parameters {
                emit_generic_params(params, sink);
            }
        }
        type_ast::Type::List(l) => {
            if let Some(params) = &l.parameters {
                emit_generic_params(params, sink);
            }
        }
        type_ast::Type::NonEmptyList(l) => {
            if let Some(params) = &l.parameters {
                emit_generic_params(params, sink);
            }
        }
        type_ast::Type::Iterable(i) => {
            if let Some(params) = &i.parameters {
                emit_generic_params(params, sink);
            }
        }

        // ── Slice: T[] ──────────────────────────────────────────────
        type_ast::Type::Slice(s) => {
            emit_type_symbols(s.inner, sink);
        }

        // ── Shape types ─────────────────────────────────────────────
        type_ast::Type::Shape(s) => {
            for field in &s.fields {
                emit_type_symbols(field.value, sink);
            }
        }

        // ── Object type (with optional shape) ───────────────────────
        type_ast::Type::Object(o) => {
            if let Some(props) = &o.properties {
                for field in &props.fields {
                    emit_type_symbols(field.value, sink);
                }
            }
        }

        // ── Callable types ──────────────────────────────────────────
        type_ast::Type::Callable(c) => {
            // Emit span for the callable keyword if it's navigable
            // (e.g. `Closure` is a class, `callable` is not).
            let kw_name = bytes_to_str(c.keyword.value);
            let kw_start = c.keyword.span.start.offset;
            let kw_end = c.keyword.span.end.offset;
            emit_identifier_span(kw_name, kw_start, kw_end, sink.spans);

            if let Some(spec) = &c.specification {
                for param in &spec.parameters.entries {
                    if let Some(param_type) = &param.parameter_type {
                        emit_type_symbols(param_type, sink);
                    }
                }
                if let Some(ret) = &spec.return_type {
                    emit_type_symbols(ret.return_type, sink);
                }
            }
        }

        // ── Conditional types ───────────────────────────────────────
        type_ast::Type::Conditional(c) => {
            // `($strict is true ? A : B)` keys the type on a parameter, so the
            // subject is a reference that has to be renamed with it.
            if let type_ast::Type::Variable(subject) = c.subject {
                push_variable_reference(subject, &mut sink.found.param_vars);
            }
            emit_type_symbols(c.target, sink);
            emit_type_symbols(c.then, sink);
            emit_type_symbols(c.r#else, sink);
        }

        // ── class-string / interface-string / enum-string / trait-string ─
        type_ast::Type::ClassString(c) => {
            if let Some(param) = &c.parameter {
                emit_type_symbols(&param.entry.inner, sink);
            }
        }
        type_ast::Type::InterfaceString(i) => {
            if let Some(param) = &i.parameter {
                emit_type_symbols(&param.entry.inner, sink);
            }
        }
        type_ast::Type::EnumString(e) => {
            if let Some(param) = &e.parameter {
                emit_type_symbols(&param.entry.inner, sink);
            }
        }
        type_ast::Type::TraitString(t) => {
            if let Some(param) = &t.parameter {
                emit_type_symbols(&param.entry.inner, sink);
            }
        }

        // ── key-of / value-of ───────────────────────────────────────
        type_ast::Type::KeyOf(k) => {
            emit_type_operand_symbols(&k.parameter.entry.inner, sink);
        }
        type_ast::Type::ValueOf(v) => {
            emit_type_operand_symbols(&v.parameter.entry.inner, sink);
        }

        // ── Index access: T[K] ─────────────────────────────────────
        type_ast::Type::IndexAccess(i) => {
            emit_type_operand_symbols(i.target, sink);
            emit_type_symbols(i.index, sink);
        }

        // ── int-mask / int-mask-of ──────────────────────────────────
        type_ast::Type::IntMask(m) => {
            for entry in &m.parameters.entries {
                emit_int_mask_member_symbols(&entry.inner, sink);
            }
        }
        type_ast::Type::IntMaskOf(m) => {
            emit_int_mask_member_symbols(&m.parameter.entry.inner, sink);
        }

        // ── properties-of ───────────────────────────────────────────
        type_ast::Type::PropertiesOf(p) => {
            emit_type_symbols(&p.parameter.entry.inner, sink);
        }

        // ── Negated / Posited literals ──────────────────────────────
        type_ast::Type::Negated(_) | type_ast::Type::Posited(_) => {
            // Numeric literals — not navigable.
        }

        // ── Variable ($this) ────────────────────────────────────────
        type_ast::Type::ThisVariable(v) => {
            let start = v.span.start.offset;
            let end = v.span.end.offset;
            sink.spans.push(SymbolSpan {
                start,
                end,
                kind: SymbolKind::SelfStaticParent(SelfStaticParentKind::This),
            });
        }
        type_ast::Type::Variable(v) if v.value == b"$this" => {
            let start = v.span.start.offset;
            let end = v.span.end.offset;
            sink.spans.push(SymbolSpan {
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
            let name = bytes_to_str(k.value);
            if let Some(ssp_kind) = self_static_parent_kind(name) {
                let start = k.span.start.offset;
                let end = k.span.end.offset;
                sink.spans.push(SymbolSpan {
                    start,
                    end,
                    kind: SymbolKind::SelfStaticParent(ssp_kind),
                });
            }
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
/// Checks [`is_navigable_type`] and emits either a `ClassReference` or
/// `SelfStaticParent` span as appropriate.
/// Emit the operand of a type operator (`key-of<X>`, `value-of<X>`, `X[K]`).
///
/// A bare name in that position is usually array-typed rather than a class:
/// a constant holding an array literal, a `@template` parameter, or a type
/// alias. It is tagged [`ClassRefContext::TypeOperatorOperand`] so the
/// unknown-class diagnostic reads it that way. A structural operand
/// (`key-of<array<string, Foo>>`) is an ordinary type and its own references
/// are emitted as usual.
fn emit_type_operand_symbols(ty: &type_ast::Type<'_>, sink: &mut DocblockSink<'_>) {
    match ty {
        type_ast::Type::Reference(r) if r.parameters.is_none() => {
            let name = crate::php_type::reference_kind_name(&r.kind);
            let span = r.kind.span();
            emit_identifier_span_in(
                name,
                span.start.offset,
                span.end.offset,
                ClassRefContext::TypeOperatorOperand,
                sink.spans,
            );
        }
        _ => emit_type_symbols(ty, sink),
    }
}

/// Emit the members of an `int-mask<A, B>` / `int-mask-of<A|B>` bitmask.
///
/// The parameters name the int constants that make up the mask, so a bare
/// identifier there is a global constant (`PREG_OFFSET_CAPTURE`), never a
/// class. Like the `self::FOO` spelling of the same thing, it is not
/// navigable, so no span is emitted for it. Anything else (a literal, a
/// nested type) is emitted as usual.
fn emit_int_mask_member_symbols(ty: &type_ast::Type<'_>, sink: &mut DocblockSink<'_>) {
    match ty {
        type_ast::Type::Union(u) => {
            emit_int_mask_member_symbols(u.left, sink);
            emit_int_mask_member_symbols(u.right, sink);
        }
        type_ast::Type::Parenthesized(p) => emit_int_mask_member_symbols(p.inner, sink),
        type_ast::Type::Reference(r) if r.parameters.is_none() => {}
        _ => emit_type_symbols(ty, sink),
    }
}

fn emit_identifier_span(name: &str, start: u32, end: u32, spans: &mut Vec<SymbolSpan>) {
    emit_identifier_span_in(name, start, end, ClassRefContext::Other, spans);
}

fn emit_identifier_span_in(
    name: &str,
    start: u32,
    end: u32,
    context: ClassRefContext,
    spans: &mut Vec<SymbolSpan>,
) {
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
        let display_name = crate::atom::atom(strip_fqn_prefix(name).trim());
        spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::ClassReference {
                name: display_name,
                is_fqn,
                context,
            },
        });
    }
}

/// Recurse into generic type parameters (`<T, U, V>`).
fn emit_generic_params(params: &type_ast::GenericParameters<'_>, sink: &mut DocblockSink<'_>) {
    for entry in &params.entries {
        emit_type_symbols(&entry.inner, sink);
    }
}

// ─── @see tag symbol extraction ─────────────────────────────────────────────

/// Walk a `Text` node's segments for inline `{@see ...}` references.
///
/// Inline tags can turn up in any prose: the free text before the first tag,
/// and the description of every tag shape that has one.  Every one of those
/// is a [`Text`] node, so a single recursive walk over its segments reaches
/// all of them without re-deriving tag structure from raw bytes; a nested
/// `{@see}` (e.g. inside a `{@deprecated ...}` aside) is found by recursing
/// into the inline tag's own description in turn.
fn scan_text_for_inline_see(
    text: &Text<'_>,
    docblock: &str,
    base_offset: u32,
    spans: &mut Vec<SymbolSpan>,
) {
    for segment in text.segments {
        let TextSegment::InlineTag(inline) = segment else {
            continue;
        };

        if tag_kind(inline.tag) == TagKind::See
            && let TagValue::Generic(value) = &inline.tag.value
        {
            emit_see_tag_symbol(&value.value, docblock, base_offset, spans);
        }

        if let Some(nested) = tag_description(&inline.tag.value) {
            scan_text_for_inline_see(&nested, docblock, base_offset, spans);
        }
    }
}

/// The free-text `description` a tag carries, if any; for `@see`/`@link`/and
/// other tags the grammar keeps as raw prose (`TagValue::Generic`/`Invalid`),
/// their whole value stands in for a description.
///
/// Every tag shape that has one keeps it as a `Text` (directly, or behind an
/// `Option` for tags where a description is optional); this is the shared
/// accessor [`scan_text_for_inline_see`] walks to find a `{@see ...}` nested
/// in another tag's prose.
fn tag_description<'a>(value: &TagValue<'a>) -> Option<Text<'a>> {
    match value {
        TagValue::Param(v) => v.description,
        TagValue::TypelessParam(v) => v.description,
        TagValue::ParamOut(v) => v.description,
        TagValue::ParamClosureThis(v) => v.description,
        TagValue::ParamImmediatelyInvokedCallable(v) => v.description,
        TagValue::ParamLaterInvokedCallable(v) => v.description,
        TagValue::Return(v) | TagValue::RealReturn(v) => v.description,
        TagValue::Var(v) => v.description,
        TagValue::Throws(v) => v.description,
        TagValue::Mixin(v) => v.description,
        TagValue::SelfOut(v) => v.description,
        TagValue::Template(v) => v.description,
        TagValue::Extends(v) => v.description,
        TagValue::Implements(v) => v.description,
        TagValue::Use(v) => v.description,
        TagValue::RequireExtends(v) => v.description,
        TagValue::RequireImplements(v) => v.description,
        TagValue::Sealed(v) => v.description,
        TagValue::Inheritors(v) => v.description,
        TagValue::Method(v) => v.description,
        TagValue::Property(v) | TagValue::PropertyRead(v) | TagValue::PropertyWrite(v) => {
            v.description
        }
        TagValue::Assert(v) | TagValue::AssertIfTrue(v) | TagValue::AssertIfFalse(v) => {
            v.description
        }
        TagValue::PureUnlessCallableIsImpure(v) => v.description,
        TagValue::Deprecated(v) => Some(v.description),
        TagValue::Final(v) => Some(v.description),
        TagValue::Internal(v) => Some(v.description),
        TagValue::Api(v) => Some(v.description),
        TagValue::Experimental(v) => Some(v.description),
        TagValue::Pure(v) => Some(v.description),
        TagValue::Impure(v) => Some(v.description),
        TagValue::Readonly(v) => Some(v.description),
        TagValue::MustUse(v) => Some(v.description),
        TagValue::NoNamedArguments(v) => Some(v.description),
        TagValue::NotDeprecated(v) => Some(v.description),
        TagValue::EnumInterface(v) => Some(v.description),
        TagValue::ConsistentConstructor(v) => Some(v.description),
        TagValue::ConsistentTemplates(v) => Some(v.description),
        TagValue::SealProperties(v) => Some(v.description),
        TagValue::NoSealProperties(v) => Some(v.description),
        TagValue::SealMethods(v) => Some(v.description),
        TagValue::NoSealMethods(v) => Some(v.description),
        TagValue::MutationFree(v) => Some(v.description),
        TagValue::ExternalMutationFree(v) => Some(v.description),
        TagValue::SuspendsFiber(v) => Some(v.description),
        TagValue::IgnoreNullableReturn(v) => Some(v.description),
        TagValue::IgnoreFalsableReturn(v) => Some(v.description),
        TagValue::InheritDoc(v) => Some(v.description),
        TagValue::Trace(v) => Some(v.description),
        TagValue::Generic(v) => Some(v.value),
        TagValue::Invalid(v) => Some(v.value),
        TagValue::Where(_) | TagValue::TypeAlias(_) | TagValue::TypeAliasImport(_) => None,
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

    if let Some(sep_pos) = reference.find("::") {
        let class_part = &reference[..sep_pos];
        let member_part = &reference[sep_pos + 2..];

        if class_part.is_empty() || member_part.is_empty() {
            return;
        }

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
        emit_identifier_span_in(
            class_part,
            class_start,
            class_end,
            ClassRefContext::DocblockSee,
            spans,
        );

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
                    subject_text: SubjectText::owned(clean_class.to_string()),
                    member_name: crate::atom::atom(member_name),
                    is_static: true,
                    is_method_call: false,
                    docblock_ref: DocblockMemberRef::See,
                    is_array_callable: false,
                    is_nullsafe: false,
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
        emit_identifier_span_in(
            class_part,
            class_start,
            class_end,
            ClassRefContext::DocblockSee,
            spans,
        );

        let member_start = file_offset + sep_pos as u32 + 1 - prefix_len;
        let member_end = member_start + member_part.len() as u32;
        spans.push(SymbolSpan {
            start: member_start,
            end: member_end,
            kind: SymbolKind::MemberAccess {
                subject_text: SubjectText::owned(clean_class.to_string()),
                member_name: crate::atom::atom(member_part),
                is_static: false,
                is_method_call: false,
                docblock_ref: DocblockMemberRef::See,
                is_array_callable: false,
                is_nullsafe: false,
            },
        });
    } else {
        // No `::` or `#` — either a class name or a standalone function.
        // Bail out unless this is self/static/parent or a navigable type
        // name; the uppercase/lowercase heuristic below then decides
        // which of the two it is.
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
            spans.push(class_ref_span_ctx(
                start,
                end,
                reference,
                ClassRefContext::DocblockSee,
            ));
        } else {
            let start = file_offset;
            let end = file_offset + reference.len() as u32 - prefix_len;
            spans.push(SymbolSpan {
                start,
                end,
                kind: SymbolKind::FunctionCall {
                    name: crate::atom::atom(clean),
                    is_definition: false,
                    is_docblock_reference: true,
                },
            });
        }
    }
}
