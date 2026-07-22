//! Data types used throughout the PHPantom server.
//!
//! This module contains all the "model" structs and enums that represent
//! extracted PHP information (classes, methods, properties, constants,
//! standalone functions) as well as completion-related types
//! (AccessKind, CompletionTarget, SubjectExpr), PHPStan conditional
//! return type representations, PHPStan/Psalm array shape types, and
//! the [`PhpVersion`] type used for version-aware stub filtering.

// Re-export SubjectExpr and BracketSegment from their canonical module
// so that existing `use crate::types::{SubjectExpr, BracketSegment, …}`
// paths continue to work.
pub use crate::subject_expr::{BracketSegment, SubjectExpr};

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::atom::{Atom, AtomMap};
use crate::php_type::PhpType;

mod resolved_type;
mod shared_vec;

pub use shared_vec::*;

// ─── MethodStore ────────────────────────────────────────────────────────────

/// Key for the global method store: `(class_fqn, method_name)`.
///
/// The class FQN is fully qualified (e.g. `"App\\Models\\User"`).
/// The method name is the original-case name (e.g. `"updateText"`).
pub type MethodStoreKey = (String, String);

/// Global method store mapping `(class_fqn, method_name)` to the
/// method's metadata.
///
/// This is the first step toward eliminating method cloning during
/// inheritance: once all consumers look up methods through the store
/// instead of iterating `ClassInfo.methods`, the inheritance merge
/// can copy `(fqn, name)` tuples instead of cloning full `MethodInfo`
/// structs.
pub type MethodStore = Arc<parking_lot::RwLock<HashMap<MethodStoreKey, Arc<MethodInfo>>>>;

/// Callback that resolves a function name to its [`FunctionInfo`].
///
/// Used by docblock generation and throws analysis to look up cross-file
/// function metadata.
pub type FunctionLoader<'a> = Option<&'a dyn Fn(&str) -> Option<FunctionInfo>>;

// ─── PHP Version ────────────────────────────────────────────────────────────

/// A PHP major.minor version used for version-aware stub filtering.
///
/// phpstorm-stubs annotate functions, methods, and parameters with
/// `#[PhpStormStubsElementAvailable(from: 'X.Y', to: 'X.Y')]` attributes
/// to indicate which PHP versions they apply to.  PHPantom uses this
/// struct to decide which variant of a stub element to present.
///
/// The version is detected from `composer.json` (`require.php`) during
/// server initialization. When no version is found, [`PhpVersion::default`]
/// returns PHP 8.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhpVersion {
    /// Major version number (e.g. `8` in PHP 8.4).
    pub major: u8,
    /// Minor version number (e.g. `4` in PHP 8.4).
    pub minor: u8,
}

impl PhpVersion {
    /// Create a new `PhpVersion`.
    pub const fn new(major: u8, minor: u8) -> Self {
        Self { major, minor }
    }

    /// Parse a PHP version from a Composer `require.php` constraint string.
    ///
    /// Extracts the first `major.minor` pair found in the constraint.
    /// Supports common formats:
    ///   - `"^8.4"` → 8.4
    ///   - `">=8.3"` → 8.3
    ///   - `"~8.2"` → 8.2
    ///   - `"8.1.*"` → 8.1
    ///   - `">=8.0 <8.4"` → 8.0 (first match wins)
    ///   - `"8.3.1"` → 8.3
    ///   - `"^8"` → 8.0
    ///
    /// Returns `None` if no version can be extracted.
    pub fn from_composer_constraint(constraint: &str) -> Option<Self> {
        // Walk through the constraint looking for digit sequences that
        // form a major.minor version.  Skip common prefix operators.
        let s = constraint.trim();

        // Try each whitespace/pipe-separated segment, return the first match.
        for segment in s.split(['|', ' ']) {
            let segment = segment.trim();
            if segment.is_empty() {
                continue;
            }

            // Strip leading operator characters: ^, ~, >=, <=, >, <, =, !
            let digits_start = segment
                .find(|c: char| c.is_ascii_digit())
                .unwrap_or(segment.len());
            let version_part = &segment[digits_start..];

            if version_part.is_empty() {
                continue;
            }

            let mut parts = version_part.split('.');
            if let Some(major_str) = parts.next()
                && let Ok(major) = major_str.parse::<u8>()
            {
                let minor = parts
                    .next()
                    .and_then(|s| s.trim_end_matches('*').parse::<u8>().ok())
                    .unwrap_or(0);
                return Some(Self { major, minor });
            }
        }

        None
    }

    /// Returns `true` if the given `from`..`to` version range includes
    /// this PHP version.
    ///
    /// - `from` is inclusive: the element is available starting at that version.
    /// - `to` is inclusive: the element is available up to and including that version.
    /// - When `from` is `None`, there is no lower bound.
    /// - When `to` is `None`, there is no upper bound.
    pub fn matches_range(&self, from: Option<PhpVersion>, to: Option<PhpVersion>) -> bool {
        if let Some(lower) = from
            && (self.major, self.minor) < (lower.major, lower.minor)
        {
            return false;
        }
        if let Some(upper) = to
            && (self.major, self.minor) > (upper.major, upper.minor)
        {
            return false;
        }
        true
    }
}

impl Default for PhpVersion {
    /// Default PHP version when none is detected: 8.5.
    fn default() -> Self {
        Self { major: 8, minor: 5 }
    }
}

impl fmt::Display for PhpVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// A namespace block within a PHP file, tracking which byte range it covers.
///
/// Files with a single `namespace Foo;` declaration produce one span covering
/// the entire file.  Files with multiple `namespace Foo { ... }` blocks produce
/// one span per block.  Files without any namespace declaration produce a single
/// span with `namespace: None`.
#[derive(Debug, Clone)]
pub struct NamespaceSpan {
    /// The namespace name (e.g. `"App\Models"`), or `None` for the global namespace.
    pub namespace: Option<String>,
    /// Byte offset of the start of this namespace block (inclusive).
    pub start: u32,
    /// Byte offset of the end of this namespace block (inclusive).
    pub end: u32,
}

/// Members extracted from a class-like body by `Backend::extract_class_like_members`.
pub struct ExtractedMembers {
    /// Methods declared directly in the class body.
    pub methods: Vec<MethodInfo>,
    /// Properties declared directly in the class body.
    pub properties: Vec<PropertyInfo>,
    /// Class constants declared directly in the class body.
    pub constants: Vec<ConstantInfo>,
    /// Trait names referenced by `use` statements inside the class body.
    pub used_traits: Vec<Atom>,
    /// `insteadof` precedence rules from trait `use` blocks.
    pub trait_precedences: Vec<TraitPrecedence>,
    /// `as` alias rules from trait `use` blocks.
    pub trait_aliases: Vec<TraitAlias>,
    /// `@use` generics extracted from docblocks on trait `use` statements
    /// inside the class body (e.g. `/** @use BuildsQueries<TModel> */`).
    /// Each entry is `(trait_name, vec_of_type_args)`.
    pub inline_use_generics: Vec<(Atom, Vec<PhpType>)>,
}

/// A type alias definition, either locally defined or imported from another class.
///
/// Local aliases are parsed into a [`PhpType`] at construction time, eliminating
/// repeated parsing during type resolution. Imported aliases store the source
/// class and original alias name so the resolver can look them up cross-file.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeAliasDef {
    /// A locally defined type alias (via `@phpstan-type` / `@psalm-type`).
    ///
    /// The `PhpType` is the fully parsed definition. For example,
    /// `@phpstan-type UserData array{name: string, email: string}` produces
    /// `Local(PhpType::parse("array{name: string, email: string}"))`.
    Local(PhpType),

    /// An imported type alias (via `@phpstan-import-type` / `@psalm-import-type`).
    ///
    /// `source_class` is the fully-qualified class name that defines the alias,
    /// and `original_name` is the alias name in that source class.
    ///
    /// For example, `@phpstan-import-type UserData from App\Models\User as UD`
    /// produces `Import { source_class: "App\\Models\\User", original_name: "UserData" }`.
    Import {
        /// Fully-qualified name of the class that defines the alias.
        source_class: String,
        /// The alias name in the source class.
        original_name: String,
    },
}

/// Variance of a `@template` parameter.
///
/// PHPStan and Psalm support `@template-covariant` and
/// `@template-contravariant` to express variance constraints on generic
/// type parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TemplateVariance {
    /// No variance annotation (`@template T`).
    #[default]
    Invariant,
    /// `@template-covariant T`
    Covariant,
    /// `@template-contravariant T`
    Contravariant,
}

impl TemplateVariance {
    /// Returns the tag name used in PHPDoc for this variance.
    pub fn tag_name(self) -> &'static str {
        match self {
            Self::Invariant => "template",
            Self::Covariant => "template-covariant",
            Self::Contravariant => "template-contravariant",
        }
    }
}

/// Visibility of a class member (method, property, or constant).
///
/// In PHP, members without an explicit visibility modifier default to `Public`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

/// Stores extracted parameter information from a parsed PHP method.
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    /// The parameter name including the `$` prefix (e.g. "$text").
    pub name: Atom,
    /// Whether this parameter is required (no default value and not variadic).
    pub is_required: bool,
    /// Effective type hint after docblock override (e.g. `Collection<User>`).
    ///
    /// When a `@param` tag is present in the docblock and is more specific
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
    /// Human-readable description extracted from the `@param` tag.
    ///
    /// For `@param list<User> $users The active users`, this would be
    /// `Some("The active users")`.  `None` when no description text
    /// follows the parameter name in the `@param` tag.
    pub description: Option<String>,
    /// The source text of the default value expression (e.g. `"0"`, `"null"`,
    /// `"[]"`, `"'hello'"`).
    ///
    /// Extracted from the AST span when the parameter has a default value.
    /// `None` when the parameter has no default.
    pub default_value: Option<String>,
    /// Whether this parameter is variadic (has `...`).
    pub is_variadic: bool,
    /// Whether this parameter is passed by reference (has `&`).
    pub is_reference: bool,
    /// The type that `$this` resolves to inside a closure passed for this
    /// parameter, declared via the `@param-closure-this` PHPDoc tag.
    ///
    /// For example, `@param-closure-this \Illuminate\Routing\Route $callback`
    /// means that inside the closure passed as `$callback`, `$this` refers to
    /// `\Illuminate\Routing\Route` rather than the lexically enclosing class.
    /// Common in Laravel where closures are rebound via `Closure::bindTo()`.
    pub closure_this_type: Option<PhpType>,
}

impl ParameterInfo {
    /// Compare two parameters by signature-relevant fields only.
    ///
    /// Ignores `name_offset` (not present on this struct) and
    /// `description` (display-only).  Everything else affects type
    /// resolution and must trigger cache eviction when it changes.
    pub fn signature_eq(&self, other: &ParameterInfo) -> bool {
        self.name == other.name
            && self.is_required == other.is_required
            && self.type_hint == other.type_hint
            && self.default_value == other.default_value
            && self.is_variadic == other.is_variadic
            && self.is_reference == other.is_reference
            && self.closure_this_type == other.closure_this_type
    }

    /// Fold `null` into the effective type when the default value is the
    /// literal `null`.
    ///
    /// A parameter such as `Type $x = null` accepts null at runtime (the
    /// pre-8.4 implicit-nullable form), so its effective type must admit
    /// null. Call this after a docblock `@param` merge has (re)computed
    /// `type_hint`, since the merge would otherwise drop the implied null.
    /// The operation is idempotent.
    pub fn apply_null_default(&mut self) {
        let defaults_to_null = self
            .default_value
            .as_deref()
            .is_some_and(|d| d.eq_ignore_ascii_case("null"));
        if defaults_to_null && let Some(t) = self.type_hint.take() {
            self.type_hint = Some(t.or_null());
        }
    }

    /// Return the type hint as a string, if present.
    ///
    /// Convenience wrapper around `self.type_hint.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn type_hint_str(&self) -> Option<String> {
        self.type_hint.as_ref().map(|t| t.to_string())
    }

    /// Return the native type hint as a string, if present.
    ///
    /// Convenience wrapper around `self.native_type_hint.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn native_type_hint_str(&self) -> Option<String> {
        self.native_type_hint.as_ref().map(|t| t.to_string())
    }
}

/// Stores extracted method information from a parsed PHP class.
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// The method name (e.g. "updateText").
    pub name: Atom,
    /// Byte offset of the method's name token in the source file.
    ///
    /// Set to the `span.start.offset` of the name `LocalIdentifier` during
    /// parsing.  A value of `0` means "not available" (e.g. for stubs and
    /// synthetic members) — callers should fall back to text search.
    pub name_offset: u32,
    /// The parameters of the method.
    pub parameters: Vec<ParameterInfo>,
    /// Effective return type after docblock override (e.g. `Collection<User>`).
    ///
    /// When a `@return` tag is present in the docblock and is more specific
    /// than the native PHP return type hint, this holds the docblock type.
    /// Otherwise it holds the native type hint unchanged.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub return_type: Option<PhpType>,
    /// The native PHP return type hint as a parsed `PhpType` (e.g. `array`, `self`).
    ///
    /// Preserved separately so that hover can show the actual PHP declaration
    /// in the code block while displaying the richer docblock type alongside
    /// the FQN header.  `None` when no return type hint is present in source.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub native_return_type: Option<PhpType>,
    /// Human-readable description extracted from the method's docblock.
    ///
    /// This is the free-text portion of the docblock (before any `@tag` lines).
    /// `None` when the docblock has no description or no docblock is present.
    pub description: Option<String>,
    /// Human-readable description extracted from the `@return` tag.
    ///
    /// For `@return list<User> The active users`, this would be
    /// `Some("The active users")`.  `None` when no description text
    /// follows the type in the `@return` tag.
    pub return_description: Option<String>,
    /// URLs from `@link` and `@see` tags in the docblock.
    ///
    /// For `@link https://php.net/...` and `@see https://example.com/`,
    /// this collects all URLs found. Empty when no link/see URL tags are present.
    pub links: Vec<String>,
    /// Symbol and URL references from `@see` tags in the docblock.
    ///
    /// Each entry is the raw text after `@see`, which may be a symbol
    /// reference (e.g. `"UnsetDemo"`, `"MyClass::method()"`) or a URL
    /// (e.g. `"https://example.com/docs"`).  Rendered in hover below
    /// `@link` entries.
    pub see_refs: Vec<String>,
    /// Whether the method is static.
    pub is_static: bool,
    /// Visibility of the method (public, protected, or private).
    pub visibility: Visibility,
    /// Optional PHPStan conditional return type parsed from the docblock.
    ///
    /// When present, the resolver should use this instead of `return_type`
    /// and resolve the concrete type based on call-site arguments.
    ///
    /// Example docblock:
    /// ```text
    /// @return ($abstract is class-string<TClass> ? TClass : mixed)
    /// ```
    pub conditional_return: Option<PhpType>,
    /// Deprecation message from the `@deprecated` PHPDoc tag.
    ///
    /// `None` means not deprecated. `Some("")` means deprecated without a
    /// message. `Some("Use foo() instead")` includes the explanation.
    pub deprecation_message: Option<String>,
    /// Replacement code template (from `#[Deprecated(replacement: "...")]`).
    ///
    /// Contains template variables like `%parametersList%`, `%parameter0%`,
    /// `%class%` that are expanded at call sites to offer a "replace
    /// deprecated call" code action.  `None` when no replacement is specified.
    pub deprecated_replacement: Option<String>,
    /// Template parameter names declared via `@template` tags in the
    /// method-level docblock.
    ///
    /// For example, a method with `@template T of Model` would have
    /// `template_params: vec!["T".into()]`.
    ///
    /// These are distinct from class-level template parameters
    /// (`ClassInfo::template_params`) and are used for general
    /// method-level generic type substitution at call sites.
    pub template_params: Vec<Atom>,
    /// Upper bounds for method-level template parameters.
    ///
    /// For `@template T of Model`, maps `"T"` → `PhpType::parse("Model")`.
    /// Used by hover to display the constraint when the return type or a
    /// parameter type is a method-level template parameter.
    pub template_param_bounds: AtomMap<PhpType>,
    /// Mappings from method-level template parameter names to the method
    /// parameter names (with `$` prefix) that directly bind them via
    /// `@param` annotations.
    ///
    /// For example, `@template T` + `@param T $model` produces
    /// `[("T", "$model")]`.  At call sites the resolver uses these
    /// bindings to infer concrete types for each template parameter
    /// from the actual argument expressions.
    pub template_bindings: Vec<(Atom, Atom)>,
    /// Whether this method has the `#[Scope]` attribute (Laravel 11+).
    ///
    /// Methods decorated with `#[\Illuminate\Database\Eloquent\Attributes\Scope]`
    /// are treated as Eloquent scope methods without needing the `scopeX`
    /// naming convention.  The method's own name is used directly as the
    /// public-facing scope name (e.g. `#[Scope] protected function active()`
    /// becomes `User::active()`).
    pub has_scope_attribute: bool,
    /// Whether this method is declared `abstract`.
    ///
    /// Abstract methods have no body (`MethodBody::Abstract`) and must be
    /// implemented by concrete subclasses.  Interface methods are
    /// implicitly abstract.  Used by the "Implement missing methods"
    /// code action to detect which inherited methods still need stubs.
    pub is_abstract: bool,
    /// Whether this method is a virtual (synthesized) member.
    ///
    /// Virtual methods come from `@method` docblock tags, `@mixin` classes,
    /// or framework-specific providers (e.g. Laravel model scopes).  They
    /// have no real declaration in source code.
    ///
    /// Set to `true` by [`MethodInfo::virtual_method`] and by providers;
    /// set to `false` by the parser for real declared methods.
    pub is_virtual: bool,
    /// Whether this method originates from a `::macro()` registration.
    ///
    /// Used by hover to show a "macro" indicator instead of the generic
    /// "virtual" label, so the user can distinguish macros from `@method`
    /// or `@mixin` synthesized members.
    pub is_macro: bool,
    /// Whether the return type was inferred from closure body analysis
    /// rather than explicitly declared via a type hint or docblock.
    ///
    /// When `true`, hover appends "(inferred)" to the return type line
    /// so the user knows the type is best-effort, not authoritative.
    pub is_inferred_return: bool,
    /// Type assertions declared via `@phpstan-assert` / `@psalm-assert` tags
    /// in the method's docblock.
    ///
    /// Works identically to [`FunctionInfo::type_assertions`] but for class
    /// methods.  Used by the narrowing engine to apply type guards from
    /// static method calls like `Assert::instanceOf($value, Foo::class)`.
    pub type_assertions: Vec<TypeAssertion>,
    /// Exception types declared via `@throws` tags in the method's docblock.
    ///
    /// For example, a method with `@throws \InvalidArgumentException` would have
    /// `throws: vec![PhpType::Named("InvalidArgumentException".into())]`.  Used
    /// during code generation and analysis to propagate exception information.
    pub throws: Vec<PhpType>,
    /// Type constraint from `@psalm-if-this-is` or `@phpstan-if-this-is`.
    ///
    /// When present, the method's return type should only be applied if
    /// the receiver's type matches this pattern. Template parameters in
    /// the pattern are resolved against the caller's concrete type to
    /// compute additional template substitutions for the return type.
    pub if_this_is: Option<PhpType>,
}

impl MethodInfo {
    /// Compare two methods by signature-relevant fields only.
    ///
    /// Ignores fields that change on every keystroke (byte offsets).
    /// Everything else — including descriptions and links — affects
    /// either type resolution or hover display and must trigger cache
    /// eviction when it changes.
    ///
    /// Parameters are compared in order (not as sets) because parameter
    /// order matters for signature help and call resolution.
    pub fn signature_eq(&self, other: &MethodInfo) -> bool {
        self.name == other.name
            && self.is_static == other.is_static
            && self.visibility == other.visibility
            && self.return_type == other.return_type
            && self.native_return_type == other.native_return_type
            && self.conditional_return == other.conditional_return
            && self.description == other.description
            && self.return_description == other.return_description
            && self.links == other.links
            && self.see_refs == other.see_refs
            && self.deprecation_message == other.deprecation_message
            && self.deprecated_replacement == other.deprecated_replacement
            && self.template_params == other.template_params
            && self.template_param_bounds == other.template_param_bounds
            && self.template_bindings == other.template_bindings
            && self.has_scope_attribute == other.has_scope_attribute
            && self.is_abstract == other.is_abstract
            && self.is_virtual == other.is_virtual
            && self.is_macro == other.is_macro
            && self.is_inferred_return == other.is_inferred_return
            && self.throws == other.throws
            && self.parameters.len() == other.parameters.len()
            && self
                .parameters
                .iter()
                .zip(other.parameters.iter())
                .all(|(a, b)| a.signature_eq(b))
    }

    /// Return the return type as a string, if present.
    ///
    /// Convenience wrapper around `self.return_type.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn return_type_str(&self) -> Option<String> {
        self.return_type.as_ref().map(|t| t.to_string())
    }

    /// Create a virtual `MethodInfo` with sensible defaults.
    ///
    /// The method is public, non-static, non-deprecated, with no
    /// parameters, no template params, and `name_offset: 0`.
    ///
    /// Use struct update syntax to override individual fields:
    ///
    /// ```ignore
    /// MethodInfo {
    ///     is_static: true,
    ///     parameters: params,
    ///     ..MethodInfo::virtual_method("foo", Some("string"))
    /// }
    /// ```
    pub fn virtual_method(name: &str, return_type: Option<&str>) -> Self {
        Self {
            name: crate::atom::atom(name),
            name_offset: 0,
            parameters: Vec::new(),
            return_type: return_type.map(PhpType::parse),
            native_return_type: None,
            description: None,
            return_description: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            is_static: false,
            visibility: Visibility::Public,
            conditional_return: None,
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: Vec::new(),
            template_param_bounds: AtomMap::default(),
            template_bindings: Vec::new(),
            has_scope_attribute: false,
            is_abstract: false,
            is_virtual: true,
            is_macro: false,
            is_inferred_return: false,
            type_assertions: Vec::new(),
            throws: Vec::new(),
            if_this_is: None,
        }
    }

    /// Like [`virtual_method`], but accepts the return type as a
    /// `PhpType` directly, avoiding the `PhpType → String → PhpType`
    /// round-trip when the caller already holds a `PhpType`.
    pub fn virtual_method_typed(name: &str, return_type: Option<&PhpType>) -> Self {
        Self {
            name: crate::atom::atom(name),
            name_offset: 0,
            parameters: Vec::new(),
            return_type: return_type.cloned(),
            native_return_type: None,
            description: None,
            return_description: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            is_static: false,
            visibility: Visibility::Public,
            conditional_return: None,
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: Vec::new(),
            template_param_bounds: AtomMap::default(),
            template_bindings: Vec::new(),
            has_scope_attribute: false,
            is_abstract: false,
            is_virtual: true,
            is_macro: false,
            is_inferred_return: false,
            type_assertions: Vec::new(),
            throws: Vec::new(),
            if_this_is: None,
        }
    }
}

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
    /// Effective type hint string after docblock override (e.g. "list<User>").
    ///
    /// When a `@var` tag is present in the docblock and is more specific
    /// than the native PHP type hint, this holds the docblock type.
    /// Otherwise it holds the native type hint unchanged.
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
    /// Each entry is the raw text after `@see`, which may be a symbol
    /// reference (e.g. `"NewProp"`, `"MyClass::$newProp"`) or a URL
    /// (e.g. `"https://example.com/docs"`).  Rendered in hover below
    /// `@link` entries, and appended to deprecation diagnostics.
    pub see_refs: Vec<String>,
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
    },
    RelationshipCount {
        relationship: String,
    },
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
        Self {
            name: crate::atom::atom(name),
            name_offset: 0,
            type_hint: type_hint.cloned(),
            native_type_hint: None,
            description: None,
            is_static: false,
            visibility: Visibility::Public,
            deprecation_message: None,
            deprecated_replacement: None,
            see_refs: Vec::new(),
            is_virtual: true,
            source: None,
        }
    }
}

/// Stores extracted constant information from a parsed PHP class.
#[derive(Debug, Clone)]
pub struct ConstantInfo {
    /// The constant name (e.g. "MAX_SIZE", "STATUS_ACTIVE").
    pub name: Atom,
    /// Byte offset of the constant's name token in the source file.
    ///
    /// Set to the `span.start.offset` of the name `LocalIdentifier` during
    /// parsing.  A value of `0` means "not available" — callers should fall
    /// back to text search.
    pub name_offset: u32,
    /// Optional type hint (e.g. `string`, `int`).
    ///
    /// Call `.to_string()` when a display string is needed.
    pub type_hint: Option<PhpType>,
    /// Visibility of the constant (public, protected, or private).
    pub visibility: Visibility,
    /// Deprecation message from the `@deprecated` PHPDoc tag.
    ///
    /// `None` means not deprecated. `Some("")` means deprecated without a
    /// message. `Some("Use OK instead")` includes the explanation.
    pub deprecation_message: Option<String>,
    /// Replacement code template (from `#[Deprecated(replacement: "...")]`).
    ///
    /// `None` when no replacement is specified.
    pub deprecated_replacement: Option<String>,
    /// Symbol and URL references from `@see` tags in the constant's docblock.
    ///
    /// Each entry is the raw text after `@see`, which may be a symbol
    /// reference (e.g. `"NEW_FLAG"`, `"MyClass::NEW_CONST"`) or a URL
    /// (e.g. `"https://example.com/docs"`).  Rendered in hover below
    /// `@link` entries, and appended to deprecation diagnostics.
    pub see_refs: Vec<String>,
    /// Human-readable description extracted from the constant's docblock.
    ///
    /// This is the free-text portion of the docblock (before any `@tag` lines).
    /// `None` when the docblock has no description or no docblock is present.
    pub description: Option<String>,
    /// Whether this constant is an enum case rather than a regular class constant.
    pub is_enum_case: bool,
    /// The literal value of a backed enum case (e.g. `"'pending'"` for
    /// `case Pending = 'pending';`).  `None` for unit enum cases and
    /// regular class constants.
    pub enum_value: Option<String>,
    /// The initializer expression source text for a regular class constant
    /// (e.g. `"'active'"` for `const STATUS = 'active';`, `"100"` for
    /// `const LIMIT = 100;`).  `None` when the constant has no initializer
    /// or the source text could not be extracted.
    pub value: Option<String>,
    /// Whether this constant is a virtual (synthesized) member.
    ///
    /// Virtual constants come from `@mixin` classes or framework-specific
    /// providers.  They have no real declaration in source code.
    ///
    /// Set to `true` by providers; set to `false` by the parser for real
    /// declared constants.
    pub is_virtual: bool,
}

impl ConstantInfo {
    /// Compare two constants by signature-relevant fields only.
    ///
    /// Ignores `name_offset` (changes on every keystroke) and
    /// `description` (display-only).  Everything else affects type
    /// resolution and must trigger cache eviction when it changes.
    pub fn signature_eq(&self, other: &ConstantInfo) -> bool {
        self.name == other.name
            && self.type_hint == other.type_hint
            && self.visibility == other.visibility
            && self.deprecation_message == other.deprecation_message
            && self.deprecated_replacement == other.deprecated_replacement
            && self.is_enum_case == other.is_enum_case
            && self.enum_value == other.enum_value
            && self.value == other.value
            && self.is_virtual == other.is_virtual
    }

    /// Return the type hint as a string, if present.
    ///
    /// Convenience wrapper around `self.type_hint.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn type_hint_str(&self) -> Option<String> {
        self.type_hint.as_ref().map(|t| t.to_string())
    }
}

/// Stores extracted information about a global constant defined via
/// `define('NAME', value)` or a top-level `const NAME = value;` statement.
///
/// Used by `global_defines` to provide hover content (showing the constant's
/// value) and go-to-definition support.
#[derive(Debug, Clone)]
pub struct DefineInfo {
    /// The `file://` URI of the file where the constant was defined.
    pub file_uri: String,
    /// Byte offset of the `define` keyword or `const` keyword in the source
    /// file, used for go-to-definition.  A value of `0` means "not available"
    /// (e.g. constants discovered from Composer autoload before parsing).
    pub name_offset: u32,
    /// The initializer expression source text (e.g. `"'1.0.0'"` for
    /// `define('APP_VERSION', '1.0.0')`, or `"42"` for `const LIMIT = 42;`).
    /// `None` when the value could not be extracted.
    pub value: Option<String>,
}

/// Describes the access operator that triggered completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessKind {
    /// Completion triggered after `->` (instance access).
    Arrow,
    /// Completion triggered after `::` (static access).
    DoubleColon,
    /// Completion triggered after `parent::`, `self::`, or `static::`.
    ///
    /// All three keywords use `::` syntax but differ from external static
    /// access (`ClassName::`): they show both static **and** instance
    /// methods (PHP allows `self::nonStaticMethod()`,
    /// `static::nonStaticMethod()`, and `parent::nonStaticMethod()` from
    /// an instance context), plus constants and static properties.
    /// Visibility filtering (e.g. excluding private members for `parent::`)
    /// is handled separately via `current_class_name`.
    ParentDoubleColon,
    /// No specific access operator detected (e.g. inside class body).
    Other,
}

/// The result of analysing what is to the left of `->` or `::`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionTarget {
    /// Whether `->` or `::` was used.
    pub access_kind: AccessKind,
    /// The textual subject before the operator, e.g. `"$this"`, `"self"`,
    /// `"$var"`, `"$this->prop"`, `"ClassName"`.
    pub subject: String,
}

// ─── Resolved Callable Target ───────────────────────────────────────────────

/// The result of resolving a call expression to its callable target.
///
/// Shared between signature help (`resolve_callable`) and named-argument
/// completion (`resolve_named_arg_params`).  Each caller projects the
/// fields it needs from the result.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedCallableTarget {
    /// The parameters of the callable.
    pub parameters: Vec<ParameterInfo>,
    /// Optional return type.
    pub return_type: Option<PhpType>,
    /// Whether the callable accepts any number of arguments without error,
    /// regardless of `parameters`. Set for a class with no explicit
    /// constructor: PHP silently ignores arguments to `new Foo(...)`, so
    /// the argument-count diagnostic must not flag extra arguments, while
    /// signature help still shows the (empty) signature.
    pub accepts_any_args: bool,
    /// Alternate parameter lists from overloaded function declarations.
    ///
    /// Populated from `FunctionInfo::overloads` when resolving standalone
    /// function calls.  The type checker tries each overload and only
    /// emits a diagnostic when the call is incompatible with ALL.
    pub overloads: Vec<Vec<ParameterInfo>>,
}
/// Stores extracted information about a standalone PHP function.
///
/// This is used for global / namespaced functions defined outside of classes,
/// typically found in files listed by Composer's `autoload_files.php`.
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// The function name (e.g. "array_map", "myHelper").
    pub name: Atom,
    /// Byte offset of the function's name token in the source file.
    ///
    /// Set to the `span.start.offset` of the name `LocalIdentifier` during
    /// parsing.  A value of `0` means "not available" (e.g. for stubs and
    /// synthetic entries) — callers should fall back to text search.
    pub name_offset: u32,
    /// The parameters of the function.
    pub parameters: Vec<ParameterInfo>,
    /// Effective return type after docblock override (e.g. `Collection<User>`).
    ///
    /// When a `@return` tag is present in the docblock and is more specific
    /// than the native PHP return type hint, this holds the docblock type.
    /// Otherwise it holds the native type hint unchanged.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub return_type: Option<PhpType>,
    /// The native PHP return type hint as a parsed `PhpType` (e.g. `array`, `self`).
    ///
    /// Preserved separately so that hover can show the actual PHP declaration
    /// in the code block while displaying the richer docblock type alongside
    /// the FQN header.  `None` when no return type hint is present in source.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub native_return_type: Option<PhpType>,
    /// Human-readable description extracted from the function's docblock.
    ///
    /// This is the free-text portion of the docblock (before any `@tag` lines).
    /// `None` when the docblock has no description or no docblock is present.
    pub description: Option<String>,
    /// Human-readable description extracted from the `@return` tag.
    ///
    /// For `@return list<User> The active users`, this would be
    /// `Some("The active users")`.  `None` when no description text
    /// follows the type in the `@return` tag.
    pub return_description: Option<String>,
    /// URLs from `@link` and `@see` tags in the docblock.
    ///
    /// For `@link https://php.net/...` and `@see https://example.com/`,
    /// this collects all URLs found. Empty when no link/see URL tags are present.
    pub links: Vec<String>,
    /// Symbol and URL references from `@see` tags in the docblock.
    ///
    /// Each entry is the raw text after `@see`, which may be a symbol
    /// reference (e.g. `"UnsetDemo"`, `"MyClass::method()"`) or a URL
    /// (e.g. `"https://example.com/docs"`).  Rendered in hover below
    /// `@link` entries.
    pub see_refs: Vec<String>,
    /// The namespace this function is declared in, if any.
    /// For example, `Amp\delay` would have namespace `Some("Amp")`.
    pub namespace: Option<String>,
    /// Optional PHPStan conditional return type parsed from the docblock.
    ///
    /// When present, the resolver should use this instead of `return_type`
    /// and resolve the concrete type based on call-site arguments.
    ///
    /// Example docblock:
    /// ```text
    /// @return ($abstract is class-string<TClass> ? TClass : \Illuminate\Foundation\Application)
    /// ```
    pub conditional_return: Option<PhpType>,
    /// Type assertions parsed from `@phpstan-assert` / `@psalm-assert`
    /// annotations in the function's docblock.
    ///
    /// These allow user-defined functions to act as custom type guards,
    /// narrowing the type of a parameter after the call (or conditionally
    /// when used in an `if` condition).
    ///
    /// Example docblocks:
    /// ```text
    /// @phpstan-assert User $value           — unconditional assertion
    /// @phpstan-assert !User $value          — negated assertion
    /// @phpstan-assert-if-true User $value   — assertion when return is true
    /// @phpstan-assert-if-false User $value  — assertion when return is false
    /// ```
    pub type_assertions: Vec<TypeAssertion>,
    /// Deprecation message from the `@deprecated` PHPDoc tag.
    ///
    /// `None` means not deprecated. `Some("")` means deprecated without a
    /// message. `Some("Use newHelper() instead")` includes the explanation.
    pub deprecation_message: Option<String>,
    /// Replacement code template (from `#[Deprecated(replacement: "...")]`).
    ///
    /// Contains template variables like `%parametersList%`, `%parameter0%`,
    /// `%class%` that are expanded at call sites to offer a "replace
    /// deprecated call" code action.  `None` when no replacement is specified.
    pub deprecated_replacement: Option<String>,
    /// Template parameter names declared via `@template` tags in the
    /// function-level docblock.
    ///
    /// For example, a function with `@template T of Model` would have
    /// `template_params: vec!["T".into()]`.
    ///
    /// These mirror the `MethodInfo::template_params` field and are used
    /// for generic type substitution at call sites.
    pub template_params: Vec<Atom>,
    /// Mappings from function-level template parameter names to the
    /// function parameter names (with `$` prefix) that directly bind
    /// them via `@param` annotations.
    ///
    /// For example, `@template T` + `@param T $model` produces
    /// `[("T", "$model")]`.  At call sites the resolver uses these
    /// bindings to infer concrete types for each template parameter
    /// from the actual argument expressions.
    pub template_bindings: Vec<(Atom, Atom)>,
    /// Upper bounds for function-level template parameters
    /// (`@template T of Foo` → maps `"T"` to `Foo`).
    ///
    /// Used by `build_function_template_subs` to replace unbound
    /// template parameters with their declared bound (or `mixed`
    /// when no bound exists) so that raw template names never leak
    /// into downstream consumers.
    pub template_param_bounds: AtomMap<PhpType>,
    /// Exception types from `@throws` docblock tags.
    ///
    /// Populated during parsing from the function's docblock.  Used by
    /// the cross-file throws analysis to propagate exceptions from
    /// standalone function calls.
    pub throws: Vec<PhpType>,
    /// Whether this function was extracted from inside a
    /// `if (! function_exists('name'))` guard.
    ///
    /// Such functions are polyfills for native PHP functions introduced
    /// in newer versions.  When the configured PHP version already
    /// provides the native function (i.e. a stub exists in
    /// `stub_function_index`), the polyfill is dead code and should
    /// not shadow the stub's signature, deprecation status, or other
    /// metadata.
    pub is_polyfill: bool,
    /// Alternate parameter lists from overloaded function declarations.
    ///
    /// Some PHP functions (e.g. `strtr`, `implode`, `array_keys`) have
    /// multiple valid signatures that differ in parameter count and types.
    /// Stubs represent these as separate `function` declarations with the
    /// same name.  During parsing, duplicates are merged into this field.
    ///
    /// The type checker tries the primary `parameters` first, then each
    /// overload.  A diagnostic is only emitted when the call is
    /// incompatible with ALL signatures.
    pub overloads: Vec<Vec<ParameterInfo>>,
}

impl FunctionInfo {
    /// Return the return type as a string, if present.
    ///
    /// Convenience wrapper around `self.return_type.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn return_type_str(&self) -> Option<String> {
        self.return_type.as_ref().map(|t| t.to_string())
    }

    /// Return the native return type as a string, if present.
    ///
    /// Convenience wrapper around `self.native_return_type.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn native_return_type_str(&self) -> Option<String> {
        self.native_return_type.as_ref().map(|t| t.to_string())
    }

    /// Compare two `FunctionInfo` values for signature equality.
    ///
    /// Returns `true` when the public-facing signature (name, parameters,
    /// return types, template params, deprecation, throws) is identical.
    /// Fields that don't affect callers (description, links, name_offset)
    /// are intentionally excluded.
    ///
    /// Used by `update_ast` to detect cross-file invalidation: when a
    /// standalone function's signature changes, all open files that may
    /// call it need fresh diagnostics.
    pub fn signature_eq(&self, other: &FunctionInfo) -> bool {
        if self.name != other.name
            || self.return_type != other.return_type
            || self.native_return_type != other.native_return_type
            || self.conditional_return != other.conditional_return
            || self.deprecation_message != other.deprecation_message
            || self.deprecated_replacement != other.deprecated_replacement
            || self.template_params != other.template_params
            || self.template_bindings != other.template_bindings
            || self.template_param_bounds != other.template_param_bounds
            || self.type_assertions != other.type_assertions
            || self.throws != other.throws
            || self.namespace != other.namespace
        {
            return false;
        }

        if self.parameters.len() != other.parameters.len() {
            return false;
        }
        for (a, b) in self.parameters.iter().zip(other.parameters.iter()) {
            if !a.signature_eq(b) {
                return false;
            }
        }

        if self.overloads.len() != other.overloads.len() {
            return false;
        }

        true
    }
}

// ─── PHPStan Type Assertions ────────────────────────────────────────────────

/// A type assertion annotation parsed from `@phpstan-assert` /
/// `@psalm-assert` (and their `-if-true` / `-if-false` variants).
///
/// These annotations let any function or method act as a custom type
/// guard, telling the analyser that a parameter has been narrowed to
/// a specific type after the call succeeds.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAssertion {
    /// When the assertion applies.
    pub kind: AssertionKind,
    /// The parameter name **with** the `$` prefix (e.g. `"$value"`).
    pub param_name: String,
    /// The asserted type (e.g. `User`, `AdminUser`).
    ///
    /// Parsed from the raw docblock text via `PhpType::parse()`.
    /// Call `.to_string()` when a display string is needed.
    pub asserted_type: crate::php_type::PhpType,
    /// Whether the assertion is negated (`!Type`), meaning the parameter
    /// is guaranteed to *not* be this type.
    pub negated: bool,
}

/// When a `@phpstan-assert` annotation takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssertionKind {
    /// `@phpstan-assert` — unconditional: after the function returns
    /// (without throwing), the assertion holds for all subsequent code.
    Always,
    /// `@phpstan-assert-if-true` — the assertion holds when the function
    /// returns `true` (i.e. inside the `if` body).
    IfTrue,
    /// `@phpstan-assert-if-false` — the assertion holds when the function
    /// returns `false` (i.e. inside the `else` body, or the `if` body of
    /// a negated condition).
    IfFalse,
}

/// A trait `insteadof` adaptation.
///
/// When a class uses multiple traits that define the same method, PHP
/// requires an explicit `insteadof` declaration to resolve the conflict.
///
/// # Example
///
/// ```php
/// use TraitA, TraitB {
///     TraitA::method insteadof TraitB;
/// }
/// ```
///
/// This means TraitA's version of `method` wins and TraitB's is excluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitPrecedence {
    /// The trait that provides the winning method (e.g. `"TraitA"`).
    pub trait_name: Atom,
    /// The method name being resolved (e.g. `"method"`).
    pub method_name: Atom,
    /// The traits whose versions of the method are excluded
    /// (e.g. `["TraitB"]`).
    pub insteadof: Vec<Atom>,
}

/// A trait `as` alias adaptation.
///
/// Creates an alias for a trait method, optionally changing its visibility.
///
/// # Examples
///
/// ```php
/// use TraitA, TraitB {
///     TraitB::method as traitBMethod;          // rename
///     TraitA::method as protected;             // visibility-only change
///     TraitB::method as private altMethod;     // rename + visibility change
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitAlias {
    /// The trait that provides the method (e.g. `Some("TraitB")`).
    /// `None` when the method reference is unqualified (e.g. `method as …`).
    pub trait_name: Option<Atom>,
    /// The original method name (e.g. `"method"`).
    pub method_name: Atom,
    /// The alias name, if any (e.g. `Some("traitBMethod")`).
    /// `None` when only the visibility is changed (e.g. `method as protected`).
    pub alias: Option<Atom>,
    /// Optional visibility override (e.g. `Some(Visibility::Protected)`).
    pub visibility: Option<Visibility>,
}

/// The syntactic kind of a class-like declaration.
///
/// PHP has four class-like constructs that share the same `ClassInfo`
/// representation.  This enum lets callers distinguish them when the
/// difference matters (e.g. `throw new` completion should only offer
/// concrete classes, not interfaces or traits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClassLikeKind {
    /// A regular `class` declaration (the default).
    #[default]
    Class,
    /// An `interface` declaration.
    Interface,
    /// A `trait` declaration.
    Trait,
    /// An `enum` declaration.
    Enum,
}

/// The backing type of a PHP backed enum.
///
/// PHP enums can optionally declare a scalar backing type, which must be
/// either `string` or `int`.  Unit enums (no backing type) are represented
/// by `None` at the `ClassInfo` level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackedEnumType {
    /// `enum Foo: string { ... }`
    String,
    /// `enum Foo: int { ... }`
    Int,
}

impl fmt::Display for BackedEnumType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackedEnumType::String => write!(f, "string"),
            BackedEnumType::Int => write!(f, "int"),
        }
    }
}

/// PHP `\Attribute` target flags.
///
/// These mirror the constants defined on the built-in `\Attribute` class
/// and are stored as a bitmask in [`ClassInfo::attribute_targets`].
///
/// A value of `0` means "not an attribute class".  A non-zero value means
/// the class is decorated with `#[\Attribute(...)]` and the bits indicate
/// which declaration kinds the attribute may be applied to.
pub mod attribute_target {
    /// The class can be used as an attribute on class declarations.
    pub const TARGET_CLASS: u8 = 1;
    /// The class can be used as an attribute on function declarations.
    pub const TARGET_FUNCTION: u8 = 1 << 1;
    /// The class can be used as an attribute on method declarations.
    pub const TARGET_METHOD: u8 = 1 << 2;
    /// The class can be used as an attribute on property declarations.
    pub const TARGET_PROPERTY: u8 = 1 << 3;
    /// The class can be used as an attribute on class constant declarations.
    pub const TARGET_CLASS_CONSTANT: u8 = 1 << 4;
    /// The class can be used as an attribute on function/method parameters.
    pub const TARGET_PARAMETER: u8 = 1 << 5;
    /// All targets (the default when `#[\Attribute]` has no arguments).
    pub const TARGET_ALL: u8 = (1 << 6) - 1; // 63
}

/// Laravel-specific metadata extracted from Eloquent model classes.
///
/// Grouped into a sub-struct to keep the core `ClassInfo` focused on
/// PHP semantics. All fields default to empty/`None`, so non-Laravel
/// classes carry no overhead beyond a single struct value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LaravelMetadata {
    /// Custom collection class for Eloquent models.
    ///
    /// Detected from three Laravel mechanisms:
    ///
    /// 1. The `#[CollectedBy(CustomCollection::class)]` attribute on the
    ///    model class.
    /// 2. The `/** @use HasCollection<CustomCollection> */` docblock
    ///    annotation on a `use HasCollection;` trait usage.
    /// 3. A `newCollection()` method override returning a custom type.
    ///
    /// When set, the `LaravelModelProvider` replaces
    /// `\Illuminate\Database\Eloquent\Collection` with this class in
    /// relationship property types and Builder-forwarded return types
    /// (e.g. `get()`, `all()`).
    pub custom_collection: Option<PhpType>,
    /// Eloquent cast definitions extracted from the `$casts` property
    /// initializer or the `casts()` method body.
    ///
    /// Each entry maps a column name to a cast type string (e.g.
    /// `("created_at", "datetime")`, `("is_admin", "boolean")`).
    /// The `LaravelModelProvider` uses these to synthesize typed virtual
    /// properties, mapping cast type strings to PHP types (e.g.
    /// `datetime` to `Carbon\Carbon`, `boolean` to `bool`).
    pub casts_definitions: Vec<(String, String)>,
    /// Column names extracted from the deprecated `$dates` property
    /// array.
    ///
    /// Before `$casts`, Laravel used `protected $dates = [...]` to mark
    /// columns as Carbon instances. Each column listed here is typed as
    /// `Carbon\Carbon`. The `LaravelModelProvider` merges these at lower
    /// priority than `$casts`: if a column appears in both `$casts` and
    /// `$dates`, the cast type wins.
    pub dates_definitions: Vec<String>,
    /// Eloquent attribute defaults extracted from the `$attributes`
    /// property initializer.
    ///
    /// Each entry maps a column name to a PHP type string inferred from
    /// the literal default value (e.g. `("role", "string")`,
    /// `("is_active", "bool")`, `("login_count", "int")`).
    /// The `LaravelModelProvider` uses these as a fallback when no
    /// `$casts` entry exists for the same column.
    pub attributes_definitions: Vec<(String, PhpType)>,
    /// Literal Eloquent app-level defaults extracted from `$attributes`.
    pub attribute_defaults: Vec<(String, String)>,
    /// Column names extracted from `$fillable`, `$guarded`, `$hidden`,
    /// and `$appends` property arrays.
    ///
    /// These are simple string lists (no type information), so the
    /// `LaravelModelProvider` synthesizes `mixed`-typed virtual
    /// properties as a last-resort fallback when a column is not
    /// already covered by `$casts` or `$attributes`.
    pub column_names: Vec<String>,
    /// Explicit Eloquent `$connection` property.
    pub connection_name: Option<String>,
    /// Explicit Eloquent `$table` property.
    pub table_name: Option<String>,
    /// Whether the model declares `getConnectionName()`.
    pub has_get_connection_name_method: bool,
    /// Whether the model declares `getTable()`.
    pub has_get_table_method: bool,
    /// Explicit Eloquent `$primaryKey` property.
    ///
    /// - `None` — not declared (inherits the default `"id"`).
    /// - `Some("uuid")` — custom primary key column name.
    pub primary_key: Option<String>,
    /// Explicit Eloquent `$keyType` property.
    ///
    /// - `None` — not declared (inherits the default `"int"`).
    /// - `Some("string")` — custom key type (e.g. for UUID/ULID keys).
    ///
    /// Determines the PHP type of the synthesized primary key property,
    /// matching Laravel's `Model::getKeyType()`.
    pub key_type: Option<String>,
    /// Whether the model declares `getKeyName()`.
    ///
    /// When `true`, the primary key column name is computed at runtime
    /// and cannot be resolved statically, so no implicit primary key
    /// property is synthesized.
    pub has_get_key_name_method: bool,
    /// Whether `$timestamps` is explicitly set on the model.
    ///
    /// - `None` — not declared (inherits the default, which is `true`
    ///   on `Illuminate\Database\Eloquent\Model`).
    /// - `Some(true)` — explicitly enabled.
    /// - `Some(false)` — explicitly disabled; no timestamp properties
    ///   should be synthesized.
    pub timestamps: Option<bool>,
    /// Override for the `CREATED_AT` column name constant.
    ///
    /// - `None` — not declared (inherits the default `"created_at"`).
    /// - `Some(None)` — explicitly set to `null`; no created-at
    ///   property should be synthesized.
    /// - `Some(Some("created"))` — custom column name.
    pub created_at_name: Option<Option<String>>,
    /// Override for the `UPDATED_AT` column name constant.
    ///
    /// - `None` — not declared (inherits the default `"updated_at"`).
    /// - `Some(None)` — explicitly set to `null`; no updated-at
    ///   property should be synthesized.
    /// - `Some(Some("modified"))` — custom column name.
    pub updated_at_name: Option<Option<String>>,
    /// Custom Eloquent builder class for the model.
    ///
    /// Detected from three Laravel mechanisms:
    ///
    /// 1. The `#[UseEloquentBuilder(CustomBuilder::class)]` attribute on
    ///    the model class (Laravel 11+).
    /// 2. The `/** @use HasBuilder<CustomBuilder> */` docblock
    ///    annotation on a `use HasBuilder;` trait usage.
    /// 3. A `newEloquentBuilder()` method override returning a custom type.
    ///
    /// When set, the `LaravelModelProvider` uses this class instead of
    /// the standard `Illuminate\Database\Eloquent\Builder` for
    /// builder-as-static forwarding and `query()` resolution.
    pub custom_builder: Option<PhpType>,
}

/// Stores extracted class information from a parsed PHP file.
/// All data is owned so we don't depend on the parser's arena lifetime.
#[derive(Debug, Clone, Default)]
pub struct ClassInfo {
    /// The syntactic kind of this class-like declaration.
    pub kind: ClassLikeKind,
    /// The name of the class (e.g. "User").
    pub name: Atom,
    /// The methods defined directly in this class.
    ///
    /// Each method is wrapped in `Arc` so that inheritance merge can
    /// share method metadata across parent and child classes without
    /// deep-cloning the `MethodInfo` struct.  When no generic
    /// substitution is needed, merging a parent method into a child
    /// is a simple `Arc::clone` (refcount bump) instead of copying
    /// all strings, vecs, and hashmaps inside `MethodInfo`.
    ///
    /// The outer [`SharedVec`] makes cloning the entire `ClassInfo`
    /// O(1) (Arc refcount bump on the Vec itself).
    pub methods: SharedVec<Arc<MethodInfo>>,
    /// O(1) index from lowercased method name → position in `methods`
    /// (PHP method names are case-insensitive).
    ///
    /// Rebuilt by [`rebuild_method_index`] after bulk mutations
    /// (inheritance merge, parsing). The `get_method*` and `has_method`
    /// helpers use this for O(1) lookup instead of linear scan.
    /// When empty or stale (detected via `indexed_method_count`),
    /// the helpers fall back to linear scan.
    pub method_index: AtomMap<u32>,
    /// The `methods.len()` at the time `method_index` was last built.
    /// Used to detect staleness: if `methods.len() != indexed_method_count`,
    /// the index is stale and the helpers fall back to linear scan.
    pub indexed_method_count: u32,
    /// The properties defined directly in this class.
    pub properties: SharedVec<PropertyInfo>,
    /// The constants defined directly in this class.
    pub constants: SharedVec<ConstantInfo>,
    /// Byte offset where the class body starts (left brace).
    pub start_offset: u32,
    /// Byte offset where the class body ends (right brace).
    pub end_offset: u32,
    /// Byte offset of the `class` / `interface` / `trait` / `enum` keyword
    /// token in the source file.
    ///
    /// Used with `offset_to_position` to convert directly to an LSP
    /// `Position`.  A value of `0` means "not available" (e.g. for
    /// synthetic classes or anonymous classes) — callers return `None`.
    pub keyword_offset: u32,
    /// Byte offset where the class declaration starts, including any
    /// leading attribute lists.
    ///
    /// For `#[Route(...)] class Foo {}` this points at the `#[`, whereas
    /// `keyword_offset` points at `class`. When the class has no
    /// attributes this equals `keyword_offset`. A value of `0` means
    /// "not available" (synthetic classes).
    ///
    /// Used to associate `self`/`static`/`parent` references that appear
    /// inside class-level attributes (which sit *before* the keyword and
    /// the body braces) with their enclosing class.
    pub decl_start_offset: u32,
    /// The parent class name from the `extends` clause, if any.
    /// This is the raw name as written in source (e.g. "BaseClass", "Foo\\Bar").
    pub parent_class: Option<Atom>,
    /// Interface names from the `implements` clause (classes and enums only).
    ///
    /// These are resolved to fully-qualified names during post-processing
    /// (see `resolve_parent_class_names` in `parser/ast_update.rs`).
    /// Used by "Go to Implementation" to find classes that implement a
    /// given interface.
    pub interfaces: Vec<Atom>,
    /// Trait names used by this class via `use TraitName;` statements.
    /// These are resolved to fully-qualified names during post-processing.
    pub used_traits: Vec<Atom>,
    /// Class names from `@mixin` docblock tags.
    /// These declare that this class exposes public members from the listed
    /// classes via magic methods (`__call`, `__get`, `__set`, etc.).
    /// Resolved to fully-qualified names during post-processing.
    pub mixins: Vec<Atom>,
    /// Generic type arguments from `@mixin` tags.
    ///
    /// Each entry is `(MixinClassName, [TypeArg1, TypeArg2, …])`.
    /// For example, `@mixin Builder<TRelatedModel>` produces
    /// `("Builder", [PhpType::parse("TRelatedModel")])`.
    ///
    /// Used by [`collect_mixin_members`] to build a substitution map
    /// from the mixin class's `@template` parameters to the provided
    /// concrete types, analogous to how `extends_generics` works for
    /// parent class inheritance.
    pub mixin_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Required base class from a `@phpstan-require-extends` tag.
    ///
    /// Only meaningful on traits. When a trait carries
    /// `@phpstan-require-extends \Tests\TestCase`, any class that uses the
    /// trait must extend that base class, so inside the trait's methods
    /// `$this` has access to the base class's members. Resolved to a
    /// fully-qualified name during post-processing (see
    /// `resolve_parent_class_names` in `parser/ast_update.rs`).
    ///
    /// `None` when the trait has no such tag (or the declaration is not a
    /// trait).
    pub require_extends: Option<Atom>,
    /// Whether the class is declared `final`.
    ///
    /// Final classes cannot be extended, so `static::` is equivalent to
    /// `self::` and need not be offered as a separate completion subject.
    pub is_final: bool,
    /// Whether the class is declared `abstract`.
    ///
    /// Abstract classes cannot be instantiated directly, so they should
    /// be excluded from contexts like `throw new` or `new` completion
    /// where only concrete classes are valid.
    pub is_abstract: bool,
    /// Deprecation message from the `@deprecated` PHPDoc tag.
    ///
    /// `None` means not deprecated. `Some("")` means deprecated without a
    /// message. `Some("Use NewApi instead")` includes the explanation.
    pub deprecation_message: Option<String>,
    /// Replacement code template (from `#[Deprecated(replacement: "...")]`).
    ///
    /// `None` when no replacement is specified.
    pub deprecated_replacement: Option<String>,
    /// URLs from `@link` and `@see` tags in the class-level docblock.
    ///
    /// For `@link https://php.net/...` and `@see https://example.com/`,
    /// this collects all URLs found. Empty when no link/see URL tags are present.
    pub links: Vec<String>,
    /// Symbol and URL references from `@see` tags in the class-level docblock.
    ///
    /// Each entry is the raw text after `@see`, which may be a symbol
    /// reference (e.g. `"UnsetDemo"`, `"MyClass::method()"`) or a URL
    /// (e.g. `"https://example.com/docs"`).  Rendered in hover below
    /// `@link` entries.
    pub see_refs: Vec<String>,
    /// Template parameter names declared via `@template` / `@template-covariant`
    /// / `@template-contravariant` tags in the class-level docblock.
    ///
    /// For example, `Collection` with `@template TKey` and `@template TValue`
    /// would have `template_params: vec!["TKey".into(), "TValue".into()]`.
    pub template_params: Vec<Atom>,
    /// Upper bounds for template parameters, keyed by parameter name.
    ///
    /// Populated from the `of` clause in `@template` tags. For example,
    /// `@template TNode of PDependNode` produces
    /// `("TNode", PhpType::parse("PDependNode"))`.
    ///
    /// When a type hint resolves to a template parameter name that cannot be
    /// concretely substituted, the resolver falls back to this bound so that
    /// completion and go-to-definition still work against the bound type.
    pub template_param_bounds: AtomMap<PhpType>,
    /// Default values for template parameters, keyed by parameter name.
    ///
    /// Populated from the `= default` clause in `@template` tags. For example,
    /// `@template TAsync of bool = false` produces `("TAsync", "false")`.
    ///
    /// When a conditional return type references a template parameter that
    /// has no explicit binding at the call site, the resolver uses the
    /// default value to evaluate the condition.
    pub template_param_defaults: AtomMap<PhpType>,
    /// Generic type arguments from `@extends` / `@phpstan-extends` tags.
    ///
    /// Each entry is `(ClassName, [TypeArg1, TypeArg2, …])`.
    /// For example, `@extends Collection<int, Language>` produces
    /// `("Collection", [PhpType::parse("int"), PhpType::parse("Language")])`.
    pub extends_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Generic type arguments from `@implements` / `@phpstan-implements` tags.
    ///
    /// Each entry is `(InterfaceName, [TypeArg1, TypeArg2, …])`.
    /// For example, `@implements ArrayAccess<int, User>` produces
    /// `("ArrayAccess", [PhpType::parse("int"), PhpType::parse("User")])`.
    pub implements_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Generic type arguments from `@use` / `@phpstan-use` tags.
    ///
    /// Each entry is `(TraitName, [TypeArg1, TypeArg2, …])`.
    /// For example, `@use HasFactory<UserFactory>` produces
    /// `("HasFactory", [PhpType::parse("UserFactory")])`.
    ///
    /// When a trait declares `@template T` and a class uses it with
    /// `@use SomeTrait<ConcreteType>`, the trait's template parameter `T`
    /// is substituted with `ConcreteType` in all inherited methods and
    /// properties.
    pub use_generics: Vec<(Atom, Vec<PhpType>)>,
    /// Type aliases defined via `@phpstan-type` / `@psalm-type` tags in the
    /// class-level docblock, and imported via `@phpstan-import-type` /
    /// `@psalm-import-type`.
    ///
    /// Maps alias name → type definition string.
    /// For example, `@phpstan-type UserData array{name: string, email: string}`
    /// produces `("UserData", "array{name: string, email: string}")`.
    ///
    /// These are consulted during type resolution so that a method returning
    /// `UserData` resolves to the underlying `array{name: string, email: string}`.
    pub type_aliases: AtomMap<TypeAliasDef>,
    /// Trait `insteadof` precedence adaptations.
    ///
    /// When a class uses multiple traits with conflicting method names,
    /// `insteadof` declarations specify which trait's version wins.
    /// For example, `TraitA::method insteadof TraitB` means TraitA's
    /// `method` is used and TraitB's is excluded.
    pub trait_precedences: Vec<TraitPrecedence>,
    /// Trait `as` alias adaptations.
    ///
    /// Creates aliases for trait methods, optionally with visibility changes.
    /// For example, `TraitB::method as traitBMethod` adds a new method
    /// `traitBMethod` that is a copy of TraitB's `method`.
    pub trait_aliases: Vec<TraitAlias>,
    /// Raw class-level docblock text, preserved for deferred parsing.
    ///
    /// `@method` and `@property` / `@property-read` / `@property-write`
    /// tags are **not** parsed eagerly into `methods` / `properties`.
    /// Instead, the raw docblock string is stored here and parsed lazily
    /// by the `PHPDocProvider` virtual member provider when completion or
    /// go-to-definition actually needs virtual members.
    ///
    /// Other docblock tags (`@template`, `@extends`, `@deprecated`, etc.)
    /// are still parsed eagerly because they affect class metadata that is
    /// needed during indexing and inheritance resolution.
    pub class_docblock: Option<String>,
    /// The namespace this class was declared in.
    ///
    /// Populated during parsing from the enclosing `namespace { }` block.
    /// For files with a single namespace (the common PSR-4 case) this
    /// matches the file-level namespace.  For files with multiple
    /// namespace blocks (e.g. `example.php` with inline stubs) each class
    /// carries its own namespace so that `find_class_in_uri_classes_index` can
    /// distinguish two classes with the same short name in different
    /// namespace blocks (e.g. `Illuminate\Database\Eloquent\Builder` vs
    /// `Illuminate\Database\Query\Builder`).
    pub file_namespace: Option<Atom>,
    /// The backing type of a backed enum (e.g. `string` or `int`).
    /// `None` for unit enums and non-enum class-like declarations.
    pub backed_type: Option<BackedEnumType>,
    /// PHP attribute target bitmask.
    ///
    /// `0` means this class is **not** a PHP attribute.  A non-zero value
    /// means the class is decorated with `#[\Attribute(...)]` and the bits
    /// indicate which declaration kinds the attribute may target (see
    /// [`attribute_target`] constants).
    ///
    /// When `#[\Attribute]` is used without arguments, the default is
    /// [`attribute_target::TARGET_ALL`] (all targets).
    pub attribute_targets: u8,
    /// Laravel-specific metadata (custom collections, casts, attribute
    /// defaults, column names). `None` for non-Laravel classes to avoid
    /// per-class allocation overhead.
    pub laravel: Option<Box<LaravelMetadata>>,
}

// ─── ClassInfo helpers ──────────────────────────────────────────────────────

impl ClassInfo {
    /// Return the fully-qualified name of this class.
    ///
    /// Combines `file_namespace` and `name` into a single FQN string
    /// (e.g. `"App\\Models\\User"`).  When no namespace is set, returns
    /// the short name as-is.
    pub fn fqn(&self) -> Atom {
        match &self.file_namespace {
            Some(ns) if !ns.is_empty() => crate::atom::atom(&format!("{}\\{}", ns, self.name)),
            _ => self.name,
        }
    }

    /// Rebuild the `method_index` from the current `methods` vec.
    ///
    /// Call this after bulk mutations to `methods` (inheritance merge,
    /// parsing, virtual member injection). Individual `push` calls in
    /// test code can skip this — the lookup helpers fall back to linear
    /// scan when the index is empty or stale.
    ///
    /// Keys are lowercased because PHP method names are
    /// case-insensitive; `methods` keeps the declared spelling.
    pub fn rebuild_method_index(&mut self) {
        self.method_index.clear();
        self.method_index.reserve(self.methods.len());
        for (i, method) in self.methods.iter().enumerate() {
            // First-writer-wins: matches the semantics of
            // `.iter().find(|m| m.name == name)` which returns the
            // first match when duplicate names exist.
            self.method_index
                .entry(crate::atom::ascii_lowercase_atom(&method.name))
                .or_insert(i as u32);
        }
        self.indexed_method_count = self.methods.len() as u32;
    }

    /// Returns `true` when `method_index` is populated and consistent
    /// with the current `methods` vec length.
    #[inline]
    fn method_index_valid(&self) -> bool {
        !self.method_index.is_empty() && self.methods.len() as u32 == self.indexed_method_count
    }

    /// Look up a method by name, ignoring ASCII case (PHP method names
    /// are case-insensitive).
    ///
    /// Uses the `method_index` for O(1) lookup when available,
    /// falling back to linear scan otherwise.
    #[inline]
    pub fn get_method(&self, name: &str) -> Option<&MethodInfo> {
        if self.method_index_valid() {
            let atom = crate::atom::ascii_lowercase_atom(name);
            return self
                .method_index
                .get(&atom)
                .and_then(|&idx| self.methods.get(idx as usize))
                .map(|arc| arc.as_ref());
        }
        self.methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .map(|arc| arc.as_ref())
    }

    /// Alias of [`get_method`](Self::get_method), kept for call sites
    /// written when the primary lookup was still case-sensitive.
    #[inline]
    pub fn get_method_ci(&self, name: &str) -> Option<&MethodInfo> {
        self.get_method(name)
    }

    /// Check whether a method with the given name exists (ignoring
    /// ASCII case, per PHP semantics).
    #[inline]
    pub fn has_method(&self, name: &str) -> bool {
        if self.method_index_valid() {
            let atom = crate::atom::ascii_lowercase_atom(name);
            return self.method_index.contains_key(&atom);
        }
        self.methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(name))
    }

    /// Look up a method by name (ignoring ASCII case) and return a
    /// clone of the `Arc`.
    ///
    /// Useful when the caller needs to hold onto the method beyond the
    /// borrow of `self`, or when it will be inserted into another
    /// `ClassInfo` without modification.
    #[inline]
    pub fn get_method_arc(&self, name: &str) -> Option<Arc<MethodInfo>> {
        if self.method_index_valid() {
            let atom = crate::atom::ascii_lowercase_atom(name);
            return self
                .method_index
                .get(&atom)
                .and_then(|&idx| self.methods.get(idx as usize))
                .map(Arc::clone);
        }
        self.methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .map(Arc::clone)
    }

    /// Compare two `ClassInfo` values by signature-relevant fields only.
    ///
    /// Returns `true` when the two classes have identical signatures,
    /// meaning the resolved-class cache entry for this FQN does not need
    /// to be evicted.  This is the key predicate for signature-level
    /// cache invalidation (§33 in the roadmap).
    ///
    /// **Ignored fields** (change on every keystroke or are display-only):
    /// - `start_offset`, `end_offset`, `keyword_offset`
    /// - `link` (display-only URL from `@link`)
    ///
    /// **Compared fields** (affect resolution, inheritance, or virtual
    /// member injection):
    /// - All class-level metadata (`kind`, `name`, `parent_class`, etc.)
    /// - Methods, properties, and constants (compared as name-keyed sets
    ///   so that reordering members in source does not trigger eviction)
    /// - `class_docblock` (adding/removing `@method`/`@property` tags)
    /// - `laravel` metadata (affects virtual member providers)
    pub fn signature_eq(&self, other: &ClassInfo) -> bool {
        // ── Class-level metadata ────────────────────────────────────
        if self.kind != other.kind
            || self.name != other.name
            || self.file_namespace != other.file_namespace
            || self.parent_class != other.parent_class
            || self.interfaces != other.interfaces
            || self.used_traits != other.used_traits
            || self.mixins != other.mixins
            || self.mixin_generics != other.mixin_generics
            || self.require_extends != other.require_extends
            || self.is_final != other.is_final
            || self.is_abstract != other.is_abstract
            || self.deprecation_message != other.deprecation_message
            || self.deprecated_replacement != other.deprecated_replacement
            || self.attribute_targets != other.attribute_targets
            || self.template_params != other.template_params
            || self.template_param_bounds != other.template_param_bounds
            || self.extends_generics != other.extends_generics
            || self.implements_generics != other.implements_generics
            || self.use_generics != other.use_generics
            || self.type_aliases != other.type_aliases
            || self.trait_precedences != other.trait_precedences
            || self.trait_aliases != other.trait_aliases
            || self.class_docblock != other.class_docblock
            || self.backed_type != other.backed_type
            || self.laravel != other.laravel
        {
            return false;
        }

        // ── Methods (compared as a name-keyed set) ──────────────────
        if self.methods.len() != other.methods.len() {
            return false;
        }
        for method in &self.methods {
            let Some(other_method) = other.get_method(&method.name) else {
                return false;
            };
            if !method.signature_eq(other_method) {
                return false;
            }
        }

        // ── Properties (compared as a name-keyed set) ───────────────
        if self.properties.len() != other.properties.len() {
            return false;
        }
        for prop in &self.properties {
            let Some(other_prop) = other.properties.iter().find(|p| p.name == prop.name) else {
                return false;
            };
            if !prop.signature_eq(other_prop) {
                return false;
            }
        }

        // ── Constants (compared as a name-keyed set) ────────────────
        if self.constants.len() != other.constants.len() {
            return false;
        }
        for constant in &self.constants {
            let Some(other_const) = other.constants.iter().find(|c| c.name == constant.name) else {
                return false;
            };
            if !constant.signature_eq(other_const) {
                return false;
            }
        }

        true
    }

    /// Return a mutable reference to the `LaravelMetadata`, creating it
    /// if absent.
    ///
    /// This is the preferred way to set Laravel-specific fields in tests
    /// and parsing code: `class.laravel_mut().casts_definitions = …;`
    pub fn laravel_mut(&mut self) -> &mut LaravelMetadata {
        self.laravel
            .get_or_insert_with(|| Box::new(LaravelMetadata::default()))
    }

    /// Return a reference to the `LaravelMetadata`, if present.
    pub fn laravel(&self) -> Option<&LaravelMetadata> {
        self.laravel.as_deref()
    }

    /// Look up the stored `name_offset` for a member by name and kind.
    ///
    /// Returns `Some(offset)` when the member exists and has a non-zero
    /// offset, or `None` otherwise.  The `kind` string should be one of
    /// `"method"`, `"property"`, or `"constant"`.
    pub(crate) fn member_name_offset(&self, name: &str, kind: &str) -> Option<u32> {
        let off: Option<u32> = match kind {
            "method" => self.get_method(name).map(|m| m.name_offset),
            "property" => self
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| p.name_offset),
            "constant" => self
                .constants
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.name_offset),
            _ => None,
        };
        off.filter(|&o| o > 0)
    }

    /// Push a `ClassInfo` into `results` only if no existing entry shares
    /// the same class name.  This is the single place where completion /
    /// resolution code deduplicates candidate classes.
    pub(crate) fn push_unique(results: &mut Vec<ClassInfo>, cls: ClassInfo) {
        if !results.iter().any(|c| c.name == cls.name) {
            results.push(cls);
        }
    }

    /// Push an `Arc<ClassInfo>` into `results` only if no existing entry
    /// shares the same class name.
    pub(crate) fn push_unique_arc(results: &mut Vec<Arc<ClassInfo>>, cls: Arc<ClassInfo>) {
        if !results.iter().any(|c| c.name == cls.name) {
            results.push(cls);
        }
    }

    /// Extend `results` with entries from `new_classes`, skipping any whose
    /// name already appears in `results`.
    pub(crate) fn extend_unique_arc(
        results: &mut Vec<Arc<ClassInfo>>,
        new_classes: Vec<Arc<ClassInfo>>,
    ) {
        for cls in new_classes {
            Self::push_unique_arc(results, cls);
        }
    }
}

// ─── ResolvedType ───────────────────────────────────────────────────────────

/// The result of resolving a single type reference.
///
/// Carries the full PHPStan-style type string (preserving generics,
/// shapes, scalars, unions) alongside the resolved [`ClassInfo`] when
/// the type names a class-like.  Consumers pick whichever
/// representation they need without re-resolving.
///
/// This is the core type of the unified type resolution engine.
/// Instead of maintaining parallel resolvers that return `Vec<ClassInfo>`
/// (losing the type string) or `Option<String>` (losing the class info),
/// every expression resolver returns `Vec<ResolvedType>` and each
/// consumer reads the field it needs.
#[derive(Clone, Debug)]
pub struct ResolvedType {
    /// Structured type expression, e.g. `PhpType::Generic("Collection", [PhpType::Named("int"), PhpType::Named("User")])`.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub type_string: PhpType,

    /// Resolved class info, present when the base type names a
    /// class/interface/trait/enum.  `None` for scalars, shapes
    /// where the base is `array`, and unresolvable types.
    pub class_info: Option<Arc<ClassInfo>>,
}

// ─── File Context ───────────────────────────────────────────────────────────

/// Bundles the three pieces of file-level metadata that almost every
/// handler needs: the parsed classes, the `use` statement import table,
/// and the declared namespace.  Constructed by
/// [`Backend::file_context`](crate::Backend) to replace the repeated
/// lock-and-unwrap boilerplate that was duplicated across completion,
/// definition, and implementation handlers.
pub(crate) struct FileContext {
    /// Classes extracted from the file's AST (from `uri_classes_index`).
    pub classes: Vec<Arc<ClassInfo>>,
    /// Import table mapping short names to fully-qualified names
    /// (from `use_map`).
    pub use_map: HashMap<String, String>,
    /// The file's declared namespace, if any (from `namespace_map`).
    pub namespace: Option<String>,
    /// Per-file resolved names from `mago-names` (byte offset → FQN).
    ///
    /// `None` for files that were loaded via `parse_and_cache_content`
    /// (vendor/stub files) which don't run the name resolver.
    pub resolved_names: Option<Arc<crate::names::OwnedResolvedNames>>,
}

impl FileContext {
    /// Resolve a name to its FQN using the best available data source.
    ///
    /// When `resolved_names` is available and contains an entry at
    /// `offset`, returns the mago-names result directly (it applies
    /// PHP's full name resolution rules in a single pass).
    ///
    /// Falls back to the legacy `resolve_to_fqn` logic (use-map +
    /// namespace prefix) when `resolved_names` is not populated or
    /// has no entry at the given offset.
    ///
    /// `name` is the raw identifier text (used for the fallback path).
    /// `offset` is the starting byte offset of the identifier in the
    /// source file.
    pub fn resolve_name_at(&self, name: &str, offset: u32) -> String {
        if let Some(ref rn) = self.resolved_names
            && let Some(fqn) = rn.get(offset)
        {
            return fqn.to_string();
        }
        // Fallback: replicate resolve_to_fqn logic inline to avoid
        // a cross-module dependency on diagnostics::helpers.
        if !name.contains('\\') {
            if let Some(fqn) = self.use_map.get(name) {
                return fqn.clone();
            }
            if let Some(ref ns) = self.namespace {
                return format!("{}\\{}", ns, name);
            }
            return name.to_string();
        }
        let first_segment = name.split('\\').next().unwrap_or(name);
        if let Some(fqn_prefix) = self.use_map.get(first_segment) {
            let rest = &name[first_segment.len()..];
            return format!("{}{}", fqn_prefix, rest);
        }
        if let Some(ref ns) = self.namespace {
            return format!("{}\\{}", ns, name);
        }
        name.to_string()
    }
}

// ─── Eloquent Constants ─────────────────────────────────────────────────────

/// The fully-qualified name of the Eloquent Collection class.
///
/// Used by the `LaravelModelProvider` to detect and replace collection
/// return types when a model declares a custom collection class.
pub const ELOQUENT_COLLECTION_FQN: &str = "Illuminate\\Database\\Eloquent\\Collection";

// ─── Recursion Depth Limits ─────────────────────────────────────────────────
//
// Centralised constants for the maximum recursion depth allowed when
// walking inheritance chains, trait hierarchies, mixin graphs, and type
// alias resolution.  Defining them in one place ensures that the same
// limit is used consistently across the inheritance, definition, and
// completion modules.

/// Maximum depth when walking the `extends` parent chain
/// (class → parent → grandparent → …).
pub(crate) const MAX_INHERITANCE_DEPTH: u32 = 20;

/// Maximum depth when recursing into `use Trait` hierarchies
/// (a trait can itself `use` other traits).
pub(crate) const MAX_TRAIT_DEPTH: u32 = 20;

/// Maximum depth when recursing into `@mixin` class graphs.
pub(crate) const MAX_MIXIN_DEPTH: u32 = 10;

/// Maximum depth when resolving `@phpstan-type` / `@psalm-type` aliases
/// (an alias can reference another alias).
pub(crate) const MAX_ALIAS_DEPTH: u8 = 10;

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "../types_tests.rs"]
mod tests;
