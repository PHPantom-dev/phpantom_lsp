//! The PHPDoc and PHPStan tags offered after `@`, grouped by the kind of
//! declaration a docblock documents.

use tower_lsp_server::gen_lsp_types::{CompletionItem, CompletionItemKind};

use super::DocblockContext;

/// A PHPDoc / PHPStan tag definition with metadata for completion.
pub(super) struct TagDef {
    /// The tag text including `@` (e.g. `"@param"`).
    pub(super) tag: &'static str,
    /// Brief one-line description shown in the completion detail.
    pub(super) detail: &'static str,
    /// Display label showing usage format (e.g. `"@param Type $name"`).
    /// `None` means use `tag` as the label.
    pub(super) label: Option<&'static str>,
}

/// Strip the leading `@` from a tag string.
///
/// The user has already typed `@` (or `@par…`) in the buffer and the LSP
/// client only replaces the *word* portion after `@`.  If the insert text
/// still contains `@`, the result is a doubled `@@tag`.
pub(super) fn strip_at(s: &str) -> &str {
    s.strip_prefix('@').unwrap_or(s)
}

// ── Function / Method tags ──────────────────────────────────────────────────

pub(super) const FUNCTION_TAGS: &[TagDef] = &[
    TagDef {
        tag: "@param",
        detail: "Document a function parameter",
        label: Some("@param Type $name"),
    },
    TagDef {
        tag: "@return",
        detail: "Document the return type",
        label: Some("@return Type"),
    },
    TagDef {
        tag: "@throws",
        detail: "Document a thrown exception",
        label: Some("@throws ExceptionType"),
    },
    TagDef {
        tag: "@template",
        detail: "Declare a generic type parameter",
        label: Some("@template T"),
    },
    TagDef {
        tag: "@template-covariant",
        detail: "Declare a covariant generic type parameter",
        label: Some("@template-covariant T"),
    },
    TagDef {
        tag: "@template-contravariant",
        detail: "Declare a contravariant generic type parameter",
        label: Some("@template-contravariant T"),
    },
    TagDef {
        tag: "@inheritdoc",
        detail: "Inherit documentation from parent",
        label: None,
    },
];

// ── Class-like tags ─────────────────────────────────────────────────────────

pub(super) const CLASS_TAGS: &[TagDef] = &[
    TagDef {
        tag: "@property",
        detail: "Declare a magic property",
        label: Some("@property Type $name"),
    },
    TagDef {
        tag: "@property-read",
        detail: "Declare a read-only magic property",
        label: Some("@property-read Type $name"),
    },
    TagDef {
        tag: "@property-write",
        detail: "Declare a write-only magic property",
        label: Some("@property-write Type $name"),
    },
    TagDef {
        tag: "@method",
        detail: "Declare a magic method",
        label: Some("@method ReturnType name()"),
    },
    TagDef {
        tag: "@mixin",
        detail: "Declare a mixin class",
        label: Some("@mixin ClassName"),
    },
    TagDef {
        tag: "@template",
        detail: "Declare a generic type parameter",
        label: Some("@template T"),
    },
    TagDef {
        tag: "@template-covariant",
        detail: "Declare a covariant generic type parameter",
        label: Some("@template-covariant T"),
    },
    TagDef {
        tag: "@template-contravariant",
        detail: "Declare a contravariant generic type parameter",
        label: Some("@template-contravariant T"),
    },
    TagDef {
        tag: "@extends",
        detail: "Specify generic parent class type",
        label: Some("@extends ClassName<Type>"),
    },
    TagDef {
        tag: "@implements",
        detail: "Specify generic interface type",
        label: Some("@implements InterfaceName<Type>"),
    },
    TagDef {
        tag: "@use",
        detail: "Specify generic trait type",
        label: Some("@use TraitName<Type>"),
    },
];

// ── Property tags ───────────────────────────────────────────────────────────

pub(super) const PROPERTY_TAGS: &[TagDef] = &[TagDef {
    tag: "@var",
    detail: "Document the property type",
    label: Some("@var Type"),
}];

// ── Constant tags ───────────────────────────────────────────────────────────

pub(super) const CONSTANT_TAGS: &[TagDef] = &[TagDef {
    tag: "@var",
    detail: "Document the constant type",
    label: Some("@var Type"),
}];

// ── General tags (available everywhere) ─────────────────────────────────────

pub(super) const GENERAL_TAGS: &[TagDef] = &[
    TagDef {
        tag: "@deprecated",
        detail: "Mark as deprecated",
        label: None,
    },
    TagDef {
        tag: "@see",
        detail: "Reference to related element",
        label: Some("@see ClassName::method()"),
    },
    TagDef {
        tag: "@since",
        detail: "Version when this was introduced",
        label: Some("@since 1.0.0"),
    },
    TagDef {
        tag: "@example",
        detail: "Reference to an example file",
        label: None,
    },
    TagDef {
        tag: "@link",
        detail: "URL to external documentation",
        label: Some("@link https://"),
    },
    TagDef {
        tag: "@internal",
        detail: "Mark as internal / not part of the public API",
        label: None,
    },
    TagDef {
        tag: "@todo",
        detail: "Document a to-do item",
        label: None,
    },
];

// ── Inline tags (inside code, not before a declaration) ─────────────────────
// Only tags that make sense as inline annotations: @var for type
// narrowing, @throws for exception hinting, plus a handful of general
// documentation tags.

pub(super) const INLINE_TAGS: &[TagDef] = &[
    TagDef {
        tag: "@var",
        detail: "Narrow the variable type",
        label: Some("@var Type"),
    },
    TagDef {
        tag: "@throws",
        detail: "Hint at an exception thrown by the next statement",
        label: Some("@throws ExceptionType"),
    },
    TagDef {
        tag: "@see",
        detail: "Reference to related element",
        label: Some("@see ClassName::method()"),
    },
    TagDef {
        tag: "@example",
        detail: "Reference to an example file",
        label: None,
    },
    TagDef {
        tag: "@link",
        detail: "URL to external documentation",
        label: Some("@link https://"),
    },
    TagDef {
        tag: "@todo",
        detail: "Document a to-do item",
        label: None,
    },
];

// ── PHPStan tags ────────────────────────────────────────────────────────────

pub(super) const PHPSTAN_FUNCTION_TAGS: &[TagDef] = &[
    TagDef {
        tag: "@phpstan-assert",
        detail: "PHPStan: assert parameter type after call",
        label: Some("@phpstan-assert Type $var"),
    },
    TagDef {
        tag: "@phpstan-assert-if-true",
        detail: "PHPStan: assert type when method returns true",
        label: Some("@phpstan-assert-if-true Type $var"),
    },
    TagDef {
        tag: "@phpstan-assert-if-false",
        detail: "PHPStan: assert type when method returns false",
        label: Some("@phpstan-assert-if-false Type $var"),
    },
    TagDef {
        tag: "@phpstan-self-out",
        detail: "PHPStan: narrow the type of $this after call",
        label: Some("@phpstan-self-out Type"),
    },
    TagDef {
        tag: "@phpstan-this-out",
        detail: "PHPStan: narrow the type of $this after call",
        label: Some("@phpstan-this-out Type"),
    },
    TagDef {
        tag: "@phpstan-ignore-next-line",
        detail: "PHPStan: suppress errors on the next line",
        label: None,
    },
    TagDef {
        tag: "@phpstan-type",
        detail: "PHPStan: define a local type alias",
        label: Some("@phpstan-type TypeName = Type"),
    },
    TagDef {
        tag: "@phpstan-import-type",
        detail: "PHPStan: import a type alias from another class",
        label: Some("@phpstan-import-type TypeName from ClassName"),
    },
];

pub(super) const PHPSTAN_CLASS_TAGS: &[TagDef] = &[
    TagDef {
        tag: "@phpstan-type",
        detail: "PHPStan: define a local type alias",
        label: Some("@phpstan-type TypeName = Type"),
    },
    TagDef {
        tag: "@phpstan-import-type",
        detail: "PHPStan: import a type alias from another class",
        label: Some("@phpstan-import-type TypeName from ClassName"),
    },
    TagDef {
        tag: "@phpstan-require-extends",
        detail: "PHPStan: require extending a specific class",
        label: Some("@phpstan-require-extends ClassName"),
    },
    TagDef {
        tag: "@phpstan-require-implements",
        detail: "PHPStan: require implementing a specific interface",
        label: Some("@phpstan-require-implements InterfaceName"),
    },
    TagDef {
        tag: "@phpstan-sealed",
        detail: "PHPStan: restrict which classes may extend/implement this class",
        label: Some("@phpstan-sealed ClassName|OtherClass"),
    },
];

pub(super) const PHPSTAN_PROPERTY_TAGS: &[TagDef] = &[];

/// The tag lists a docblock in `context` offers, in the order they are
/// listed.
pub(super) fn tags_for_context(context: DocblockContext) -> &'static [&'static [TagDef]] {
    match context {
        DocblockContext::FunctionOrMethod => &[FUNCTION_TAGS, GENERAL_TAGS, PHPSTAN_FUNCTION_TAGS],
        DocblockContext::ClassLike => &[CLASS_TAGS, GENERAL_TAGS, PHPSTAN_CLASS_TAGS],
        DocblockContext::Property => &[PROPERTY_TAGS, GENERAL_TAGS, PHPSTAN_PROPERTY_TAGS],
        DocblockContext::Constant => &[CONSTANT_TAGS, GENERAL_TAGS],
        DocblockContext::Inline => &[INLINE_TAGS],
        DocblockContext::Unknown => &[
            FUNCTION_TAGS,
            CLASS_TAGS,
            PROPERTY_TAGS,
            GENERAL_TAGS,
            PHPSTAN_FUNCTION_TAGS,
            PHPSTAN_CLASS_TAGS,
            PHPSTAN_PROPERTY_TAGS,
        ],
    }
}

/// The plain item for a tag: its usage label, inserting the bare tag.
pub(super) fn generic_tag_item(def: &TagDef) -> CompletionItem {
    // PHPStan tags sort after standard tags.
    let sort_prefix = if def.tag.starts_with("@phpstan") {
        "2"
    } else {
        "1"
    };
    CompletionItem {
        label: def.label.unwrap_or(def.tag).to_string(),
        kind: Some(CompletionItemKind::Keyword),
        detail: Some(def.detail.to_string()),
        insert_text: Some(strip_at(def.tag).to_string()),
        filter_text: Some(def.tag.to_string()),
        sort_text: Some(format!("{}_{}", sort_prefix, def.tag.to_lowercase())),
        ..CompletionItem::default()
    }
}
