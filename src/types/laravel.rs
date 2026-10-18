//! The Laravel metadata a class records while it is parsed, and the
//! Eloquent factory count that travels with a resolved type.

use super::*;

/// Flags for the list-valued Eloquent model properties a class declares
/// itself, stored as a bitmask in [`LaravelMetadata::declared`].
///
/// A subclass inherits each of these from the nearest ancestor that
/// declares it, so "declared as `[]`" (which hides the parent's value) has
/// to be told apart from "not declared" (which inherits it). The
/// scalar-valued fields carry that distinction in their own `Option`.
pub mod model_declaration {
    /// The class declares a `$casts` property.
    pub const CASTS_PROPERTY: u16 = 1;
    /// The class declares a `casts()` method.
    pub const CASTS_METHOD: u16 = 1 << 1;
    /// The class declares a `$dates` property.
    pub const DATES: u16 = 1 << 2;
    /// The class declares an `$attributes` property.
    pub const ATTRIBUTES: u16 = 1 << 3;
    /// The class declares a `$fillable` property.
    pub const FILLABLE: u16 = 1 << 4;
    /// The class declares a `$guarded` property.
    pub const GUARDED: u16 = 1 << 5;
    /// The class declares a `$hidden` property.
    pub const HIDDEN: u16 = 1 << 6;
    /// The class declares a `$visible` property.
    pub const VISIBLE: u16 = 1 << 7;
    /// The class declares an `$appends` property.
    pub const APPENDS: u16 = 1 << 8;
    /// The column-name lists that feed [`LaravelMetadata::column_names`](super::LaravelMetadata::column_names).
    pub const COLUMN_LISTS: u16 = FILLABLE | GUARDED | HIDDEN | VISIBLE | APPENDS;
}

/// The `$casts` property and `casts()` method entries of a model, kept
/// apart so a subclass that redeclares one of them still inherits the
/// other.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CastSources {
    /// Entries of the `$casts` property.
    pub property: Vec<(String, String)>,
    /// Entries returned by the `casts()` method.
    pub method: Vec<(String, String)>,
}

/// Laravel-specific metadata extracted from class declarations.
///
/// Grouped into a sub-struct to keep the core `ClassInfo` focused on
/// PHP semantics. All fields default to empty/`None`, so non-Laravel
/// classes carry no overhead beyond a single struct value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LaravelMetadata {
    /// Factory selected by a model's `newFactory()`, `$factory`, or `#[UseFactory]`.
    pub custom_factory: Option<PhpType>,
    /// Model class explicitly configured by a Laravel factory's `$model`
    /// property.
    ///
    /// Both `protected $model = Post::class` and literal class strings are
    /// recognized. The type is resolved to an FQN during the normal
    /// name-resolution pass and provides factory-to-model evidence when
    /// generic PHPDoc or Laravel's naming convention is unavailable.
    pub factory_model: Option<PhpType>,
    /// Custom collection class for Eloquent models.
    ///
    /// Detected from three Laravel mechanisms:
    ///
    /// 1. A `newCollection()` method override returning a custom type.
    /// 2. The `#[CollectedBy(CustomCollection::class)]` attribute on the model.
    /// 3. The `/** @use HasCollection<CustomCollection> */` annotation.
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
    /// The two sources of [`casts_definitions`](Self::casts_definitions),
    /// recorded only when the model has both a `$casts` property and a
    /// `casts()` method (its own or inherited). With a single source,
    /// `casts_definitions` already is that source's list.
    pub cast_sources: Option<Box<CastSources>>,
    /// Which list-valued model properties the class declares itself, as
    /// [`model_declaration`] flags.
    pub declared: u16,
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
    /// The [`model_declaration`] list flags each entry of
    /// [`column_names`](Self::column_names) comes from, index for index.
    ///
    /// Lets a subclass that redeclares `$hidden` still inherit the parent's
    /// `$fillable`. An entry past the end of this list belongs to every
    /// declared column list.
    pub column_sources: Vec<u16>,
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
    /// Columns returned by the model's own `uniqueIds()` override.
    ///
    /// - `None` — not declared, or not a literal list of strings.
    /// - `Some(["uuid"])` — `HasUuids`/`HasUlids` generate these columns;
    ///   the primary key is only a string when it is listed.
    pub unique_ids: Option<Vec<String>>,
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
    /// Whether the model uses Eloquent's `SoftDeletes` trait, which casts
    /// its deletion column to a date.
    pub soft_deletes: bool,
    /// Override for the `DELETED_AT` column name constant `SoftDeletes`
    /// reads. `None` when not declared (the default `"deleted_at"`).
    pub deleted_at_name: Option<String>,
    /// Custom Eloquent builder class for the model.
    ///
    /// Detected from three Laravel mechanisms:
    ///
    /// 1. A `newEloquentBuilder()` method override returning a custom type.
    /// 2. The `#[UseEloquentBuilder(CustomBuilder::class)]` attribute.
    /// 3. The `/** @use HasBuilder<CustomBuilder> */` annotation.
    ///
    /// When set, the `LaravelModelProvider` uses this class instead of
    /// the standard `Illuminate\Database\Eloquent\Builder` for
    /// builder-as-static forwarding and `query()` resolution.
    pub custom_builder: Option<PhpType>,
    /// Authorization policy class declared by the
    /// `#[UsePolicy(PostPolicy::class)]` attribute on the model
    /// (Laravel 11+), resolved to an FQN in the name-resolution pass.
    ///
    /// Takes precedence over Laravel's naming convention when resolving
    /// which policy governs the model, so the abilities a `$user->can(…)`
    /// check may name are read from the declared class.
    pub policy_class: Option<String>,
    /// Pivot configuration recovered from `belongsToMany`/`morphToMany`
    /// relationship method bodies.
    ///
    /// One entry per many-to-many relationship method that declares an
    /// `->as('name')`, `->using(CustomPivot::class)`, and/or
    /// `->withPivot('col', …)` chain.
    /// Populated from the method body during parsing; the `using` class is
    /// resolved to an FQN in the name-resolution pass.  Used to surface the
    /// custom pivot class and extra pivot columns in hover.
    pub belongs_to_many_pivots: Vec<PivotRelation>,
    /// Relation method names and discriminator columns recovered from
    /// returned `$this->morphTo()` calls. A missing column derives its name
    /// from the effective method name, including trait aliases. Kept on the
    /// declaring class or trait so lookup respects method overrides.
    pub morph_type_columns: Vec<(Atom, Option<Atom>)>,
    /// What this class's `getFacadeAccessor()` returns, when it declares
    /// one.
    ///
    /// Recorded at parse time because the accessor's value is only
    /// recoverable from source: the method's declared `: string` return
    /// type hides a `::class` reference, and body-return inference would
    /// report the hint rather than the name. `None` for every class that
    /// does not declare the method, which is all but the handful of
    /// facades in a project.
    pub facade_accessor: Option<FacadeAccessor>,
}

/// What a facade's `getFacadeAccessor()` returns.
///
/// Both payloads are the value exactly as written in source; the
/// [`Class`](Self::Class) name is rewritten to its fully-qualified form
/// during the name-resolution pass (see `parser/name_resolution.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacadeAccessor {
    /// A container-binding string (`return 'view';`), looked up in the
    /// container alias table to find the concrete class.
    Alias(Atom),
    /// A direct class reference (`return Factory::class;`).
    Class(Atom),
}

/// Pivot metadata recovered from a single many-to-many relationship method.
///
/// Corresponds to a `belongsToMany`/`morphToMany` method whose body configures
/// its pivot accessor, model, or columns. Keyed back to the relationship by
/// `method` so the provider can attach it to the synthesized property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PivotRelation {
    /// The relationship method name (e.g. `roles`).
    pub method: String,
    /// The pivot accessor configured by `->as(...)`.
    pub accessor: PivotAccessor,
    /// The custom pivot class from `->using(X::class)`, if declared.
    ///
    /// Stored as the short name during parsing and resolved to an FQN in
    /// the name-resolution pass.
    pub using: Option<String>,
    /// Extra pivot columns declared via `->withPivot('a', 'b', …)`.
    pub columns: Vec<String>,
}

/// The pivot accessor configuration recovered from a relationship body.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PivotAccessor {
    /// No `->as(...)` call is present, so Laravel uses `$pivot`.
    #[default]
    Default,
    /// A literal custom accessor such as `->as('participation')`.
    Custom(Atom),
    /// An accessor was configured dynamically and cannot be named statically.
    Unknown,
}

/// How many models an Eloquent factory chain builds.
///
/// Laravel writes both outcomes of `create()`/`make()` as one
/// `Collection<int, TModel>|TModel` return type, so the branch a call
/// produces is decided by the count the chain set — which can be several
/// calls and one variable assignment away from the `create()` that reads
/// it (`$factory = User::factory(); … $factory->state([…])->create()`).
///
/// The state therefore travels on the value rather than being re-derived
/// from the call's syntax: every fluent call passes it along, and a branch
/// join that disagrees about it drops back to [`Unknown`](Self::Unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FactoryCount {
    /// No count was set, or a previously set count was cleared with
    /// `count(null)`.
    One,
    /// `count()`, `times()`, or a numeric `factory(3)` argument set a
    /// count, so the chain builds a collection.
    Many,
    /// Either outcome is possible: the value is not a factory at all, or
    /// it is one whose count never became visible.
    #[default]
    Unknown,
}

impl FactoryCount {
    /// Join two count states, which agree only on themselves.
    ///
    /// A value that is a single model on one path and a collection on
    /// the other is neither at the join, so `create()` keeps Laravel's
    /// declared union there rather than picking a side.
    pub(crate) fn join(self, other: FactoryCount) -> FactoryCount {
        if self == other {
            self
        } else {
            FactoryCount::Unknown
        }
    }
}

/// The fully-qualified name of the Eloquent Collection class.
///
/// Used by the `LaravelModelProvider` to detect and replace collection
/// return types when a model declares a custom collection class.
pub const ELOQUENT_COLLECTION_FQN: &str = "Illuminate\\Database\\Eloquent\\Collection";

/// The fully-qualified name of the Eloquent `Pivot` class.
///
/// Used to type synthesized pivot accessors on models reached through a
/// many-to-many (`belongsToMany` / `morphToMany`) relationship.
pub const ELOQUENT_PIVOT_FQN: &str = "Illuminate\\Database\\Eloquent\\Relations\\Pivot";
