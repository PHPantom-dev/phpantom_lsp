//! A class, interface, trait or enum as parsed, and the pieces of it that
//! are parsed apart from its members.

use super::*;

/// Members extracted from a class-like body by `Backend::extract_class_like_members`.
#[derive(Default)]
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

/// Virtual members declared by a class-level docblock's `@method` and
/// `@property` tags.
///
/// Parsed once while the class is extracted from source and shared behind
/// an `Arc`, so the inheritance merge and the PHPDoc virtual-member
/// provider read the structured form instead of re-parsing
/// [`ClassInfo::class_docblock`] every time the class is resolved.
#[derive(Debug, Clone, Default)]
pub struct DocblockMembers {
    /// `@method` / `@psalm-method` signatures, in source order.
    pub methods: Vec<Arc<MethodInfo>>,
    /// `@property`, `@property-read`, and `@property-write` entries as
    /// `(name, declared type)`.  The name carries no `$` prefix, and the
    /// type is `None` for an untyped `@property $name`.
    pub properties: Vec<(Atom, Option<PhpType>)>,
}

impl DocblockMembers {
    /// Parse the `@method` / `@property` tags out of a raw docblock.
    ///
    /// Returns `None` when the docblock declares neither, which is the
    /// overwhelmingly common case and keeps `ClassInfo` from carrying an
    /// empty allocation per class.
    pub fn from_docblock(docblock: &str) -> Option<Arc<Self>> {
        if docblock.is_empty() {
            return None;
        }
        let info = crate::docblock::parse_docblock_for_tags(docblock)?;
        Self::from_info(&info)
    }

    /// [`Self::from_docblock`] against an already-parsed docblock.
    pub fn from_info(info: &crate::docblock::DocblockInfo) -> Option<Arc<Self>> {
        let methods = crate::docblock::extract_method_tags_from_info(info);
        let properties = crate::docblock::extract_property_tags_from_info(info);
        if methods.is_empty() && properties.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            methods: methods.into_iter().map(Arc::new).collect(),
            properties,
        }))
    }
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
    /// Index from lowercased method name → position in `methods`
    /// (PHP method names are case-insensitive), sorted by name for
    /// binary search.
    ///
    /// A sorted `Vec` costs about a third of the equivalent `AtomMap`
    /// (hashbrown rounds its bucket array up to the next power of two,
    /// so a class with ~200 methods pays for 512 slots) while a lookup
    /// only costs a handful of `Atom` comparisons, which are `Copy`
    /// pointer-sized values.
    ///
    /// Rebuilt by [`rebuild_method_index`] after bulk mutations
    /// (inheritance merge, parsing). The `get_method*` and `has_method`
    /// helpers use this for `O(log n)` lookup instead of linear scan.
    /// When empty or stale (detected via `indexed_method_count`),
    /// the helpers fall back to linear scan.
    pub method_index: Vec<(Atom, u32)>,
    /// The `methods.len()` at the time `method_index` was last built.
    /// Used to detect staleness: if `methods.len() != indexed_method_count`,
    /// the index is stale and the helpers fall back to linear scan.
    pub indexed_method_count: u32,
    /// The properties defined directly in this class.
    ///
    /// Each property is wrapped in `Arc` (like [`methods`](Self::methods))
    /// so the inheritance merge shares property metadata across parent and
    /// child classes instead of deep-cloning the `PropertyInfo` struct.
    /// When no generic substitution is needed, merging a parent property
    /// into a child is a plain `Arc::clone`; when substitution applies, the
    /// transformed copy is interned (see
    /// [`intern_transformed_property`](crate::virtual_members::intern_transformed_property))
    /// so value-identical results collapse to one allocation.
    pub properties: SharedVec<Arc<PropertyInfo>>,
    /// The constants defined directly in this class.
    ///
    /// Wrapped in `Arc` so inheritance merging shares the allocation:
    /// constants are never transformed on inheritance, so an inherited
    /// constant is always a plain `Arc::clone` of the declaring class's.
    pub constants: SharedVec<Arc<ConstantInfo>>,
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
    /// The parent class from the `extends` clause, if any, as an FQN
    /// without a leading `\` (the parser resolves the name as written).
    pub parent_class: Option<Atom>,
    /// Interface names from the `implements` clause (classes and enums only).
    ///
    /// These are resolved to fully-qualified names during post-processing
    /// (see `resolve_parent_class_names` in `parser/name_resolution.rs`).
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
    /// `resolve_parent_class_names` in `parser/name_resolution.rs`).
    ///
    /// `None` when the trait has no such tag (or the declaration is not a
    /// trait).
    pub require_extends: Option<Atom>,
    /// Required interfaces from `@phpstan-require-implements` tags.
    ///
    /// Only meaningful on traits. When a trait carries
    /// `@phpstan-require-implements \Countable`, any class that uses the trait
    /// must implement that interface, so inside the trait's methods `$this` has
    /// access to the interface's members. Resolved to fully-qualified names
    /// during post-processing (see `resolve_parent_class_names` in
    /// `parser/name_resolution.rs`).
    pub require_implements: Vec<Atom>,
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
    /// Whether the class is declared `readonly` (PHP 8.2), or inherits
    /// that from an ancestor.
    ///
    /// Every declared property of a readonly class is readonly without
    /// carrying the keyword itself, so this is what tells a write
    /// diagnostic that `$point->x = 1` is illegal.  PHP requires the whole
    /// hierarchy to agree (a readonly class can neither be extended by a
    /// non-readonly class nor extend one, and every property of a trait it
    /// uses must be readonly), so the flag is propagated down the parent
    /// chain by the inheritance merge and holds for inherited properties
    /// too.
    ///
    /// The per-property [`PropertyInfo::is_readonly`] stays `false` for
    /// these: the keyword is not repeatable in a subclass.
    pub is_readonly: bool,
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
    /// Maps alias name → [`TypeAliasDef`]. For example,
    /// `@phpstan-type UserData array{name: string, email: string}` produces
    /// `("UserData", TypeAliasDef::Local(PhpType::parse("array{name: string, email: string}")))`.
    ///
    /// These are consulted during type resolution so that a method returning
    /// `UserData` resolves to the underlying `array{name: string, email: string}`.
    pub type_aliases: AtomMap<TypeAliasDef>,
    /// The member types the docblock settled in place of a native hint while
    /// naming a class the hint does not, sorted by member and slot.  Loading
    /// the class keeps each one only where it narrows the hint.
    pub unverified_doc_types: Box<[UnverifiedDocType]>,
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
    /// Raw class-level docblock text.
    ///
    /// Retained for the consumers that render or re-scan the original text
    /// (hover, `@see` navigation, docblock code actions).  Tag data that
    /// resolution depends on is parsed at extraction time into the typed
    /// fields above and into [`Self::doc_members`].
    pub class_docblock: Option<String>,
    /// Byte offset of [`Self::class_docblock`] in the file the class is
    /// declared in, `0` when it has none.
    pub class_docblock_start: u32,
    /// `@method` and `@property` / `@property-read` / `@property-write`
    /// tags parsed out of [`Self::class_docblock`].
    ///
    /// `None` when the docblock declares neither, so classes with an
    /// ordinary description-only docblock cost nothing beyond the
    /// `Option`.  The members are **not** merged into `methods` /
    /// `properties`: they stay separate so the PHPDoc virtual-member
    /// provider can apply generic substitutions per consumer and so that
    /// real declared members keep precedence over the tags.
    ///
    /// Always assign through [`Self::set_class_docblock`] to keep the two
    /// fields in sync.
    pub doc_members: Option<Arc<DocblockMembers>>,
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
    /// Cached fully-qualified name, populated by [`cache_fqn`] once the
    /// namespace has been stamped onto the class (see `parser/ast_update.rs`
    /// and `resolution/lazy_parse.rs`). `name` and `file_namespace` are never mutated
    /// after that point, so the cache never goes stale.
    ///
    /// `None` for classes whose FQN has not been cached (synthetic classes,
    /// classes still under construction); [`fqn`] falls back to computing it
    /// on demand, which is what every caller did unconditionally before.
    ///
    /// Kept out of [`signature_eq`] because it is derived from `name` +
    /// `file_namespace`, both of which are already compared there.
    ///
    /// Construct with `fqn: None` and populate via [`cache_fqn`]; never set a
    /// value here directly.
    pub fqn: Option<Atom>,
}

/// Prefix of the synthetic name the parser gives an anonymous class
/// (`__anonymous@<offset of its opening brace>`).
pub const ANONYMOUS_CLASS_PREFIX: &str = "__anonymous@";

impl ClassInfo {
    /// Return the fully-qualified name of this class.
    ///
    /// Combines `file_namespace` and `name` into a single FQN string
    /// (e.g. `"App\\Models\\User"`).  When no namespace is set, returns
    /// the short name as-is.
    pub fn fqn(&self) -> Atom {
        match self.fqn {
            Some(fqn) => fqn,
            None => self.compute_fqn(),
        }
    }

    /// Compute the FQN from `name` + `file_namespace` without consulting the
    /// cache. Allocates a temporary `String` to intern the joined name when a
    /// namespace is present; returns `name` as-is otherwise (no allocation).
    ///
    /// The nameless placeholder that stands in for top-level code keeps its
    /// empty name: it is no class of its namespace.
    fn compute_fqn(&self) -> Atom {
        match &self.file_namespace {
            Some(ns) if !ns.is_empty() && !self.name.is_empty() => {
                crate::atom::atom(&format!("{}\\{}", ns, self.name))
            }
            _ => self.name,
        }
    }

    /// Populate the cached FQN. Call this once the namespace has been stamped
    /// onto the class (parse/index load), after which `name` and
    /// `file_namespace` are immutable, so the cache cannot go stale.
    #[inline]
    pub fn cache_fqn(&mut self) {
        self.fqn = Some(self.compute_fqn());
    }

    /// Populate the cached FQN for a class parsed out of `uri`.
    ///
    /// Identical to [`cache_fqn`] for a named class. An anonymous class is
    /// named after the offset of its opening brace *within its own file* and
    /// is deliberately kept out of the workspace declaration index, so nothing
    /// else makes it unique across the workspace. Boilerplate-identical files
    /// put that brace on the same offset (every Laravel migration is
    /// `return new class extends Migration {` after the same header), and the
    /// stores keyed by FQN — the resolved-class cache and the method store —
    /// would then let whichever file resolved first answer for both. The URI
    /// goes into the FQN rather than into `name`, which reaches the user
    /// through hover and the outline.
    pub fn cache_fqn_in_uri(&mut self, uri: &str) {
        if self.name.starts_with(ANONYMOUS_CLASS_PREFIX) {
            self.fqn = Some(crate::atom::atom(&format!(
                "{}@{}",
                self.compute_fqn(),
                uri
            )));
        } else {
            self.cache_fqn();
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
            self.method_index
                .push((crate::atom::ascii_lowercase_atom(&method.name), i as u32));
        }
        // Sort by name (then by original index) so lookups can binary
        // search. Sorting the index pairs is stable per name, and
        // deduping keeps the lowest index for each name, matching the
        // first-writer-wins semantics of the old `.iter().find(...)`
        // linear scan when duplicate names exist.
        self.method_index.sort_unstable();
        self.method_index.dedup_by_key(|&mut (name, _)| name);
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
    /// Uses the `method_index` for `O(log n)` binary search when
    /// available, falling back to linear scan otherwise.
    #[inline]
    pub fn get_method(&self, name: &str) -> Option<&MethodInfo> {
        if self.method_index_valid() {
            let atom = crate::atom::ascii_lowercase_atom(name);
            return self
                .method_index
                .binary_search_by_key(&atom, |&(n, _)| n)
                .ok()
                .and_then(|pos| self.methods.get(self.method_index[pos].1 as usize))
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
            return self
                .method_index
                .binary_search_by_key(&atom, |&(n, _)| n)
                .is_ok();
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
                .binary_search_by_key(&atom, |&(n, _)| n)
                .ok()
                .and_then(|pos| self.methods.get(self.method_index[pos].1 as usize))
                .map(Arc::clone);
        }
        self.methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .map(Arc::clone)
    }

    /// Set the raw class-level docblock and re-derive [`Self::doc_members`].
    ///
    /// The two fields describe the same source text, so they must never
    /// drift apart.  Code that builds a `ClassInfo` outside the parser
    /// (tests, synthesized classes) should go through this instead of
    /// assigning `class_docblock` directly.
    pub fn set_class_docblock(&mut self, docblock: Option<String>) {
        self.doc_members = docblock.as_deref().and_then(DocblockMembers::from_docblock);
        self.class_docblock = docblock;
    }

    /// `@method` tags declared on this class's own docblock.
    #[inline]
    pub fn doc_methods(&self) -> &[Arc<MethodInfo>] {
        self.doc_members.as_deref().map_or(&[], |d| &d.methods)
    }

    /// `@property` / `@property-read` / `@property-write` tags declared on
    /// this class's own docblock, as `(name, declared type)`.
    #[inline]
    pub fn doc_properties(&self) -> &[(Atom, Option<PhpType>)] {
        self.doc_members.as_deref().map_or(&[], |d| &d.properties)
    }

    /// Look up a property by name (case-sensitive, per PHP semantics).
    ///
    /// Linear scan: unlike methods there is no property index, because
    /// property lookups are far less frequent than method lookups and
    /// most classes have few properties.
    #[inline]
    pub fn get_property(&self, name: &str) -> Option<&PropertyInfo> {
        self.properties
            .iter()
            .find(|p| p.name == name)
            .map(|arc| arc.as_ref())
    }

    /// Check whether a property with the given name exists (case-sensitive).
    #[inline]
    pub fn has_property(&self, name: &str) -> bool {
        self.properties.iter().any(|p| p.name == name)
    }

    /// Look up a constant by name (case-sensitive, per PHP semantics).
    #[inline]
    pub fn get_constant(&self, name: &str) -> Option<&ConstantInfo> {
        self.constants
            .iter()
            .find(|c| c.name == name)
            .map(|arc| arc.as_ref())
    }

    /// Compare two `ClassInfo` values by signature-relevant fields only.
    ///
    /// Returns `true` when the two classes have identical signatures,
    /// meaning the resolved-class cache entry for this FQN does not need
    /// to be evicted.  This is the key predicate for signature-level
    /// cache invalidation.
    ///
    /// **Ignored fields** (change on every keystroke or are display-only):
    /// - `start_offset`, `end_offset`, `keyword_offset`
    /// - `links`, `see_refs` (display-only `@link`/`@see` references)
    ///
    /// **Compared fields** (affect resolution, inheritance, or virtual
    /// member injection):
    /// - All class-level metadata (`kind`, `name`, `parent_class`, etc.)
    /// - Methods, properties, and constants (compared as name-keyed sets
    ///   so that reordering members in source does not trigger eviction)
    /// - `class_docblock` (adding/removing `@method`/`@property` tags)
    /// - `laravel` metadata (affects virtual member providers)
    pub fn signature_eq(&self, other: &ClassInfo) -> bool {
        self.class_level_signature_eq(other) && self.members_signature_eq(other)
    }

    /// The class-level half of [`signature_eq`](Self::signature_eq):
    /// everything except the declared methods, properties, and constants.
    ///
    /// A change here (a new parent, a different `@mixin`, an edited
    /// class docblock, changed Laravel metadata) can alter *any* member's
    /// resolution, so callers that want to know which members an edit
    /// affected must treat the whole member surface as affected when this
    /// returns `false`.
    pub fn class_level_signature_eq(&self, other: &ClassInfo) -> bool {
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
            || self.require_implements != other.require_implements
            || self.is_final != other.is_final
            || self.is_abstract != other.is_abstract
            || self.is_readonly != other.is_readonly
            || self.deprecation_message != other.deprecation_message
            || self.deprecated_replacement != other.deprecated_replacement
            || self.attribute_targets != other.attribute_targets
            || self.template_params != other.template_params
            || self.template_param_bounds != other.template_param_bounds
            || self.template_param_defaults != other.template_param_defaults
            || self.extends_generics != other.extends_generics
            || self.implements_generics != other.implements_generics
            || self.use_generics != other.use_generics
            || self.type_aliases != other.type_aliases
            || self.unverified_doc_types != other.unverified_doc_types
            || self.trait_precedences != other.trait_precedences
            || self.trait_aliases != other.trait_aliases
            || self.class_docblock != other.class_docblock
            || self.backed_type != other.backed_type
            || self.laravel != other.laravel
        {
            return false;
        }

        true
    }

    /// The member half of [`signature_eq`](Self::signature_eq): the
    /// declared methods, properties, and constants, compared as
    /// name-keyed sets so reordering members in source is not a change.
    pub fn members_signature_eq(&self, other: &ClassInfo) -> bool {
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

    /// Push an `Arc<ClassInfo>` into `results` only if no existing entry
    /// shares the same FQN.  This is the single place where completion /
    /// resolution code deduplicates candidate classes.
    ///
    /// The comparison is on the FQN, not the short name: two classes with
    /// the same short name in different namespaces (`NsA\Thing` and
    /// `NsB\Thing`) are distinct receivers and both must survive, or a
    /// union spanning them loses every member of the second.
    pub(crate) fn push_unique_arc(results: &mut Vec<Arc<ClassInfo>>, cls: Arc<ClassInfo>) {
        let fqn = cls.fqn();
        if !results.iter().any(|c| c.fqn() == fqn) {
            results.push(cls);
        }
    }

    /// Extend `results` with entries from `new_classes`, skipping any whose
    /// FQN already appears in `results`.
    pub(crate) fn extend_unique_arc(
        results: &mut Vec<Arc<ClassInfo>>,
        new_classes: Vec<Arc<ClassInfo>>,
    ) {
        for cls in new_classes {
            Self::push_unique_arc(results, cls);
        }
    }
}
