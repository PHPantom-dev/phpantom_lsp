//! Laravel Eloquent Model virtual member provider.
//!
//! Synthesizes virtual members for classes that extend
//! `Illuminate\Database\Eloquent\Model`.  This is the highest-priority
//! virtual member provider: its contributions beat `@method` /
//! `@property` / `@mixin` members (PHPDocProvider).
//!
//! Currently implements:
//!
//! - **Relationship properties.** Methods returning a known Eloquent
//!   relationship type (e.g. `HasOne`, `HasMany`, `BelongsTo`) produce
//!   a virtual property with the same name.  The property type is
//!   inferred from the relationship's generic parameters (a generic
//!   `@return HasMany<Post, $this>` annotation) or, as a fallback,
//!   from the first `::class` argument in the method body text.
//!
//! - **Relationship count properties.** For each relationship method, a
//!   `{snake_name}_count` property typed `int` is synthesized, matching
//!   the `withCount()`/`loadCount()` convention.  Skipped when a
//!   property of that name already exists.
//!
//! - **Scope methods.** Methods whose name starts with `scope` (e.g.
//!   `scopeActive`, `scopeVerified`) produce a virtual method with the
//!   `scope` prefix stripped and the first letter lowercased (e.g.
//!   `active`, `verified`).  Methods decorated with `#[Scope]`
//!   (Laravel 11+) are also recognized: their own name is used
//!   directly as the public-facing scope name (e.g.
//!   `#[Scope] protected function active()` becomes `active()`).
//!   The first `$query` parameter is removed.
//!   Scope methods are available as both static and instance methods
//!   so they resolve for `User::active()` and `$user->active()`.
//!
//! - **Accessor properties.** Legacy `getXAttribute()` methods and
//!   modern (Laravel 9+) `Attribute`-returning accessors both produce a
//!   virtual property named after the attribute, typed from the
//!   accessor's return type.
//!
//! - **Builder-as-static forwarding.** Laravel's `Model::__callStatic()`
//!   forwards static calls to `static::query()`, which returns an
//!   Eloquent Builder.  This provider loads
//!   `\Illuminate\Database\Eloquent\Builder`, fully resolves it
//!   (including its `@mixin` on `Query\Builder`), and presents its
//!   public instance methods as static virtual methods on the model.
//!   Return types are mapped so that `static`/`$this`/`self` resolve
//!   to `Builder<ConcreteModel>` (the chain continues on the builder)
//!   and template parameters like `TModel` resolve to the concrete
//!   model class.  This makes `User::where(...)->orderBy(...)->get()`
//!   resolve end-to-end.
//!
//! - **Cast properties.** Entries in the `$casts` property array or
//!   `casts()` method body produce typed virtual properties.  Cast type
//!   strings are mapped to PHP types (e.g. `datetime` → `\Carbon\Carbon`,
//!   `boolean` → `bool`, `decimal:2` → `float`).  Custom cast classes
//!   are resolved by loading the class and reading the first generic
//!   argument from an `@implements CastsAttributes<TGet, TSet>`
//!   annotation on the cast class.  When no such annotation is present,
//!   the resolver falls back to the `get()` method's return type.  Enum
//!   casts resolve to the enum class itself.  Classes implementing
//!   `Castable` also resolve to themselves.  A `:argument` suffix (e.g.
//!   `Address::class.':nullable'`) is stripped before resolution.  The
//!   deprecated `$dates` property array is handled the same way, typed
//!   as the configured Laravel date class.
//!
//! - **Attribute default properties.** Entries in the `$attributes`
//!   property array produce typed virtual properties as a fallback.
//!   Types are inferred from the literal default values: strings,
//!   booleans, integers, floats, `null`, and arrays.  Columns that
//!   already have a `$casts` or `$dates` entry are skipped, so those
//!   always take priority.
//!
//! - **Database schema columns.** When a schema dump or migration scan
//!   is available, columns not already covered by `$casts`, `$dates`,
//!   or `$attributes` produce properties typed from the actual column
//!   type, carrying nullability and default-value metadata for hover.
//!
//! - **Implicit primary key.** Every model exposes a primary key column
//!   (`id` by default, respecting `$primaryKey`/`$keyType` overrides)
//!   even when no schema or cast entry describes it, unless the model
//!   overrides `getKeyName()`.
//!
//! - **Timestamp properties.** `created_at`/`updated_at` (or their
//!   configured names) are added as the configured Laravel date class,
//!   unless timestamps are disabled or the columns are already covered.
//!
//! - **Column name properties.** Column names from `$fillable`,
//!   `$guarded`, `$hidden`, and `$appends` produce `mixed`-typed
//!   virtual properties as a last-resort fallback.  Columns already
//!   covered by any of the sources above are skipped.
//!
//! - **`where{PropertyName}()` dynamic methods.** Laravel's
//!   `Builder::__call()` translates calls like `whereBrandId($value)`
//!   into `where('brand_id', $value)`.  For each known column on the
//!   model (from `$casts`, `$dates`, `$attributes` defaults,
//!   `$fillable`/`$guarded`/`$hidden`/`$appends`, timestamps,
//!   `@property` annotations, and properties declared on the class
//!   itself), a virtual `where{StudlyCase}()` method is synthesized.
//!   The method accepts a `mixed` value parameter and returns
//!   `Builder<ConcreteModel>`.  These methods appear as both instance
//!   methods on the Builder (for chaining: `$query->whereBrandId(42)`)
//!   and static methods on the model (for `User::whereName('Alice')`).

mod accessors;
mod aliases;
mod auth;
mod builder;
mod builder_injection;
mod casts;
mod commands;
mod completion_cache;
mod config_keys;
pub(crate) mod config_values;
mod const_eval;
pub(crate) mod database_schema;
mod env_vars;
mod facade;
mod factory;
pub(crate) mod factory_count;
mod folio;
pub(crate) mod gates;
mod helpers;
mod higher_order_proxy;
mod macros;
mod model_extraction;
pub(crate) mod morph_map;
pub(crate) mod patches;
mod path_helpers;
mod pivots;
mod provider_resources;
mod relationships;
mod request_fields;
pub(crate) mod request_input;
mod route_names;
mod scopes;
mod storage;
mod string_keys;
mod trans_keys;
pub(crate) mod validated_shape;
pub(crate) mod validation_rules;
mod view_data;
mod view_names;
pub(crate) mod where_property;

pub(crate) use aliases::{LaravelAliasSlot, new_alias_slot};
pub(crate) use auth::{GUARD_FQN, REQUEST_FQN, patch_auth_user_class, resolve_auth_user_type};
pub(crate) use commands::{
    LaravelCommandIndex, command_signature_at_offset, is_command_accessor,
    is_command_directory_uri, resolve_accessor_type as resolve_command_accessor_type,
    scan_command_file,
};
pub(crate) use config_keys::find_config_references;
pub(crate) use config_keys::{
    collect_laravel_config_declarations, find_all_config_references,
    laravel_config_prefix_from_uri, resolve_config_key_declaration,
    resolve_config_key_definition_fallback,
};
pub(crate) use const_eval::ClassContext;
pub(crate) use env_vars::{enumerate_env_keys, env_declaration, env_name_is_sensitive};
pub(crate) use gates::{
    LaravelGateIndex, enumerate_gate_abilities, model_policy_abilities, scan_gate_registrations,
};
pub(crate) use macros::{
    LaravelMacroIndex, MacroRegistration, extract_date_factory_class, extract_macro_registrations,
    extract_mixin_registrations, inject_macros, macro_closure_this_target,
    parse_installed_providers, parse_provider_class_list, parse_provider_referenced_classes,
    synthesize_mixin_macros,
};
pub(crate) use model_extraction::{
    extract_laravel_metadata, has_scope_attribute, infer_relationship_from_method,
};
pub(crate) use morph_map::{LaravelMorphMapIndex, MorphMapEntry, MorphMapScan, scan_morph_map};
pub(crate) use patches::STORAGE_FACADE_FQN;
pub(crate) use path_helpers::{
    collect_path_helper_links, is_path_helper, path_helper_base, resolve_path_helper_definition,
};
pub(crate) use provider_resources::{
    ProviderIdentity, ProviderOrigin, ProviderResources, ProviderScan, ProviderScans,
    extract_provider_resources,
};
pub(crate) use request_fields::{request_fields_at_position, resolve_request_field_definition};
pub(crate) use route_names::{
    RouteDiscovery, enumerate_all_routes, route_name_matches, route_uri_parameters,
};
pub(crate) use storage::{
    FILESYSTEM_MANAGER_FQN, LaravelStorageDriverIndex, StorageDriverRegistration,
    extract_storage_driver_registrations, is_storage_facade_name, patch_storage_disk_type,
    storage_facade_local_names,
};
pub(crate) use trans_keys::{collect_trans_declarations, trans_line, unresolved_trans_type};
pub(crate) use validation_rules::{safe_call_receiver_variable, safe_source_variable};
pub(crate) use view_data::{SharedViewVar, composer_class_vars};
pub(crate) use view_names::canonical_view_name;

pub(crate) use builder_injection::{try_inject_builder_scopes, try_inject_mixin_builder_scopes};
pub(crate) use higher_order_proxy::{
    inject_higher_order_proxy_members, is_tagged_higher_order_proxy,
};
pub(crate) use string_keys::{find_laravel_string_key_references, resolve_laravel_string_key};

pub use helpers::extends_eloquent_model;
pub(crate) use helpers::walk_all_php_expressions;
pub(crate) use helpers::{accessor_method_candidates, camel_to_snake};

use crate::atom::atom;
pub(crate) use accessors::is_accessor_or_mutator_method;
use accessors::{
    extract_modern_accessor_type, is_legacy_accessor, is_legacy_mutator, is_modern_accessor,
    legacy_accessor_property_name, legacy_mutator_property_name,
};
pub(crate) use where_property::where_property_method_to_column;

pub(crate) use pivots::{LaravelPivotIndex, build_pivot_index, inject_pivot};
pub(crate) use relationships::class_has_relation_method_ci;
pub(crate) use relationships::classify_relationship_typed;
pub(crate) use relationships::count_property_to_relationship_method;
pub use relationships::infer_relationship_from_body;
pub(crate) use relationships::{RELATION_QUERY_METHODS, resolve_relation_chain};
use relationships::{
    RelationshipKind, build_property_type, count_property_name, extract_pivot_accessor_typed,
    extract_related_type_typed,
};
pub(crate) use relationships::{
    class_declares_pivot_relationship, extract_pivot_accessor, extract_pivot_using,
    extract_with_pivot_columns,
};

pub use scopes::build_scope_methods_for_builder;
use scopes::{build_scope_methods, is_scope_method};
use where_property::{build_where_property_methods_for_class, lowercase_method_names};

use std::collections::HashMap;
use std::sync::Arc;

use builder::build_builder_forwarded_methods;
use casts::cast_type_to_php_type;
pub use facade::LaravelFacadeProvider;
pub use factory::LaravelFactoryProvider;
pub(crate) use factory::{
    factory_model_type, is_factory_class, is_has_factory_trait, model_to_factory_fqn,
};
pub(crate) use factory_count::{
    carry_factory_count, fluent_factory_count, resolve_factory_count_return,
    resolve_factory_count_return_ast, tag_static_factory_call,
};

use crate::atom::{AtomSet, ascii_lowercase_atom};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{
    AttributeDefaultSource, ClassInfo, DatabaseColumnSource, ELOQUENT_COLLECTION_FQN,
    MAX_INHERITANCE_DEPTH, PivotAccessor, PropertyInfo, PropertySource,
};

use super::resolve::resolve_class_base_cached;
use super::{ResolvedClassCache, VirtualMemberProvider, VirtualMembers};
use database_schema::SchemaTable;

/// The fully-qualified name of the Eloquent base model.
pub(crate) const ELOQUENT_MODEL_FQN: &str = "Illuminate\\Database\\Eloquent\\Model";

/// The fully-qualified name of the Eloquent Builder class.
pub const ELOQUENT_BUILDER_FQN: &str = "Illuminate\\Database\\Eloquent\\Builder";

/// The fully-qualified name of Laravel's concrete Carbon subclass, which
/// the `now()` and `today()` helpers actually instantiate.
pub const SUPPORT_CARBON_FQN: &str = "Illuminate\\Support\\Carbon";

/// Internal class-loader key for the class selected through `Date::use()`.
pub const CONFIGURED_DATE_CLASS_FQN: &str = "phpantom-configured-laravel-date-class";

/// The fully-qualified name of the concrete view object Laravel's view
/// factory builds, which the `view()` helper hands back.
pub const VIEW_FQN: &str = "Illuminate\\View\\View";

/// Whether a call to the `view()` helper names a template, and therefore
/// returns a rendered view rather than the view factory.
///
/// The helper's declared return type is
/// `($view is null ? Contracts\View\Factory : Contracts\View\View)`, but
/// the factory always constructs the concrete `Illuminate\View\View`.
/// Resolving to the contract loses that and reports a mismatch on the
/// (correct) `render(): View` signature every Blade component writes.
/// Mapping the named form to the concrete class mirrors the `view()` stub
/// the Laravel PHPStan extensions ship, which the ecosystem is written
/// against.
pub(crate) fn view_helper_returns_view(func_name: &str, text_args: &str) -> bool {
    if func_name.trim_start_matches('\\') != "view" {
        return false;
    }
    let first = text_args.trim();
    !first.is_empty() && !first.eq_ignore_ascii_case("null")
}

/// Build a substitution map that replaces `static`, `$this`, and `self`
/// with the given type.
///
/// This is used across multiple Laravel virtual member providers
/// (builder forwarding, model virtual methods, scope methods) to
/// resolve self-referencing return types to concrete model or builder
/// types.
pub(super) fn self_ref_subs(ty: PhpType) -> HashMap<String, PhpType> {
    HashMap::from([
        ("static".to_owned(), ty.clone()),
        ("$this".to_owned(), ty.clone()),
        ("self".to_owned(), ty),
    ])
}

// ─── Type-resolution helpers ────────────────────────────────────────────────
//
// Called from `type_engine/types/resolution.rs`
// (`type_hint_to_classes_typed_depth`) and the call/return-type resolvers
// under `type_engine/` to apply Eloquent-specific post-processing after a
// class has been resolved and generic substitution applied.  Keeping the
// framework logic here rather than inline in the generic resolver avoids
// coupling the type engine to Laravel conventions.

/// Swap a resolved Eloquent Collection to a model's custom collection.
///
/// When the resolved class is `Illuminate\Database\Eloquent\Collection`
/// and one of the generic type arguments is a model with a
/// `custom_collection` declared (via `#[CollectedBy]` or
/// `@use HasCollection<X>`), returns the custom collection class
/// instead.  This handles the common chain pattern:
///
/// ```php
/// Model::where(...)->get()  // returns Collection<int, TModel>
/// ```
///
/// where `TModel` has been substituted to the concrete model and the
/// model declares a custom collection like `ProductCollection`.
///
/// Returns `None` when the class is not the Eloquent Collection, has no
/// generic args, or the model does not declare a custom collection.
pub(crate) fn try_swap_custom_collection(
    cls: ClassInfo,
    base_fqn: &str,
    generic_args: &[PhpType],
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> ClassInfo {
    if base_fqn != crate::types::ELOQUENT_COLLECTION_FQN || generic_args.is_empty() {
        return cls;
    }

    // The last generic arg is typically the model type.
    let model_name = match generic_args.last().unwrap().base_name() {
        Some(name) => name.to_string(),
        None => return cls,
    };
    let model_class = find_class_in(all_classes, &model_name)
        .cloned()
        .or_else(|| class_loader(&model_name).map(Arc::unwrap_or_clone));

    if let Some(ref mc) = model_class
        && let Some(coll_type) = mc.laravel().and_then(|l| l.custom_collection.as_ref())
    {
        let coll_name = coll_type.to_string();
        find_class_in(all_classes, &coll_name)
            .cloned()
            .or_else(|| class_loader(&coll_name).map(Arc::unwrap_or_clone))
            .unwrap_or(cls)
    } else {
        cls
    }
}

/// Find a class in a slice by name (short or FQN).
///
/// Minimal local lookup used by the collection-swap helper.  Prefers
/// namespace-aware matching when the name contains backslashes.
fn find_class_in<'a>(all_classes: &'a [Arc<ClassInfo>], name: &str) -> Option<&'a ClassInfo> {
    let short = name.rsplit('\\').next().unwrap_or(name);

    if name.contains('\\') {
        let expected_ns = name.rsplit_once('\\').map(|(ns, _)| ns);
        all_classes
            .iter()
            .find(|c| c.name == short && c.file_namespace.as_deref() == expected_ns)
            .map(|c| c.as_ref())
    } else {
        all_classes
            .iter()
            .find(|c| c.name == short)
            .map(|c| c.as_ref())
    }
}

/// Rewrite every `Illuminate\Database\Eloquent\Collection<…, TModel>`
/// node in a type to the collection class the model actually builds.
///
/// Laravel's own `Builder::get()`, `Relation::get()` and friends are
/// annotated `@return Collection<int, TModel>`.  A model that declares a
/// custom collection (via `#[CollectedBy]`, `@use HasCollection<X>`, or a
/// `newCollection()` override) hands back that subclass at runtime, so the
/// declared base type is wrong for every such model.  Substituting
/// `TModel` alone leaves `Collection<int, Audience>` where the code
/// (correctly) declares `AudienceCollection`.
///
/// The model is therefore read off the *last* generic argument, so a builder that
/// returns some other model's collection resolves to that model's
/// collection class rather than the receiver's.
///
/// The generic arity of the result follows the target collection class:
/// a collection with two template parameters keeps `<key, model>`, one
/// keeps `<model>`, and a non-generic subclass (the common
/// `@extends Collection<int, Model>` shape) becomes a bare class name.
///
/// Returns `None` when nothing was rewritten, so callers on the hot path
/// keep their existing type without allocating a copy.
pub(crate) fn replace_eloquent_collections_in_type(
    ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    if !mentions_eloquent_collection(ty) {
        return None;
    }
    rewrite_eloquent_collections(ty, class_loader)
}

/// Cheap pre-check for [`replace_eloquent_collections_in_type`].
///
/// Walking the tree twice is still cheaper than cloning it: the vast
/// majority of return types never name the Eloquent collection, and this
/// pass allocates nothing.
fn mentions_eloquent_collection(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Generic(g) => {
            is_eloquent_collection_name(&g.name) || g.args.iter().any(mentions_eloquent_collection)
        }
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            members.iter().any(mentions_eloquent_collection)
        }
        TypeKind::Nullable(inner) | TypeKind::Array(inner) => mentions_eloquent_collection(inner),
        _ => false,
    }
}

fn is_eloquent_collection_name(name: &str) -> bool {
    name.trim_start_matches('\\') == ELOQUENT_COLLECTION_FQN
}

fn rewrite_eloquent_collections(
    ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    match ty.kind() {
        TypeKind::Generic(g) if is_eloquent_collection_name(&g.name) => {
            let model = g.args.last()?.base_name()?;
            let collection = custom_collection_for_model(model, class_loader)?;
            Some(collection_type_for(&collection, &g.args, class_loader))
        }
        TypeKind::Generic(g) => {
            let args = rewrite_members(&g.args, class_loader)?;
            Some(PhpType::generic_atom(g.name, args))
        }
        TypeKind::Union(members) => Some(PhpType::union(rewrite_members(members, class_loader)?)),
        TypeKind::Intersection(members) => Some(PhpType::intersection(rewrite_members(
            members,
            class_loader,
        )?)),
        TypeKind::Nullable(inner) => Some(PhpType::nullable(rewrite_eloquent_collections(
            inner,
            class_loader,
        )?)),
        TypeKind::Array(inner) => Some(PhpType::array_of(rewrite_eloquent_collections(
            inner,
            class_loader,
        )?)),
        _ => None,
    }
}

/// Rewrite a list of type members, returning `None` when none changed.
fn rewrite_members(
    members: &[PhpType],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<Vec<PhpType>> {
    let mut changed = false;
    let rewritten: Vec<PhpType> = members
        .iter()
        .map(|m| match rewrite_eloquent_collections(m, class_loader) {
            Some(new) => {
                changed = true;
                new
            }
            None => m.clone(),
        })
        .collect();
    changed.then_some(rewritten)
}

/// Look up the custom collection declared by a model or, failing that,
/// inherited from one of its parents.
///
/// `#[CollectedBy]`, `$collectionClass` and `newCollection()` overrides
/// all live on the class that declares them but apply to every subclass,
/// so a shared base model can set the collection for a whole hierarchy.
fn custom_collection_for_model(
    model: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    let mut current = class_loader(model)?;
    for _ in 0..MAX_INHERITANCE_DEPTH {
        if let Some(collection) = current.laravel().and_then(|l| l.custom_collection.as_ref()) {
            return collection.base_name().map(str::to_owned);
        }
        current = class_loader(current.parent_class.as_ref()?)?;
    }
    None
}

/// Build the replacement type for a custom collection class, matching its
/// generic arity to the class's own template parameters.
fn collection_type_for(
    collection: &str,
    base_args: &[PhpType],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> PhpType {
    let arity = class_loader(collection).map_or(0, |c| c.template_params.len());
    match arity {
        0 => PhpType::named(atom(collection)),
        1 => PhpType::generic(
            collection,
            vec![base_args.last().cloned().unwrap_or_else(PhpType::mixed)],
        ),
        _ if arity >= base_args.len() => PhpType::generic(collection, base_args.to_vec()),
        // More arguments than the target accepts: keep the trailing ones,
        // which are the value types (`<key, model>` → `<model>`).
        n => PhpType::generic(collection, base_args[base_args.len() - n..].to_vec()),
    }
}

/// Virtual member provider for Laravel Eloquent models.
///
/// When a class extends `Illuminate\Database\Eloquent\Model` (directly
/// or through an intermediate parent), this provider synthesizes the
/// full set of virtual members described in the module documentation
/// above: relationship properties, scope methods, builder-forwarded
/// methods, cast/attribute/column properties, and `where{Property}()`
/// dynamic methods.
///
/// For example, a method `posts()` returning `HasMany<Post, $this>`
/// produces a virtual property `$posts` with type
/// `\Illuminate\Database\Eloquent\Collection<Post>`.
pub struct LaravelModelProvider;

/// Laravel date type used for date-related virtual properties.
fn carbon_type() -> PhpType {
    PhpType::named(atom(CONFIGURED_DATE_CLASS_FQN))
}

fn timestamp_columns(laravel: &crate::types::LaravelMetadata) -> Vec<String> {
    if !laravel.timestamps.unwrap_or(true) {
        return Vec::new();
    }
    let mut columns = Vec::new();
    if let Some(col) = match &laravel.created_at_name {
        Some(Some(name)) => Some(name.clone()),
        Some(None) => None,
        None => Some("created_at".to_string()),
    } {
        columns.push(col);
    }
    if let Some(col) = match &laravel.updated_at_name {
        Some(Some(name)) => Some(name.clone()),
        Some(None) => None,
        None => Some("updated_at".to_string()),
    } {
        columns.push(col);
    }
    columns
}

/// The table a model maps to: its declared `$table`, or Eloquent's snake-case
/// plural of the class's short name.
///
/// Returns `None` when the model overrides `getTable()`, since the table is
/// then computed at runtime.  Used by the morph-map scanner to derive the
/// aliases of a `Relation::morphMap([Post::class, …])` list registration, which
/// Laravel keys by table name.
pub(crate) fn model_table_name(class: &ClassInfo) -> Option<String> {
    let laravel = class.laravel()?;
    if let Some(table) = laravel.table_name.clone() {
        return Some(table);
    }
    if laravel.has_get_table_method {
        return None;
    }
    Some(default_table_name(&class.name))
}

fn model_connection_and_table(
    class: &ClassInfo,
    cache: Option<&ResolvedClassCache>,
) -> Option<(String, String)> {
    let laravel = class.laravel()?;
    let cache_read = cache?.read();
    let schema = cache_read.schema_index();
    let connection = if let Some(connection) = laravel.connection_name.clone() {
        connection
    } else if laravel.has_get_connection_name_method {
        return None;
    } else {
        schema.default_connection.clone()?
    };
    let table = if let Some(table) = laravel.table_name.clone() {
        table
    } else if laravel.has_get_table_method {
        return None;
    } else {
        default_table_name(&class.name)
    };
    Some((connection, table))
}

fn model_schema_table(
    class: &ClassInfo,
    cache: Option<&ResolvedClassCache>,
) -> Option<SchemaTable> {
    let (connection, table) = model_connection_and_table(class, cache)?;
    cache?
        .read()
        .schema_index()
        .table(&connection, &table)
        .cloned()
}

fn default_table_name(class_name: &str) -> String {
    let short = class_name
        .rsplit('\\')
        .next()
        .unwrap_or(class_name)
        .rsplit('/')
        .next()
        .unwrap_or(class_name);
    pluralize_snake_table_name(&camel_to_snake(short))
}

fn pluralize_snake_table_name(name: &str) -> String {
    if let Some((prefix, last)) = name.rsplit_once('_') {
        return format!("{}_{}", prefix, pluralize_english_word(last));
    }
    pluralize_english_word(name)
}

fn pluralize_english_word(word: &str) -> String {
    if word.ends_with('y')
        && !matches!(word.chars().rev().nth(1), Some('a' | 'e' | 'i' | 'o' | 'u'))
    {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with('s')
        || word.ends_with('x')
        || word.ends_with('z')
        || word.ends_with("ch")
        || word.ends_with("sh")
    {
        format!("{}es", word)
    } else {
        format!("{}s", word)
    }
}

fn schema_column_source(table: Option<&SchemaTable>, column: &str) -> Option<DatabaseColumnSource> {
    table?.column_source(column)
}

fn attribute_default_source(class: &ClassInfo, column: &str) -> Option<AttributeDefaultSource> {
    class
        .laravel()?
        .attribute_defaults
        .iter()
        .find(|(name, _)| name == column)
        .map(|(_, value)| AttributeDefaultSource {
            value: value.clone(),
        })
}

fn relationship_kind_name(kind: RelationshipKind) -> &'static str {
    match kind {
        RelationshipKind::Singular => "singular",
        RelationshipKind::Collection => "collection",
        RelationshipKind::MorphTo => "morphTo",
    }
}

fn mutator_methods_by_property(class: &ClassInfo) -> HashMap<String, String> {
    let mut mutators = HashMap::new();
    for method in &class.methods {
        if is_legacy_mutator(method) {
            mutators.insert(
                legacy_mutator_property_name(&method.name),
                method.name.to_string(),
            );
        }
    }
    mutators
}

fn push_or_replace_property(properties: &mut Vec<PropertyInfo>, property: PropertyInfo) {
    if let Some(existing) = properties.iter_mut().find(|p| p.name == property.name) {
        *existing = property;
    } else {
        properties.push(property);
    }
}

impl VirtualMemberProvider for LaravelModelProvider {
    /// Returns `true` if the class extends `Illuminate\Database\Eloquent\Model`.
    fn applies_to(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> bool {
        extends_eloquent_model(class, class_loader)
    }

    /// Scan the class's methods for Eloquent relationship return types,
    /// scope methods, Builder-as-static forwarded methods, `$casts`
    /// definitions, `$attributes` defaults, and `$fillable`/`$guarded`/
    /// `$hidden`/`$appends` column names.
    fn provide(
        &self,
        class: &ClassInfo,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cache: Option<&ResolvedClassCache>,
    ) -> VirtualMembers {
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut seen_props: std::collections::HashSet<String> = std::collections::HashSet::new();
        let schema_table = model_schema_table(class, cache);

        // Method names declared by the base Eloquent Model, resolved with
        // inheritance (own + traits + parent chain) but without virtual
        // members.  Anything the framework's own Model declares is an
        // internal — the relationship builder API (`hasMany`, `belongsTo`)
        // and its protected helpers (`newHasOne`, `morphEagerTo`) all
        // return relationship types — and must not be mistaken for a
        // user-defined member.
        //
        // Collected into a set rather than probed with `has_method`: the
        // base-resolved Model carries no valid method index, so each probe
        // would be a linear scan over ~500 framework methods, once per
        // method of every model.  Keys are lowercased because PHP method
        // names are case-insensitive.
        let base_model_methods: AtomSet = class_loader(ELOQUENT_MODEL_FQN)
            .map(|base| {
                resolve_class_base_cached(&base, class_loader)
                    .methods
                    .iter()
                    .map(|m| ascii_lowercase_atom(&m.name))
                    .collect()
            })
            .unwrap_or_default();
        let mutator_methods = mutator_methods_by_property(class);

        // ── Cast properties ─────────────────────────────────────────
        if let Some(laravel) = class.laravel() {
            for (column, cast_type) in &laravel.casts_definitions {
                let php_type = cast_type_to_php_type(cast_type, class_loader);
                seen_props.insert(column.clone());
                properties.push(PropertyInfo {
                    source: Some(PropertySource::Cast {
                        cast: cast_type.clone(),
                        column: schema_column_source(schema_table.as_ref(), column),
                        attribute_default: attribute_default_source(class, column),
                        mutator: mutator_methods.get(column).cloned(),
                    }),
                    ..PropertyInfo::virtual_property_typed(column, Some(&php_type))
                });
            }

            // ── $dates properties (deprecated, lower priority than $casts) ──
            // Columns in `$dates` are typed as Carbon\Carbon unless already
            // covered by an explicit `$casts` entry.
            for column in &laravel.dates_definitions {
                if !seen_props.insert(column.clone()) {
                    continue;
                }
                properties.push(PropertyInfo {
                    source: Some(PropertySource::Cast {
                        cast: "date".to_string(),
                        column: schema_column_source(schema_table.as_ref(), column),
                        attribute_default: attribute_default_source(class, column),
                        mutator: mutator_methods.get(column).cloned(),
                    }),
                    ..PropertyInfo::virtual_property_typed(column, Some(&carbon_type()))
                });
            }

            // ── Attribute default properties (fallback) ─────────────
            // Only add properties for columns not already covered by $casts
            // or $dates.
            for (column, php_type) in &laravel.attributes_definitions {
                if !seen_props.insert(column.clone()) {
                    continue;
                }
                properties.push(PropertyInfo {
                    source: attribute_default_source(class, column).map(|default| {
                        PropertySource::AttributeDefault {
                            default,
                            column: schema_column_source(schema_table.as_ref(), column),
                            mutator: mutator_methods.get(column).cloned(),
                        }
                    }),
                    ..PropertyInfo::virtual_property_typed(column, Some(php_type))
                });
            }

            let timestamp_columns = timestamp_columns(laravel);

            if let Some(schema_table) = &schema_table {
                for column in &schema_table.columns {
                    if !seen_props.insert(column.name.clone()) {
                        continue;
                    }
                    let php_type = if timestamp_columns.contains(&column.name) {
                        carbon_type()
                    } else {
                        column.php_type.clone()
                    };
                    properties.push(PropertyInfo {
                        source: Some(PropertySource::DatabaseColumn {
                            column: DatabaseColumnSource {
                                connection: schema_table.connection.clone(),
                                table: schema_table.name.clone(),
                                column: column.name.clone(),
                                database_type: column.database_type.clone(),
                                nullable: column.nullable,
                                default: column.default.clone(),
                                generated_expression: column.generated_expression.clone(),
                                generated_mode: column.generated_mode.clone(),
                            },
                            attribute_default: attribute_default_source(class, &column.name),
                            mutator: mutator_methods.get(&column.name).cloned(),
                        }),
                        ..PropertyInfo::virtual_property_typed(&column.name, Some(&php_type))
                    });
                }
            }

            // ── Implicit primary key ────────────────────────────────
            // Every Eloquent model exposes a primary key column (default
            // `id`) even when no migration or schema dump describes the
            // table. Synthesize it when schema/casts/attributes have not
            // already provided it, respecting `$primaryKey` and `$keyType`
            // overrides. Skip when `getKeyName()` is declared, since the key
            // name is then computed at runtime and cannot be resolved here.
            if !laravel.has_get_key_name_method {
                let primary_key = laravel.primary_key.as_deref().unwrap_or("id");
                if seen_props.insert(primary_key.to_string()) {
                    let php_type = if laravel.key_type.as_deref() == Some("string") {
                        PhpType::string()
                    } else {
                        PhpType::int()
                    };
                    properties.push(PropertyInfo::virtual_property_typed(
                        primary_key,
                        Some(&php_type),
                    ));
                }
            }

            // ── Timestamp properties ────────────────────────────────
            // Add timestamp properties only when schema/casts/attributes did
            // not already provide the column. Schema-backed timestamp columns
            // keep their database source and use the configured Laravel date
            // class above.
            for column in timestamp_columns {
                if seen_props.insert(column.clone()) {
                    properties.push(PropertyInfo::virtual_property_typed(
                        &column,
                        Some(&carbon_type()),
                    ));
                }
            }

            // ── Column name properties (last-resort fallback) ───────
            // $fillable, $guarded, $hidden, and $appends provide column
            // names without type info.  Only add those not already covered.
            for column in &laravel.column_names {
                if !seen_props.insert(column.clone()) {
                    continue;
                }
                properties.push(PropertyInfo::virtual_property_typed(
                    column,
                    Some(&PhpType::mixed()),
                ));
            }
        }

        for method in &class.methods {
            // Framework internals inherited from the base Model never
            // describe a member of the user's model.
            if base_model_methods.contains(&ascii_lowercase_atom(&method.name)) {
                continue;
            }

            // ── Scope methods ───────────────────────────────────────
            if is_scope_method(method) {
                // A method that is both `#[Scope]`-attributed and named
                // `scopeX` keeps the attribute's name as-is; the prefix
                // is only stripped for convention-based scope methods.
                let [instance_method, static_method] = build_scope_methods(method);
                methods.push(Arc::new(instance_method));
                methods.push(Arc::new(static_method));
                continue;
            }

            // ── Legacy accessors (getXAttribute) ────────────────────
            if is_legacy_accessor(method) {
                let prop_name = legacy_accessor_property_name(&method.name);
                let column = schema_column_source(schema_table.as_ref(), &prop_name);
                let source = if column.is_some() {
                    PropertySource::Accessor {
                        method: method.name.to_string(),
                        mutator: mutator_methods.get(&prop_name).cloned(),
                        column,
                    }
                } else {
                    PropertySource::ComputedProperty {
                        method: method.name.to_string(),
                        mutator: mutator_methods.get(&prop_name).cloned(),
                    }
                };
                push_or_replace_property(
                    &mut properties,
                    PropertyInfo {
                        deprecation_message: method.deprecation_message.clone(),
                        source: Some(source),
                        ..PropertyInfo::virtual_property_typed(
                            &prop_name,
                            method.return_type.as_ref(),
                        )
                    },
                );
                continue;
            }

            // ── Modern accessors (Laravel 9+ Attribute casts) ───────
            if is_modern_accessor(method) {
                let prop_name = camel_to_snake(&method.name);
                let accessor_type = extract_modern_accessor_type(method);
                let column = schema_column_source(schema_table.as_ref(), &prop_name);
                let source = if column.is_some() {
                    PropertySource::Accessor {
                        method: method.name.to_string(),
                        mutator: mutator_methods.get(&prop_name).cloned(),
                        column,
                    }
                } else {
                    PropertySource::ComputedProperty {
                        method: method.name.to_string(),
                        mutator: mutator_methods.get(&prop_name).cloned(),
                    }
                };
                push_or_replace_property(
                    &mut properties,
                    PropertyInfo {
                        deprecation_message: method.deprecation_message.clone(),
                        source: Some(source),
                        ..PropertyInfo::virtual_property_typed(&prop_name, Some(&accessor_type))
                    },
                );
                continue;
            }

            // ── Relationship properties ─────────────────────────────
            let return_type = match method.return_type.as_ref() {
                Some(rt) => rt,
                None => continue,
            };

            let kind = match classify_relationship_typed(return_type) {
                Some(k) => k,
                None => continue,
            };

            let related_type = extract_related_type_typed(return_type);

            // For collection relationships, use the *related* model's
            // custom_collection, not the owning model's.  For example,
            // if Product has `#[CollectedBy(ProductCollection)]` and
            // Review has `#[CollectedBy(ReviewCollection)]`, then
            // `Product::reviews()` returning `HasMany<Review, $this>`
            // should produce `ReviewCollection<Review>`, not
            // `ProductCollection<Review>`.
            let custom_collection = if kind == RelationshipKind::Collection {
                related_type.and_then(|t| {
                    // A self-referential relation (`HasMany<self, $this>`)
                    // names the owning model, so the keyword has to be
                    // resolved before the model can be looked up.
                    let model = if t.is_self_ref() {
                        class.fqn().to_string()
                    } else {
                        t.base_name()?.to_string()
                    };
                    custom_collection_for_model(&model, class_loader)
                })
            } else {
                None
            };

            let type_hint = build_property_type(kind, related_type, custom_collection.as_deref());

            if let Some(ref th) = type_hint {
                // Attach any pivot configuration recovered from the
                // relationship body (`->as(...)` / `->using(...)` /
                // `->withPivot(...)`) so hover can surface the custom
                // accessor, pivot class, and extra columns.
                let generic_pivot_accessor = extract_pivot_accessor_typed(return_type);
                let (body_pivot_accessor, pivot_using, pivot_columns) = class
                    .laravel()
                    .and_then(|l| {
                        l.belongs_to_many_pivots
                            .iter()
                            .find(|p| p.method == method.name.as_str())
                    })
                    .map(|p| {
                        let accessor = match p.accessor {
                            PivotAccessor::Custom(name) => Some(name),
                            PivotAccessor::Default | PivotAccessor::Unknown => None,
                        };
                        (accessor, p.using.clone(), p.columns.clone())
                    })
                    .unwrap_or_default();
                let pivot_accessor = generic_pivot_accessor.or(body_pivot_accessor);
                properties.push(PropertyInfo {
                    source: Some(PropertySource::Relationship {
                        method: method.name.to_string(),
                        kind: relationship_kind_name(kind).to_string(),
                        pivot_accessor,
                        pivot_using,
                        pivot_columns,
                    }),
                    ..PropertyInfo::virtual_property_typed(&method.name, Some(th))
                });
            }
        }

        // ── Relationship count properties (`*_count`) ───────────────
        // `withCount`/`loadCount` is one of the most common Eloquent
        // patterns.  For each relationship method, synthesize a
        // `{snake_name}_count` property typed as `int`.  Skip if a
        // property with that name already exists (e.g. from an explicit
        // `@property` tag).
        for method in &class.methods {
            if base_model_methods.contains(&ascii_lowercase_atom(&method.name)) {
                continue;
            }
            let return_type = match method.return_type.as_ref() {
                Some(rt) => rt,
                None => continue,
            };
            if classify_relationship_typed(return_type).is_none() {
                continue;
            }
            let count_name = count_property_name(&method.name);
            if !seen_props.insert(count_name.clone()) {
                continue;
            }
            properties.push(PropertyInfo {
                source: Some(PropertySource::RelationshipCount {
                    relationship: method.name.to_string(),
                }),
                ..PropertyInfo::virtual_property_typed(&count_name, Some(&PhpType::int()))
            });
        }

        // ── Builder-as-static forwarding ────────────────────────────
        let forwarded = build_builder_forwarded_methods(class, class_loader, cache);
        methods.extend(forwarded);

        // ── where{PropertyName}() static forwarding ─────────────────
        // Laravel's Model::__callStatic() delegates to Builder, which
        // handles where{Column}() calls.  Synthesize these as static
        // methods on the model so that User::whereName('Alice') resolves.
        let existing = lowercase_method_names(&methods);
        let where_static = build_where_property_methods_for_class(class, &existing);
        for mut m in where_static {
            m.is_static = true;
            methods.push(Arc::new(m));
        }

        VirtualMembers {
            methods,
            properties,
            constants: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests;
