//! Parsing adapter for `mago-phpdoc-syntax`.
//!
//! This module bridges our existing docblock extraction code (which works
//! with raw `&str` slices) and the structured `mago_phpdoc_syntax::cst`
//! representation.
//!
//! # Design
//!
//! Most call sites pass a raw docblock string (`&str`) obtained from
//! [`super::tags::get_docblock_text_for_node`].  The adapter provides one
//! entry point, [`parse_docblock`], which creates a short-lived arena,
//! parses the docblock, and returns an owned [`DocblockInfo`] that captures
//! the tag data we need without borrowing from the arena.  That keeps the
//! arena lifetime contained within each call.
//!
//! Each tag keeps its [`TagKind`], its vendor prefix, the raw source text of
//! its value, and — for every tag shape the PHPDoc grammar models — a
//! structured [`TagValueInfo`] snapshot of the pieces the parser already
//! identified.  The raw text is taken straight from the docblock via the
//! tag's span, with the `*` continuation prefixes removed, so extractors
//! that still need the value as written see it exactly as the author typed
//! it.
//!
//! Prefer [`TagValueInfo`] over re-scanning [`TagInfo::description`]: the
//! structured form comes from the real grammar, so it splits types from
//! variables and descriptions the way PHPStan and Psalm do.  Tags the
//! grammar does not model (`@see`, `@link`, `@removed`, …) and tags whose
//! value it could not make sense of arrive as [`TagValueInfo::Unstructured`],
//! and those are the only ones the text scanners still have to handle.

use std::borrow::Cow;

use mago_allocator::LocalArena;
use mago_phpdoc_syntax::PHPDocParser;
use mago_phpdoc_syntax::cst::r#type::Type;
use mago_phpdoc_syntax::cst::{
    AssertPattern, Document, Element, MethodTagValue, TagValue,
    TemplateTagValue as CstTemplateTagValue,
};
use mago_span::{HasSpan, Position, Span};

use super::tag_kind::tag_kind;
use super::type_strings::split_type_token;

pub use super::tag_kind::{TagKind, TagVendor};

/// Owned snapshot of a parsed tag.
///
/// This captures the tag name, kind, vendor prefix, value text, and the
/// structured value as owned data so callers do not need to worry about
/// arena lifetimes.
#[derive(Debug, Clone)]
pub struct TagInfo {
    /// The raw tag name without the `@` (e.g. `"param"`, `"phpstan-return"`,
    /// `"deprecated"`).
    pub name: String,
    /// What the tag means, with any vendor prefix folded away.
    pub kind: TagKind,
    /// The vendor prefix the tag was written with, if any.
    pub vendor: Option<TagVendor>,
    /// The tag's value as written, with `*` continuation prefixes removed.
    /// For `@param string $foo A description` this is
    /// `"string $foo A description"`.
    pub description: String,
    /// The value as the PHPDoc grammar parsed it, or
    /// [`TagValueInfo::Unstructured`] for tags it does not model.
    pub value: TagValueInfo,
    /// The span of the entire tag (from `@` to the end of its value) in the
    /// source file.
    pub span: Span,
    /// The span of just the value portion of the tag.
    pub description_span: Span,
}

/// A tag value as the PHPDoc grammar parsed it.
///
/// Each variant holds the pieces the grammar identified, already interned
/// into owned data.  Type positions keep the author's own text (generic
/// arguments, shapes and callable signatures included) so they can be fed
/// straight to [`crate::php_type::PhpType::parse`].
#[derive(Debug, Clone, Default)]
pub enum TagValueInfo {
    /// A tag the grammar does not model, or one whose value it could not
    /// parse.  Callers fall back to scanning [`TagInfo::description`].
    #[default]
    Unstructured,
    /// A type with an optional variable and description: `@return`, `@var`,
    /// `@param`, `@throws`, `@mixin`, `@extends`, `@property`, …
    Typed(TypedTagValue),
    /// `@template` and its variance variants.
    Template(TemplateTagValue),
    /// `@method`, boxed to keep [`TagInfo`] small.
    Method(Box<MethodTagInfo>),
    /// `@assert` and its `-if-true` / `-if-false` variants.
    Assert(AssertTagInfo),
    /// `@phpstan-type` / `@psalm-type` local alias.
    TypeAlias(TypeAliasTagInfo),
    /// `@phpstan-import-type` / `@psalm-import-type`.
    TypeAliasImport(TypeAliasImportTagInfo),
}

/// A single type, optionally attached to a variable and followed by prose.
#[derive(Debug, Clone, Default)]
pub struct TypedTagValue {
    /// The type as written, or `None` for a typeless `@param $foo`.
    pub type_text: Option<String>,
    /// The variable the type applies to, `$` included.
    pub variable: Option<String>,
    /// Whether the variable was written as `...$foo`.
    pub variadic: bool,
    /// The prose that follows, with `*` continuation prefixes removed.
    pub description: Option<String>,
}

/// A `@template` declaration: `@template T of Bound = Default`.
#[derive(Debug, Clone, Default)]
pub struct TemplateTagValue {
    pub name: String,
    /// The `of` / `as` upper bound, as written.
    pub bound: Option<String>,
    /// The `= Default` type, as written.
    pub default: Option<String>,
}

/// A `@method` declaration.
#[derive(Debug, Clone, Default)]
pub struct MethodTagInfo {
    pub name: String,
    pub is_static: bool,
    /// The return type written before the method name, as written.  The
    /// `methodName(): Type` spelling is not part of the grammar, so it
    /// arrives in [`Self::description`] instead.
    pub return_type: Option<String>,
    /// Method-level `<T of Bound>` template parameters.
    pub templates: Vec<TemplateTagValue>,
    pub parameters: Vec<MethodParamTagInfo>,
    /// Everything after the parameter list.
    pub description: Option<String>,
}

/// One entry in a `@method` parameter list.
#[derive(Debug, Clone, Default)]
pub struct MethodParamTagInfo {
    /// The parameter name, `$` included.
    pub name: String,
    pub type_text: Option<String>,
    pub variadic: bool,
    /// Whether the parameter has a `= default`.
    pub optional: bool,
}

/// An `@assert` declaration: `@assert !Type $subject`.
#[derive(Debug, Clone, Default)]
pub struct AssertTagInfo {
    pub negated: bool,
    /// Whether the tag was written with the `=` modifier (`=Type`,
    /// `!=Type`), which asserts value equality rather than a subtype
    /// relationship.
    pub is_equality: bool,
    /// The asserted type, or `None` for the truthy / falsy / non-empty
    /// patterns, which assert a shape rather than a type.
    pub type_text: Option<String>,
    /// What is asserted about: `$param`, `$param->prop` or
    /// `$param->method()`.
    pub subject: String,
}

/// A `@phpstan-type Alias = Definition` declaration.
#[derive(Debug, Clone, Default)]
pub struct TypeAliasTagInfo {
    pub alias: String,
    pub definition: String,
}

/// A `@phpstan-import-type Alias from Source as Local` declaration.
#[derive(Debug, Clone, Default)]
pub struct TypeAliasImportTagInfo {
    pub imported: String,
    pub from: String,
    /// The `as Local` rename, when present.
    pub local: Option<String>,
}

impl TagValueInfo {
    /// The [`TypedTagValue`] payload, when this tag carries one.
    pub fn as_typed(&self) -> Option<&TypedTagValue> {
        match self {
            TagValueInfo::Typed(value) => Some(value),
            _ => None,
        }
    }

    /// The [`MethodTagInfo`] payload, when this is a `@method` tag the
    /// grammar could parse.
    pub fn as_method(&self) -> Option<&MethodTagInfo> {
        match self {
            TagValueInfo::Method(value) => Some(value),
            _ => None,
        }
    }

    /// The [`TemplateTagValue`] payload, when this is a `@template` tag.
    pub fn as_template(&self) -> Option<&TemplateTagValue> {
        match self {
            TagValueInfo::Template(value) => Some(value),
            _ => None,
        }
    }
}

/// Accessors for the tags that lead with a type (`@param`, `@return`,
/// `@var`, `@property`, `@extends`, …).
///
/// Each falls back to scanning [`Self::description`] when the grammar could
/// not parse the tag, so a half-typed or non-standard annotation still
/// yields whatever can be salvaged from it.
impl TagInfo {
    /// The type this tag declares, as written.
    pub fn type_text(&self) -> Option<Cow<'_, str>> {
        if let TagValueInfo::Typed(value) = &self.value {
            return value.type_text.as_deref().map(Cow::Borrowed);
        }
        match self.fallback_value() {
            Cow::Borrowed(raw) => split_leading_type(raw).map(Cow::Borrowed),
            Cow::Owned(raw) => split_leading_type(&raw).map(|t| Cow::Owned(t.to_owned())),
        }
    }

    /// The variable this tag attaches its type to, `$` included and any
    /// `...` variadic marker dropped.
    pub fn variable(&self) -> Option<Cow<'_, str>> {
        if let TagValueInfo::Typed(value) = &self.value {
            return value.variable.as_deref().map(Cow::Borrowed);
        }
        match self.fallback_value() {
            Cow::Borrowed(raw) => fallback_variable(raw).map(Cow::Borrowed),
            Cow::Owned(raw) => fallback_variable(&raw).map(|v| Cow::Owned(v.to_owned())),
        }
    }

    /// The prose that follows this tag's type and variable.
    pub fn type_description(&self) -> Option<Cow<'_, str>> {
        if let TagValueInfo::Typed(value) = &self.value {
            return value.description.as_deref().map(Cow::Borrowed);
        }
        match self.fallback_value() {
            Cow::Borrowed(raw) => fallback_description(raw).map(Cow::Borrowed),
            Cow::Owned(raw) => fallback_description(&raw).map(|d| Cow::Owned(d.to_owned())),
        }
    }

    /// The raw value folded onto one line, ready for the token scanners.
    fn fallback_value(&self) -> Cow<'_, str> {
        let raw = self.description.trim();
        if raw.contains('\n') {
            Cow::Owned(collapse_newlines(raw))
        } else {
            Cow::Borrowed(raw)
        }
    }
}

/// The first type token of a tag value, minus any trailing sentence
/// punctuation that ran into it.
fn split_leading_type(raw: &str) -> Option<&str> {
    let (token, _) = split_type_token(raw);
    let token = token.trim_end_matches(['.', ',']);
    (!token.is_empty()).then_some(token)
}

/// The `$variable` token that follows the type, if the value has one.
fn fallback_variable(raw: &str) -> Option<&str> {
    let (_, rest) = split_type_token(raw);
    let token = rest.split_whitespace().next()?;
    let token = token.strip_prefix("...").unwrap_or(token);
    token.starts_with('$').then_some(token)
}

/// Everything after the type token and the `$variable` that may follow it.
fn fallback_description(raw: &str) -> Option<&str> {
    let (_, rest) = split_type_token(raw);
    let rest = rest.trim_start();
    let rest = match rest.split_whitespace().next() {
        Some(token) if token.strip_prefix("...").unwrap_or(token).starts_with('$') => {
            rest[token.len()..].trim_start()
        }
        _ => rest,
    };
    (!rest.is_empty()).then_some(rest)
}

/// Owned snapshot of a parsed docblock.
///
/// Contains the free-text description (before the first tag) and all
/// structured tag entries.
#[derive(Debug, Clone)]
pub struct DocblockInfo {
    /// The free-text description that appears before the first `@tag`,
    /// with `*` continuation prefixes removed.  Inline code and inline
    /// tags are kept verbatim, so `` `code` `` and `{@link …}` survive.
    ///
    /// `None` when the docblock has no text before the first tag (e.g.
    /// `/** @return string */`).
    pub description: Option<String>,
    /// All tags found in the docblock, in source order.
    pub tags: Vec<TagInfo>,
}

impl DocblockInfo {
    /// Returns an iterator over tags matching the given [`TagKind`].
    pub fn tags_by_kind(&self, kind: TagKind) -> impl Iterator<Item = &TagInfo> {
        self.tags.iter().filter(move |t| t.kind == kind)
    }

    /// Returns an iterator over tags matching any of the given [`TagKind`]s.
    pub fn tags_by_kinds<'a>(&'a self, kinds: &'a [TagKind]) -> impl Iterator<Item = &'a TagInfo> {
        self.tags.iter().filter(move |t| kinds.contains(&t.kind))
    }

    /// Returns the first tag matching the given [`TagKind`], if any.
    pub fn first_tag_by_kind(&self, kind: TagKind) -> Option<&TagInfo> {
        self.tags_by_kind(kind).next()
    }

    /// Returns tags of `kind` ordered by vendor precedence rather than by
    /// position: `@phpstan-…` first, then `@psalm-…`, then any other vendor
    /// prefix, then the unprefixed form.  Within each group the source order
    /// is preserved.
    ///
    /// PHPStan and Psalm both let a vendor-prefixed tag override the plain
    /// one on the same symbol, so a docblock carrying both `@param` and
    /// `@phpstan-param` for one parameter must resolve to the latter.
    pub fn tags_by_kind_vendor_first(&self, kind: TagKind) -> impl Iterator<Item = &TagInfo> {
        (0..=VENDOR_RANKS).flat_map(move |rank| {
            self.tags_by_kind(kind)
                .filter(move |t| vendor_rank(t) == rank)
        })
    }

    /// Returns the highest-precedence tag of `kind`; see
    /// [`Self::tags_by_kind_vendor_first`].
    pub fn first_tag_by_kind_vendor_first(&self, kind: TagKind) -> Option<&TagInfo> {
        self.tags_by_kind_vendor_first(kind).next()
    }
}

/// Highest rank produced by [`vendor_rank`].
const VENDOR_RANKS: u8 = 3;

/// Sort key that puts the most authoritative vendor prefix first.
pub(crate) fn vendor_rank(tag: &TagInfo) -> u8 {
    match tag.vendor {
        Some(TagVendor::PhpStan) => 0,
        Some(TagVendor::Psalm) => 1,
        Some(_) => 2,
        None => VENDOR_RANKS,
    }
}

/// Parse a raw docblock string (including `/**` and `*/` delimiters) into
/// a [`DocblockInfo`].
///
/// Returns `None` if the string is not a docblock comment.  Tag-level
/// syntax errors do not fail the parse: the malformed tag is kept with its
/// raw text so that partially typed docblocks still yield usable tags.
/// This function never panics.
///
/// # Arguments
///
/// * `docblock` — The full docblock text, e.g. `"/** @return string */"`.
/// * `base_span` — The span in the source file where this docblock starts.
///   When the caller does not have span information (e.g. unit tests that
///   work with standalone strings), pass a zero-offset span.
pub fn parse_docblock(docblock: &str, base_span: Span) -> Option<DocblockInfo> {
    with_docblock_cst(docblock, base_span, |document| {
        collect_tags(document, docblock, base_span.start.offset)
    })
}

/// Parse a docblock and hand the CST to `f`.
///
/// Every span in the CST is a *file* offset, because the parser is anchored
/// at `base_span`: the lexer treats the `*` continuation prefixes as trivia,
/// so an identifier nested inside a type written across several lines still
/// reports where it really sits in the file.  That makes this the entry point
/// for anything that needs source positions rather than text, and it is why
/// the symbol map walks the CST instead of re-scanning tag values.
///
/// The CST borrows from an arena that dies with this call, so it cannot
/// escape the closure.  Returns `None` if the string is not a docblock.
pub fn with_docblock_cst<R>(
    docblock: &str,
    base_span: Span,
    f: impl FnOnce(&Document<'_>) -> R,
) -> Option<R> {
    if !docblock.trim_start().starts_with("/**") {
        return None;
    }

    // `PHPDocParser::parse_with_span` ties the arena borrow and the content
    // borrow to one lifetime, and the caller's bytes already outlive this
    // scope, so they can be handed over directly — no copy into the arena.
    let arena = LocalArena::new();
    let document = PHPDocParser::parse_with_span(&arena, docblock.as_bytes(), base_span);

    Some(f(&document))
}

/// Walk a parsed `Document` and collect all tags into owned [`TagInfo`]
/// values, along with the free-text description that precedes them.
fn collect_tags(document: &Document<'_>, docblock: &str, base_offset: u32) -> DocblockInfo {
    let mut tags = Vec::new();
    let mut description_parts: Vec<String> = Vec::new();
    let mut seen_tag = false;

    for element in document.elements.iter() {
        match element {
            Element::Tag(tag) => {
                seen_tag = true;
                let value_span = value_span(tag, docblock, base_offset);
                tags.push(TagInfo {
                    name: crate::atom::bytes_to_str(tag.name.value).to_owned(),
                    kind: tag_kind(tag),
                    vendor: tag.vendor,
                    description: source_text(docblock, base_offset, value_span),
                    value: tag_value_info(&tag.value, docblock, base_offset),
                    span: tag.span(),
                    description_span: value_span,
                });
            }
            Element::Text(text) if !seen_tag => {
                let part = source_text(docblock, base_offset, text.span);
                if !part.is_empty() {
                    description_parts.push(part);
                }
            }
            _ => {}
        }
    }

    let description = if description_parts.is_empty() {
        None
    } else {
        Some(description_parts.join("\n"))
    };

    DocblockInfo { description, tags }
}

/// Intern the pieces the PHPDoc grammar identified in a tag value.
///
/// Types keep the author's own text: the span is sliced out of the docblock
/// and normalised the way a PHPDoc reader would (continuation `*` prefixes
/// dropped, line breaks folded away), so a multi-line `array{…}` shape comes
/// back as one parseable type string.
fn tag_value_info(value: &TagValue<'_>, docblock: &str, base_offset: u32) -> TagValueInfo {
    /// A type plus a variable — the shape most tag values share.
    macro_rules! typed {
        ($type_text:expr, $variable:expr, $description:expr $(,)?) => {
            TagValueInfo::Typed(TypedTagValue {
                type_text: $type_text,
                variable: $variable,
                variadic: false,
                description: $description,
            })
        };
    }

    let ty = |t: &Type<'_>| Some(type_text(docblock, base_offset, t.span()));

    match value {
        TagValue::Param(value) => TagValueInfo::Typed(TypedTagValue {
            type_text: ty(value.r#type),
            variable: value.parameter.map(|v| variable_text(v.value)),
            variadic: value.is_variadic(),
            description: value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        }),
        TagValue::TypelessParam(value) => TagValueInfo::Typed(TypedTagValue {
            type_text: None,
            variable: Some(variable_text(value.parameter.value)),
            variadic: value.is_variadic(),
            description: value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        }),
        TagValue::ParamOut(value) => typed!(
            ty(value.r#type),
            Some(variable_text(value.parameter.value)),
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::ParamClosureThis(value) => typed!(
            ty(value.r#type),
            Some(variable_text(value.parameter.value)),
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::ParamImmediatelyInvokedCallable(value) => typed!(
            None,
            Some(variable_text(value.parameter.value)),
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::ParamLaterInvokedCallable(value) => typed!(
            None,
            Some(variable_text(value.parameter.value)),
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Return(value) | TagValue::RealReturn(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Var(value) => typed!(
            ty(value.r#type),
            value.variable.map(|v| variable_text(v.value)),
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Throws(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Mixin(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::SelfOut(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Extends(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Implements(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Use(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::RequireExtends(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::RequireImplements(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Sealed(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Inheritors(value) => typed!(
            ty(value.r#type),
            None,
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Property(value)
        | TagValue::PropertyRead(value)
        | TagValue::PropertyWrite(value) => typed!(
            value.r#type.and_then(ty),
            Some(variable_text(value.variable.value)),
            value
                .description
                .map(|d| source_text(docblock, base_offset, d.span)),
        ),
        TagValue::Template(value) => {
            TagValueInfo::Template(template_tag_value(value, docblock, base_offset))
        }
        TagValue::Method(value) => {
            TagValueInfo::Method(Box::new(method_tag_info(value, docblock, base_offset)))
        }
        TagValue::Assert(value)
        | TagValue::AssertIfTrue(value)
        | TagValue::AssertIfFalse(value) => TagValueInfo::Assert(AssertTagInfo {
            negated: value.is_negated(),
            is_equality: value.is_equality(),
            type_text: match value.pattern {
                AssertPattern::Type(t) => ty(t),
                _ => None,
            },
            subject: source_text(docblock, base_offset, value.subject.span()),
        }),
        TagValue::TypeAlias(value) => TagValueInfo::TypeAlias(TypeAliasTagInfo {
            alias: crate::atom::bytes_to_str(value.alias.value).to_owned(),
            definition: type_text(docblock, base_offset, value.r#type.span()),
        }),
        TagValue::TypeAliasImport(value) => TagValueInfo::TypeAliasImport(TypeAliasImportTagInfo {
            imported: crate::atom::bytes_to_str(value.imported_alias.value).to_owned(),
            from: crate::atom::bytes_to_str(value.imported_from.value).to_owned(),
            local: value
                .imported_as
                .map(|as_clause| crate::atom::bytes_to_str(as_clause.local.value).to_owned()),
        }),
        // Tags with no value worth modelling (`@deprecated`, `@pure`, …),
        // tags the grammar does not know, and tags it could not parse.
        _ => TagValueInfo::Unstructured,
    }
}

/// Intern a `@template` declaration.
fn template_tag_value(
    value: &CstTemplateTagValue<'_>,
    docblock: &str,
    base_offset: u32,
) -> TemplateTagValue {
    TemplateTagValue {
        name: crate::atom::bytes_to_str(value.name.value).to_owned(),
        bound: value
            .bound
            .map(|bound| type_text(docblock, base_offset, bound.r#type.span())),
        default: value
            .default
            .map(|default| type_text(docblock, base_offset, default.r#type.span())),
    }
}

/// Intern a `@method` declaration, including its inline template list and
/// parameter list.
fn method_tag_info(value: &MethodTagValue<'_>, docblock: &str, base_offset: u32) -> MethodTagInfo {
    MethodTagInfo {
        name: crate::atom::bytes_to_str(value.name.value).to_owned(),
        is_static: value.is_static(),
        return_type: value
            .return_type
            .map(|t| type_text(docblock, base_offset, t.span())),
        templates: value
            .templates
            .map(|list| {
                list.entries
                    .iter()
                    .map(|entry| template_tag_value(&entry.template, docblock, base_offset))
                    .collect()
            })
            .unwrap_or_default(),
        parameters: value
            .parameters
            .entries
            .iter()
            .map(|param| MethodParamTagInfo {
                name: variable_text(param.parameter.value),
                type_text: param
                    .r#type
                    .map(|t| type_text(docblock, base_offset, t.span())),
                variadic: param.is_variadic(),
                optional: param.is_optional(),
            })
            .collect(),
        description: value
            .description
            .map(|d| source_text(docblock, base_offset, d.span)),
    }
}

/// A variable token's text, `$` included.
fn variable_text(value: &[u8]) -> String {
    crate::atom::bytes_to_str(value).to_owned()
}

/// Slice a type out of the docblock and fold it onto one line.
///
/// A type written across several lines carries the `*` continuation prefix
/// and the source indentation; both have to go before the string can be
/// handed to the type parser.
pub(crate) fn type_text(docblock: &str, base_offset: u32, span: Span) -> String {
    let raw = source_text(docblock, base_offset, span);
    if raw.contains('\n') {
        collapse_newlines(&raw)
    } else {
        raw
    }
}

/// The span of a tag's value: everything from the first byte after the tag
/// name to the end of what the parser recognised.
///
/// This deliberately does not use `TagValue`'s own span.  A `TagValue` spans
/// only the parts the parser understood, so leading modifiers it consumed
/// before giving up (the `=!` in `@phpstan-assert =!Foo $x`) and anything it
/// declined to model at all would drop out of the text.  Taking the region
/// from the tag name instead yields the value exactly as written, which is
/// what the extractors re-parse.
pub(crate) fn value_span(
    tag: &mago_phpdoc_syntax::cst::Tag<'_>,
    docblock: &str,
    base_offset: u32,
) -> Span {
    let name_end = tag.name.span.end;
    let end = tag.value.span().end;
    let start = if end.offset > name_end.offset {
        skip_value_gap(docblock, base_offset, name_end, end)
    } else {
        name_end
    };

    Span::new(tag.name.span.file_id, start, end.max(start))
}

/// Advance past the whitespace (and `*` line prefixes) that separate a tag
/// name from its value, without running past `end`.
fn skip_value_gap(docblock: &str, base_offset: u32, from: Position, end: Position) -> Position {
    let limit = (end.offset.saturating_sub(base_offset) as usize).min(docblock.len());
    let mut offset = (from.offset.saturating_sub(base_offset) as usize).min(limit);
    let bytes = docblock.as_bytes();

    // Horizontal whitespace, then — if the value continues on a later line —
    // the newline, its indentation, and the single `*` that prefixes it.
    loop {
        while offset < limit && matches!(bytes[offset], b' ' | b'\t') {
            offset += 1;
        }
        if offset < limit && matches!(bytes[offset], b'\n' | b'\r') {
            offset += 1;
            while offset < limit && matches!(bytes[offset], b' ' | b'\t' | b'\r') {
                offset += 1;
            }
            if offset < limit && bytes[offset] == b'*' {
                offset += 1;
            }
            continue;
        }
        break;
    }

    Position::new(base_offset + offset as u32)
}

/// Return the docblock text covered by `span`, with the `*` continuation
/// prefix removed from every line after the first.
///
/// Spans are file-relative, so `base_offset` (where the docblock starts in
/// the file) is subtracted first.  Out-of-range spans yield an empty
/// string rather than panicking.
fn source_text(docblock: &str, base_offset: u32, span: Span) -> String {
    let start = span.start.offset.saturating_sub(base_offset) as usize;
    let end = (span.end.offset.saturating_sub(base_offset) as usize).min(docblock.len());
    if start >= end || !docblock.is_char_boundary(start) || !docblock.is_char_boundary(end) {
        return String::new();
    }

    strip_line_prefixes(&docblock[start..end])
}

/// Remove the leading `*` (and one following space) from every line but the
/// first, the way a PHPDoc reader would.
///
/// The first line never carries a prefix, because a span always starts at
/// real content.  Lines that do not have an asterisk are left alone.
fn strip_line_prefixes(raw: &str) -> String {
    if !raw.contains('\n') {
        return raw.to_owned();
    }

    let mut out = String::with_capacity(raw.len());
    for (index, line) in raw.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        if index == 0 {
            out.push_str(line.trim_end());
            continue;
        }
        let body = line.trim_start_matches([' ', '\t']);
        let body = match body.strip_prefix('*') {
            // A run of `*` only ever means one prefix marker; anything
            // after the first is content.
            Some(rest) => rest.strip_prefix([' ', '\t']).unwrap_or(rest),
            None => line,
        };
        out.push_str(body.trim_end());
    }
    out
}

/// Collapse `\n` (and any surrounding horizontal whitespace) into a
/// single space.
///
/// Multi-line tag values keep their newlines, but continuation lines may
/// carry leading whitespace from the source indentation.  This helper
/// reproduces the behaviour of a line-by-line scanner that trimmed each
/// line before joining with a space.
pub fn collapse_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            // Trim trailing whitespace already appended
            let trimmed_len = out.trim_end().len();
            out.truncate(trimmed_len);
            // Skip leading whitespace on the next line
            while chars.peek().is_some_and(|&ch| ch == ' ' || ch == '\t') {
                chars.next();
            }
            // Decide whether a separating space is needed.  Skip the
            // space when the last emitted character is a structural
            // opener (`<`, `{`, `(`) or when the next character is a
            // structural closer (`>`, `}`, `)`, `,`, `:`) — these
            // tokens are already unambiguous without whitespace and
            // the old line-by-line scanner never inserted spaces in
            // these positions.
            let last_ch = out.chars().last();
            let next_ch = chars.peek().copied();
            let skip_space = matches!(last_ch, Some('<' | '{' | '('))
                || matches!(next_ch, Some('>' | '}' | ')'));
            if !out.is_empty() && !out.ends_with(' ') && !skip_space {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse a raw docblock string into a [`DocblockInfo`] with a zero-offset span.
///
/// This is the standard entry point for all tag extraction functions that
/// receive a raw `&str` docblock.  The span is set to cover the entire
/// string starting at offset 0, which is correct for standalone extraction
/// (the spans are only meaningful when the caller needs source positions).
///
/// Returns `None` if the string is not a docblock.
pub fn parse_docblock_for_tags(docblock: &str) -> Option<DocblockInfo> {
    use mago_database::file::FileId;
    use mago_span::Position;

    let span = Span::new(
        FileId::zero(),
        Position::new(0),
        Position::new(docblock.len() as u32),
    );
    parse_docblock(docblock, span)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_return_tag() {
        let doc = "/** @return string */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::Return);
        assert_eq!(info.tags[0].description, "string");
    }

    #[test]
    fn parse_multiple_tags() {
        let doc = r#"/**
         * @param string $name The name
         * @param int $age The age
         * @return bool
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 3);

        assert_eq!(info.tags[0].kind, TagKind::Param);
        assert_eq!(info.tags[0].description, "string $name The name");

        assert_eq!(info.tags[1].kind, TagKind::Param);
        assert_eq!(info.tags[1].description, "int $age The age");

        assert_eq!(info.tags[2].kind, TagKind::Return);
        assert_eq!(info.tags[2].description, "bool");
    }

    #[test]
    fn parse_deprecated_tag_bare() {
        let doc = "/** @deprecated */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Deprecated)
            .expect("should have deprecated");
        assert_eq!(tag.description, "");
    }

    #[test]
    fn parse_deprecated_tag_with_message() {
        let doc = "/** @deprecated Use newMethod() instead */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Deprecated)
            .expect("should have deprecated");
        assert_eq!(tag.description, "Use newMethod() instead");
    }

    #[test]
    fn parse_mixin_tag() {
        let doc = "/** @mixin \\App\\Models\\User */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Mixin)
            .expect("should have mixin");
        assert_eq!(tag.description, "\\App\\Models\\User");
    }

    #[test]
    fn parse_throws_tag() {
        let doc = "/** @throws \\InvalidArgumentException When input is bad */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Throws)
            .expect("should have throws");
        assert_eq!(
            tag.description,
            "\\InvalidArgumentException When input is bad"
        );
    }

    #[test]
    fn parse_var_tag() {
        let doc = "/** @var array<int, string> $items */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Var)
            .expect("should have var");
        assert_eq!(tag.description, "array<int, string> $items");
    }

    #[test]
    fn parse_see_tag() {
        let doc = "/** @see MyClass::method() */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::See)
            .expect("should have see");
        assert_eq!(tag.description, "MyClass::method()");
    }

    #[test]
    fn vendor_prefixes_fold_into_one_kind() {
        let doc = r#"/**
         * @phpstan-assert string $value
         * @psalm-assert-if-true non-empty-string $value
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags[0].kind, TagKind::Assert);
        assert_eq!(info.tags[0].vendor, Some(TagVendor::PhpStan));
        assert_eq!(info.tags[1].kind, TagKind::AssertIfTrue);
        assert_eq!(info.tags[1].vendor, Some(TagVendor::Psalm));
    }

    #[test]
    fn tags_by_kind_filters_correctly() {
        let doc = r#"/**
         * @param string $a
         * @return int
         * @param bool $b
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");

        let params: Vec<_> = info.tags_by_kind(TagKind::Param).collect();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].description, "string $a");
        assert_eq!(params[1].description, "bool $b");

        let returns: Vec<_> = info.tags_by_kind(TagKind::Return).collect();
        assert_eq!(returns.len(), 1);
    }

    #[test]
    fn vendor_prefixed_tags_share_a_kind() {
        let doc = r#"/**
         * @phpstan-assert int $x
         * @psalm-assert string $y
         * @param bool $z
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");

        let asserts: Vec<_> = info.tags_by_kind(TagKind::Assert).collect();
        assert_eq!(asserts.len(), 2);
    }

    #[test]
    fn first_tag_prefers_the_vendor_variant() {
        let doc = r#"/**
         * @return string
         * @psalm-return non-empty-string
         * @phpstan-return literal-string
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind_vendor_first(TagKind::Return)
            .expect("should have return");
        assert_eq!(tag.description, "literal-string");
    }

    // ── Structured tag values ───────────────────────────────────────

    #[test]
    fn param_tag_splits_type_variable_and_description() {
        let doc = "/** @param int|string $a description with a $dollar */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let value = info.tags[0].value.as_typed().expect("structured param");
        assert_eq!(value.type_text.as_deref(), Some("int|string"));
        assert_eq!(value.variable.as_deref(), Some("$a"));
        assert_eq!(
            value.description.as_deref(),
            Some("description with a $dollar")
        );
    }

    #[test]
    fn variadic_param_reports_the_bare_variable() {
        let doc = "/** @param string ...$rest */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let value = info.tags[0].value.as_typed().expect("structured param");
        assert_eq!(value.variable.as_deref(), Some("$rest"));
        assert!(value.variadic);
    }

    #[test]
    fn typeless_param_has_no_type() {
        let doc = "/** @param $foo */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let value = info.tags[0].value.as_typed().expect("structured param");
        assert_eq!(value.type_text, None);
        assert_eq!(value.variable.as_deref(), Some("$foo"));
    }

    #[test]
    fn multiline_shape_type_is_folded_onto_one_line() {
        let doc = "/**\n * @param array{\n *   name: string,\n *   age: int\n * } $data desc\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let value = info.tags[0].value.as_typed().expect("structured param");
        assert_eq!(
            value.type_text.as_deref(),
            Some("array{name: string, age: int}")
        );
        assert_eq!(value.variable.as_deref(), Some("$data"));
        assert_eq!(value.description.as_deref(), Some("desc"));
    }

    #[test]
    fn return_description_excludes_the_type() {
        let doc = "/** @return array an array of everything */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let value = info.tags[0].value.as_typed().expect("structured return");
        assert_eq!(value.type_text.as_deref(), Some("array"));
        assert_eq!(value.description.as_deref(), Some("an array of everything"));
    }

    #[test]
    fn template_tag_reports_bound_and_default() {
        let doc = "/** @template T of array<int, string> = array{} */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let value = info.tags[0]
            .value
            .as_template()
            .expect("structured template");
        assert_eq!(value.name, "T");
        assert_eq!(value.bound.as_deref(), Some("array<int, string>"));
        assert_eq!(value.default.as_deref(), Some("array{}"));
    }

    #[test]
    fn method_tag_reports_signature_pieces() {
        let doc = "/** @method static TVal get<TVal of mixed>(TVal $default = null) desc */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let value = info.tags[0].value.as_method().expect("structured method");
        assert_eq!(value.name, "get");
        assert!(value.is_static);
        assert_eq!(value.return_type.as_deref(), Some("TVal"));
        assert_eq!(value.templates.len(), 1);
        assert_eq!(value.templates[0].bound.as_deref(), Some("mixed"));
        assert_eq!(value.parameters.len(), 1);
        assert_eq!(value.parameters[0].name, "$default");
        assert!(value.parameters[0].optional);
        assert_eq!(value.description.as_deref(), Some("desc"));
    }

    #[test]
    fn assert_tag_reports_negation_and_subject() {
        let doc = "/**\n * @phpstan-assert !string $value\n * @psalm-assert Type $this->prop\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");

        let TagValueInfo::Assert(first) = &info.tags[0].value else {
            panic!("expected a structured assert: {:?}", info.tags[0].value);
        };
        assert!(first.negated);
        assert_eq!(first.type_text.as_deref(), Some("string"));
        assert_eq!(first.subject, "$value");

        let TagValueInfo::Assert(second) = &info.tags[1].value else {
            panic!("expected a structured assert: {:?}", info.tags[1].value);
        };
        assert!(!second.negated);
        assert_eq!(second.subject, "$this->prop");
    }

    #[test]
    fn type_alias_tags_report_their_pieces() {
        let doc = "/**\n * @phpstan-type Money = array{amount: int}\n * @phpstan-import-type Id from \\App\\Calc as Key\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");

        let TagValueInfo::TypeAlias(alias) = &info.tags[0].value else {
            panic!("expected a structured type alias: {:?}", info.tags[0].value);
        };
        assert_eq!(alias.alias, "Money");
        assert_eq!(alias.definition, "array{amount: int}");

        let TagValueInfo::TypeAliasImport(import) = &info.tags[1].value else {
            panic!("expected a structured import: {:?}", info.tags[1].value);
        };
        assert_eq!(import.imported, "Id");
        assert_eq!(import.from, "\\App\\Calc");
        assert_eq!(import.local.as_deref(), Some("Key"));
    }

    #[test]
    fn unparseable_tag_falls_back_to_scanning_the_raw_value() {
        // Stacked assertion modifiers are not part of the grammar, so the
        // tag arrives unstructured and the accessors scan the text.
        let doc = "/** @phpstan-assert =!Foo $x */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert!(matches!(info.tags[0].value, TagValueInfo::Unstructured));
        assert_eq!(info.tags[0].type_text().as_deref(), Some("=!Foo"));
        assert_eq!(info.tags[0].variable().as_deref(), Some("$x"));
    }

    #[test]
    fn accessors_read_the_structured_value_when_there_is_one() {
        let doc = "/** @property-read Collection<int, Model> $models the models */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = &info.tags[0];
        assert_eq!(tag.type_text().as_deref(), Some("Collection<int, Model>"));
        assert_eq!(tag.variable().as_deref(), Some("$models"));
        assert_eq!(tag.type_description().as_deref(), Some("the models"));
    }

    #[test]
    fn invalid_docblock_returns_none() {
        assert!(parse_docblock_for_tags("/* not a docblock */").is_none());
        assert!(parse_docblock_for_tags("// not a docblock").is_none());
        assert!(parse_docblock_for_tags("").is_none());
    }

    #[test]
    fn parse_template_tags() {
        let doc = r#"/**
         * @template T
         * @template-covariant TValue of object
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");

        let templates: Vec<_> = info
            .tags_by_kinds(&[TagKind::Template, TagKind::TemplateCovariant])
            .collect();
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].kind, TagKind::Template);
        assert_eq!(templates[0].description, "T");
        assert_eq!(templates[1].kind, TagKind::TemplateCovariant);
        assert_eq!(templates[1].description, "TValue of object");
    }

    #[test]
    fn parse_property_tags() {
        let doc = r#"/**
         * @property string $name
         * @property-read int $id
         * @property-write bool $active
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");

        assert_eq!(info.tags.len(), 3);
        assert_eq!(info.tags[0].kind, TagKind::Property);
        assert_eq!(info.tags[1].kind, TagKind::PropertyRead);
        assert_eq!(info.tags[2].kind, TagKind::PropertyWrite);
    }

    #[test]
    fn parse_method_tag() {
        let doc = "/** @method static Builder query() */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Method)
            .expect("should have method");
        assert_eq!(tag.description, "static Builder query()");
    }

    #[test]
    fn parse_multiline_param_type() {
        let doc = r#"/**
         * @param array{
         *   name: string,
         *   age: int
         * } $data
         */"#;
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Param)
            .expect("should have param");
        assert!(tag.description.contains("$data"));
        assert!(tag.description.contains("name: string"));
    }

    #[test]
    fn parse_link_tag() {
        let doc = "/** @link https://php.net/array_map */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Link)
            .expect("should have link");
        assert_eq!(tag.description, "https://php.net/array_map");
    }

    #[test]
    fn parse_extends_tag() {
        let doc = "/** @extends Collection<int, User> */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Extends)
            .expect("should have extends");
        assert_eq!(tag.description, "Collection<int, User>");
    }

    #[test]
    fn parse_phpstan_type_tag() {
        let doc = "/** @phpstan-type Money = array{amount: int, currency: string} */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::TypeAlias)
            .expect("should have type");
        assert!(tag.description.contains("Money"));
    }

    #[test]
    fn parse_phpstan_import_type_tag() {
        let doc = "/** @phpstan-import-type Money from PriceCalculator */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::TypeAliasImport)
            .expect("should have import-type");
        assert!(tag.description.contains("Money"));
        assert!(tag.description.contains("PriceCalculator"));
    }

    #[test]
    fn parse_tag_with_two_spaces_after_asterisk() {
        // A line written with two spaces between the asterisk and the tag
        // must parse exactly like one written with a single space.
        let doc = "/**\n *  @param string $foo A description\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1, "tag should not be dropped");
        assert_eq!(info.tags[0].kind, TagKind::Param);
        assert_eq!(info.tags[0].description, "string $foo A description");
    }

    #[test]
    fn parse_import_type_with_two_spaces_after_asterisk() {
        let doc = "/**\n *  @phpstan-import-type Money from PriceCalculator\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::TypeAliasImport)
            .expect("import-type must still be found with two leading spaces");
        assert!(tag.description.contains("Money"));
        assert!(tag.description.contains("PriceCalculator"));
    }

    #[test]
    fn tag_spans_track_the_source_offset() {
        // Spans must point at the real source text, so the extra space in
        // the second docblock shifts the value span by exactly one byte.
        let one = "/**\n * @return string The value\n */";
        let two = "/**\n *  @return string The value\n */";
        let info_one = parse_docblock_for_tags(one).expect("should parse");
        let info_two = parse_docblock_for_tags(two).expect("should parse");
        assert_eq!(
            info_two.tags[0].description_span.start.offset,
            info_one.tags[0].description_span.start.offset + 1,
        );
    }

    #[test]
    fn indented_code_example_is_not_treated_as_tag() {
        // An indented code block inside a description (which does not
        // start with `@` after the asterisk) must be left untouched.
        let doc = "/**\n * Example:\n *\n *     $x = compute();\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert!(info.tags.is_empty(), "no tags in this docblock");
    }

    #[test]
    fn parse_param_closure_this_tag() {
        let doc = "/** @param-closure-this \\App\\Route $callback */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::ParamClosureThis)
            .expect("should have param-closure-this");
        assert!(tag.description.contains("\\App\\Route"));
        assert!(tag.description.contains("$callback"));
    }

    #[test]
    fn phpstan_extends_tag_is_an_extends_tag() {
        let doc = "/**\n * @phpstan-extends Collection<int, User>\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::Extends);
        assert_eq!(info.tags[0].vendor, Some(TagVendor::PhpStan));
        assert_eq!(info.tags[0].name, "phpstan-extends");
        assert_eq!(info.tags[0].description, "Collection<int, User>");
    }

    #[test]
    fn phpstan_require_extends_tag_parsed() {
        let doc = "/**\n * @phpstan-require-extends JsonResource\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::RequireExtends);
        assert_eq!(info.tags[0].name, "phpstan-require-extends");
        assert_eq!(info.tags[0].description, "JsonResource");
        assert!(
            info.tags[0].description_span.start.offset < info.tags[0].description_span.end.offset,
            "description span should be non-empty: {:?}",
            info.tags[0].description_span
        );
    }

    #[test]
    fn phpstan_require_implements_tag_parsed() {
        let doc = "/**\n * @phpstan-require-implements Countable\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::RequireImplements);
        assert_eq!(info.tags[0].name, "phpstan-require-implements");
        assert_eq!(info.tags[0].description, "Countable");
        assert!(
            info.tags[0].description_span.start.offset < info.tags[0].description_span.end.offset,
            "description span should be non-empty: {:?}",
            info.tags[0].description_span
        );
    }

    #[test]
    fn phpstan_sealed_tag_parsed() {
        let doc = "/**\n * @phpstan-sealed FooClass|BarClass\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::Sealed);
        assert_eq!(info.tags[0].name, "phpstan-sealed");
        assert_eq!(info.tags[0].description, "FooClass|BarClass");
    }

    #[test]
    fn multiline_return_description_uses_newlines() {
        let doc = "/**\n * @return array an array containing all the elements of arr1\n * after applying the callback function to each one.\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Return)
            .expect("should have return");
        assert_eq!(
            tag.description,
            "array an array containing all the elements of arr1\nafter applying the callback function to each one."
        );
    }

    #[test]
    fn multiline_type_in_return_tag() {
        let doc =
            "/**\n * @return array{\n *   name: string,\n *   age: int\n * } the user data\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = info
            .first_tag_by_kind(TagKind::Return)
            .expect("should have return");
        assert!(
            tag.description.contains("name: string"),
            "should contain shape fields: {:?}",
            tag.description
        );
        assert!(
            tag.description.contains("the user data"),
            "should contain description after type: {:?}",
            tag.description
        );
    }

    #[test]
    fn description_extracted_from_text_elements() {
        let doc = "/**\n * This is a description.\n * Second line.\n *\n * @return string\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(
            info.description.as_deref(),
            Some("This is a description.\nSecond line.")
        );
        assert_eq!(info.tags.len(), 1);
        assert_eq!(info.tags[0].kind, TagKind::Return);
    }

    #[test]
    fn description_none_when_tags_only() {
        let doc = "/** @return string */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.description, None);
    }

    #[test]
    fn description_with_inline_code() {
        let doc = "/**\n * Use `code` here.\n * @return void\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(info.description.as_deref(), Some("Use `code` here."));
    }

    #[test]
    fn description_with_inline_link_tag() {
        let doc = "/**\n * See {@link https://php.net} for details.\n * @return void\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(
            info.description.as_deref(),
            Some("See {@link https://php.net} for details.")
        );
    }

    #[test]
    fn description_with_html_tags_preserved() {
        let doc = "/**\n * Use <b>bold</b> text.\n * @param string $x\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let desc = info
            .description
            .as_deref()
            .expect("should have description");
        assert!(
            desc.contains("<b>bold</b>"),
            "HTML tags should be preserved in raw description: {desc}"
        );
    }

    #[test]
    fn tag_spans_are_populated() {
        let doc = "/** @return string The result */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let tag = &info.tags[0];
        // The span should cover the @return tag
        assert!(
            tag.span.start.offset < tag.span.end.offset,
            "tag span should be non-empty"
        );
        // The description_span should cover "string The result"
        assert!(
            tag.description_span.start.offset < tag.description_span.end.offset,
            "description span should be non-empty"
        );
    }

    #[test]
    fn description_only_docblock() {
        let doc = "/**\n * Just a description, no tags.\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        assert_eq!(
            info.description.as_deref(),
            Some("Just a description, no tags.")
        );
        assert!(info.tags.is_empty());
    }

    #[test]
    fn partial_docblock_without_closing_delimiter() {
        // While the user is still typing, the docblock may have no closing
        // `*/`.  Everything typed so far must still parse.
        let doc = "/**\n * @param string $name\n * @return ";
        let info = parse_docblock_for_tags(doc).expect("partial docblock should parse");
        let params: Vec<_> = info.tags_by_kind(TagKind::Param).collect();
        assert_eq!(params.len(), 1, "should find one @param tag");
        assert!(
            params[0].description.contains("$name"),
            "param description should contain $name: {:?}",
            params[0].description
        );
    }

    #[test]
    fn partial_docblock_with_trailing_at_sign() {
        // Simulates a completion scenario: the user has typed `@` on a new
        // line but hasn't finished the tag name, and there is no closing
        // `*/` because the cursor is mid-edit.  The finished tags above it
        // must still be reported.
        let doc = "/**\n * @param string $name\n * @";
        let info = parse_docblock_for_tags(doc).expect("partial docblock should parse");
        let params: Vec<_> = info.tags_by_kind(TagKind::Param).collect();
        assert_eq!(
            params.len(),
            1,
            "should find @param: tags={:?}",
            info.tags.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
        assert!(
            params[0].description.contains("$name"),
            "param should contain $name: {:?}",
            params[0].description
        );
    }

    #[test]
    fn complete_docblock_with_bare_at_mid_body() {
        // The docblock has a closing `*/` but contains a bare `@` where the
        // user is typing.  The `@throws` tag above it must still be found.
        let doc = "/**\n * @throws RuntimeException\n * @\n */";
        let info = parse_docblock_for_tags(doc).expect("should parse");
        let throws: Vec<_> = info.tags_by_kind(TagKind::Throws).collect();
        assert_eq!(
            throws.len(),
            1,
            "should find @throws despite bare @: tags={:?}",
            info.tags
                .iter()
                .map(|t| format!("@{}", t.name))
                .collect::<Vec<_>>()
        );
        assert!(
            throws[0].description.contains("RuntimeException"),
            "throws tag should contain RuntimeException: {:?}",
            throws[0].description
        );
    }
}
