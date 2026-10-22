//! A property as parsed, and where a virtual one came from.

use super::*;

/// Stores extracted property information from a parsed PHP class.
#[derive(Debug, Clone)]
pub struct PropertyInfo {
    /// The property name WITHOUT the `$` prefix (e.g. "name", "age").
    /// This matches PHP access syntax: `$this->name` not `$this->$name`.
    pub name: Atom,
    /// Byte offset of the property's variable token (`$name`) in the source file.
    ///
    /// Set to the `span.start.offset` of the `DirectVariable` during parsing.
    /// A value of `0` means "not available" — callers should fall back to
    /// text search.
    pub name_offset: u32,
    /// Effective type hint after docblock override (e.g. `list<User>`).
    ///
    /// When a `@var` tag is present in the docblock and is more specific
    /// than the native PHP type hint, this holds the docblock type.
    /// Otherwise it holds the native type hint unchanged.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub type_hint: Option<PhpType>,
    /// The native PHP type hint as a parsed `PhpType` (e.g. `array`, `string`).
    ///
    /// Preserved separately so that hover can show the actual PHP declaration
    /// in the code block while displaying the richer docblock type alongside
    /// the FQN header.  `None` when no type hint is present in source.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub native_type_hint: Option<PhpType>,
    /// Human-readable description extracted from the property's docblock.
    ///
    /// This is the free-text portion of the docblock (before any `@tag` lines).
    /// `None` when the docblock has no description or no docblock is present.
    pub description: Option<String>,
    /// Whether the property is static.
    pub is_static: bool,
    /// Whether the property carries an explicit `readonly` modifier.
    ///
    /// A subclass may redeclare an inherited readonly property, but only as
    /// readonly, so override completion has to carry the modifier through.
    /// Properties that are only *implicitly* readonly (every property of a
    /// `readonly class`) have this `false`: a subclass of a readonly class is
    /// itself readonly, and repeating the keyword there is a compile error.
    pub is_readonly: bool,
    /// Visibility of the property (public, protected, or private).
    pub visibility: Visibility,
    /// Deprecation message from the `@deprecated` PHPDoc tag.
    ///
    /// `None` means not deprecated. `Some("")` means deprecated without a
    /// message. `Some("Use foo() instead")` includes the explanation.
    pub deprecation_message: Option<String>,
    /// Replacement code template (from `#[Deprecated(replacement: "...")]`).
    ///
    /// `None` when no replacement is specified.
    pub deprecated_replacement: Option<String>,
    /// Symbol and URL references from `@see` tags in the property's docblock.
    ///
    /// Each entry's `text` is what follows `@see`, which may be a symbol
    /// reference (e.g. `"NewProp"`, `"MyClass::$newProp"`) or a URL
    /// (e.g. `"https://example.com/docs"`).  Rendered in hover below
    /// `@link` entries, and appended to deprecation diagnostics.
    pub see_refs: Vec<SeeRef>,
    /// Whether this property is a virtual (synthesized) member.
    ///
    /// Virtual properties come from `@property` / `@property-read` /
    /// `@property-write` docblock tags, `@mixin` classes, or
    /// framework-specific providers (e.g. Laravel model columns).
    /// They have no real declaration in source code.
    ///
    /// Set to `true` by [`PropertyInfo::virtual_property`] and by
    /// providers; set to `false` by the parser for real declared
    /// properties.
    pub is_virtual: bool,
    /// Structured source for synthesized/model properties.
    pub source: Option<PropertySource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseColumnSource {
    pub connection: String,
    pub table: String,
    pub column: String,
    pub database_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub generated_expression: Option<String>,
    pub generated_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeDefaultSource {
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertySource {
    DeclaredDefault {
        value: Box<str>,
    },
    DatabaseColumn {
        column: DatabaseColumnSource,
        attribute_default: Option<AttributeDefaultSource>,
        mutator: Option<String>,
    },
    Cast {
        cast: String,
        column: Option<DatabaseColumnSource>,
        attribute_default: Option<AttributeDefaultSource>,
        mutator: Option<String>,
    },
    Accessor {
        method: String,
        mutator: Option<String>,
        column: Option<DatabaseColumnSource>,
    },
    AttributeDefault {
        default: AttributeDefaultSource,
        column: Option<DatabaseColumnSource>,
        mutator: Option<String>,
    },
    ComputedProperty {
        method: String,
        mutator: Option<String>,
    },
    Relationship {
        method: String,
        kind: String,
        /// Custom pivot accessor from `->as('name')` or the relationship's
        /// fourth generic, if any. Surfaced in hover.
        pivot_accessor: Option<Atom>,
        /// Custom pivot class from `->using(X::class)` on a many-to-many
        /// relationship (FQN), if any.  Surfaced in hover.
        pivot_using: Option<String>,
        /// Extra pivot columns from `->withPivot(...)` on a many-to-many
        /// relationship.  Surfaced in hover.
        pivot_columns: Vec<String>,
    },
    RelationshipCount {
        relationship: String,
    },
    /// A pivot accessor synthesized on a many-to-many target model.
    ///
    /// Related models accessed through a `belongsToMany`/`morphToMany`
    /// relationship gain a `$pivot` instance at runtime, or an accessor with
    /// the name configured by `->as('name')`. A project-wide reverse index
    /// records which accessors each related model gains and types them from
    /// the relationship's pivot generic.
    Pivot,
    /// An explicit `@property` / `@property-read` / `@property-write` tag
    /// declared on the class itself, a used trait, a parent class, or an
    /// implemented interface (not a `@mixin` member).
    ///
    /// Marks the property as user-declared intent: when merging virtual
    /// members it overrides types *inferred* from the database schema or
    /// framework conventions, while still losing to types derived from
    /// real PHP code (casts, accessors, relationship methods).
    DocblockTag,
}

impl PropertyInfo {
    /// Compare two properties by signature-relevant fields only.
    ///
    /// Ignores `name_offset` (changes on every keystroke).  Everything
    /// else — including description — affects type resolution or hover
    /// display and must trigger cache eviction when it changes.
    pub fn signature_eq(&self, other: &PropertyInfo) -> bool {
        self.name == other.name
            && self.type_hint == other.type_hint
            && self.visibility == other.visibility
            && self.is_static == other.is_static
            && self.is_readonly == other.is_readonly
            && self.description == other.description
            && self.deprecation_message == other.deprecation_message
            && self.deprecated_replacement == other.deprecated_replacement
            && self.is_virtual == other.is_virtual
            && self.source == other.source
    }

    /// Return the type hint as a string, if present.
    ///
    /// Convenience wrapper around `self.type_hint.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn type_hint_str(&self) -> Option<String> {
        self.type_hint.as_ref().map(|t| t.to_string())
    }

    /// Create a virtual `PropertyInfo` with sensible defaults.
    ///
    /// The property is public, non-static, with no deprecation message and
    /// `name_offset: 0`.
    ///
    /// Use struct update syntax to override individual fields:
    ///
    /// ```ignore
    /// PropertyInfo {
    ///     deprecation_message: Some("Use newProp instead".into()),
    ///     ..PropertyInfo::virtual_property("foo", Some("string"))
    /// }
    /// ```
    pub fn virtual_property(name: &str, type_hint: Option<&str>) -> Self {
        Self::virtual_property_typed(name, type_hint.map(PhpType::parse).as_ref())
    }

    /// Create a virtual property from a pre-parsed [`PhpType`].
    ///
    /// Same as [`virtual_property`](Self::virtual_property) but avoids a
    /// `PhpType → String → PhpType` round-trip when the caller already
    /// holds a `PhpType`.
    pub fn virtual_property_typed(name: &str, type_hint: Option<&PhpType>) -> Self {
        Self::virtual_property_owned(crate::atom::atom(name), type_hint.cloned())
    }

    /// Create a virtual property from an interned name and an owned type.
    pub fn virtual_property_owned(name: Atom, type_hint: Option<PhpType>) -> Self {
        Self {
            name,
            name_offset: 0,
            type_hint,
            native_type_hint: None,
            description: None,
            is_static: false,
            is_readonly: false,
            visibility: Visibility::Public,
            deprecation_message: None,
            deprecated_replacement: None,
            see_refs: Vec::new(),
            is_virtual: true,
            source: None,
        }
    }
}
