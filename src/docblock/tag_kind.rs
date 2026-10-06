//! Vendor-agnostic classification of PHPDoc tags.
//!
//! `mago-phpdoc-syntax` parses a tag into a `TagValue` whose variant already
//! folds the vendor prefixes together: `@param`, `@psalm-param` and
//! `@phpstan-param` all produce `TagValue::Param`, with the prefix recorded
//! separately in `Tag::vendor`.  [`TagKind`] mirrors that: one variant per
//! *meaning*, with the vendor kept alongside it on
//! [`super::parser::TagInfo`].
//!
//! Tags that the parser does not model structurally (`@see`, `@link`,
//! `@removed`, `@psalm-if-this-is`, …) arrive as `TagValue::Generic`.  The few
//! we act on are recognised by name here; the rest become [`TagKind::Other`]
//! and are matched by name at the call site.

use mago_phpdoc_syntax::cst::{Tag, TagValue, TemplateTagValueVariance};

pub use mago_phpdoc_syntax::cst::TagVendor;

/// What a PHPDoc tag means, independent of any `@psalm-` / `@phpstan-`
/// prefix it was written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagKind {
    /// `@param`
    Param,
    /// `@param-out`
    ParamOut,
    /// `@param-closure-this`
    ParamClosureThis,
    /// `@return`
    Return,
    /// `@var`
    Var,
    /// `@throws`
    Throws,
    /// `@mixin`
    Mixin,
    /// `@self-out` / `@this-out`
    SelfOut,
    /// `@template` (invariant)
    Template,
    /// `@template-covariant`
    TemplateCovariant,
    /// `@template-contravariant`
    TemplateContravariant,
    /// `@extends` / `@template-extends`
    Extends,
    /// `@implements` / `@template-implements`
    Implements,
    /// `@use` / `@template-use`
    Use,
    /// `@require-extends`
    RequireExtends,
    /// `@require-implements`
    RequireImplements,
    /// `@sealed`
    Sealed,
    /// `@inheritors`
    Inheritors,
    /// `@method`
    Method,
    /// `@property`
    Property,
    /// `@property-read`
    PropertyRead,
    /// `@property-write`
    PropertyWrite,
    /// `@assert`
    Assert,
    /// `@assert-if-true`
    AssertIfTrue,
    /// `@assert-if-false`
    AssertIfFalse,
    /// `@type` — a local type alias declaration.
    TypeAlias,
    /// `@import-type` — an alias imported from another class.
    TypeAliasImport,
    /// `@deprecated`
    Deprecated,
    /// `@see`
    See,
    /// `@link`
    Link,
    /// PHPUnit's `@covers` — names the code unit a test exercises.
    Covers,
    /// PHPUnit's `@coversDefaultClass` — the class a bare `::member`
    /// reference in a sibling `@covers` tag belongs to.
    CoversDefaultClass,
    /// PHPUnit's `@uses` — names a code unit a test executes but does not
    /// measure coverage for.
    Uses,
    /// PHPUnit's `@dataProvider` — names the method that supplies a test's
    /// arguments.
    DataProvider,
    /// PHPUnit's `@depends` — names a test whose result this one consumes.
    Depends,
    /// Any other tag; match on [`super::parser::TagInfo::name`].
    Other,
}

/// Classify a parsed tag.
pub(crate) fn tag_kind(tag: &Tag<'_>) -> TagKind {
    match &tag.value {
        TagValue::Param(_) | TagValue::TypelessParam(_) => TagKind::Param,
        TagValue::ParamOut(_) => TagKind::ParamOut,
        TagValue::ParamClosureThis(_) => TagKind::ParamClosureThis,
        TagValue::Return(_) | TagValue::RealReturn(_) => TagKind::Return,
        TagValue::Var(_) => TagKind::Var,
        TagValue::Throws(_) => TagKind::Throws,
        TagValue::Mixin(_) => TagKind::Mixin,
        TagValue::SelfOut(_) => TagKind::SelfOut,
        TagValue::Template(value) => match value.variance {
            TemplateTagValueVariance::Invariant => TagKind::Template,
            TemplateTagValueVariance::Covariant => TagKind::TemplateCovariant,
            TemplateTagValueVariance::Contravariant => TagKind::TemplateContravariant,
        },
        TagValue::Extends(_) => TagKind::Extends,
        TagValue::Implements(_) => TagKind::Implements,
        TagValue::Use(_) => TagKind::Use,
        TagValue::RequireExtends(_) => TagKind::RequireExtends,
        TagValue::RequireImplements(_) => TagKind::RequireImplements,
        TagValue::Sealed(_) => TagKind::Sealed,
        TagValue::Inheritors(_) => TagKind::Inheritors,
        TagValue::Method(_) => TagKind::Method,
        TagValue::Property(_) => TagKind::Property,
        TagValue::PropertyRead(_) => TagKind::PropertyRead,
        TagValue::PropertyWrite(_) => TagKind::PropertyWrite,
        TagValue::Assert(_) => TagKind::Assert,
        TagValue::AssertIfTrue(_) => TagKind::AssertIfTrue,
        TagValue::AssertIfFalse(_) => TagKind::AssertIfFalse,
        TagValue::TypeAlias(_) => TagKind::TypeAlias,
        TagValue::TypeAliasImport(_) => TagKind::TypeAliasImport,
        TagValue::Deprecated(_) => TagKind::Deprecated,
        // `Generic` is a tag the parser does not model, and `Invalid` is one
        // whose value it could not make sense of.  Both keep their value as
        // raw text, which is what the extractors work with anyway, so fall
        // back to the name: a tag written with odd syntax still reaches the
        // code that knows how to be lenient about it.
        TagValue::Generic(_) | TagValue::Invalid(_) => tag_kind_from_name(tag.name.value),
        _ => TagKind::Other,
    }
}

/// Longest tag name we bother normalising; anything longer is [`TagKind::Other`].
const NORMALISED_NAME_CAPACITY: usize = 32;

/// Classify a tag by name alone.
///
/// The name is normalised the way the parser normalises its own dispatch
/// keys: the vendor prefix is dropped, hyphens are removed, and the rest is
/// lowercased.  So `@phpstan-assert-if-true` and `@assertIfTrue` both reduce
/// to `assertiftrue`.
fn tag_kind_from_name(name: &[u8]) -> TagKind {
    let bare = [b"psalm-".as_slice(), b"phpstan-", b"phan-", b"mago-"]
        .iter()
        .find_map(|prefix| name.strip_prefix(*prefix))
        .unwrap_or(name);

    let mut buffer = [0u8; NORMALISED_NAME_CAPACITY];
    let mut length = 0;
    for byte in bare {
        if *byte == b'-' {
            continue;
        }
        if length == buffer.len() {
            return TagKind::Other;
        }
        buffer[length] = byte.to_ascii_lowercase();
        length += 1;
    }

    match &buffer[..length] {
        b"param" => TagKind::Param,
        b"paramout" => TagKind::ParamOut,
        b"paramclosurethis" => TagKind::ParamClosureThis,
        b"return" | b"realreturn" => TagKind::Return,
        b"var" => TagKind::Var,
        b"throws" => TagKind::Throws,
        b"mixin" => TagKind::Mixin,
        b"selfout" | b"thisout" => TagKind::SelfOut,
        b"template" | b"templateinvariant" => TagKind::Template,
        b"templatecovariant" => TagKind::TemplateCovariant,
        b"templatecontravariant" => TagKind::TemplateContravariant,
        b"extends" | b"templateextends" => TagKind::Extends,
        b"implements" | b"templateimplements" => TagKind::Implements,
        b"use" | b"templateuse" => TagKind::Use,
        b"requireextends" => TagKind::RequireExtends,
        b"requireimplements" => TagKind::RequireImplements,
        b"sealed" => TagKind::Sealed,
        b"inheritors" => TagKind::Inheritors,
        b"method" => TagKind::Method,
        b"property" => TagKind::Property,
        b"propertyread" => TagKind::PropertyRead,
        b"propertywrite" => TagKind::PropertyWrite,
        b"assert" => TagKind::Assert,
        b"assertiftrue" => TagKind::AssertIfTrue,
        b"assertiffalse" => TagKind::AssertIfFalse,
        b"type" => TagKind::TypeAlias,
        b"importtype" => TagKind::TypeAliasImport,
        b"deprecated" => TagKind::Deprecated,
        b"see" => TagKind::See,
        b"link" => TagKind::Link,
        b"covers" => TagKind::Covers,
        b"coversdefaultclass" => TagKind::CoversDefaultClass,
        b"uses" => TagKind::Uses,
        b"dataprovider" => TagKind::DataProvider,
        b"depends" => TagKind::Depends,
        _ => TagKind::Other,
    }
}
