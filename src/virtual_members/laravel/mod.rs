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
//!   overrides `getKeyName()`. `HasUuids` and `HasUlids` make the key a
//!   string, including when inherited or composed through other traits,
//!   unless a `uniqueIds()` override leaves the key out.
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
pub(crate) mod array_file;
mod auth;
mod builder;
mod builder_injection;
mod cache_state;
mod cast_literals;
mod casts;
mod collections;
mod commands;
mod completion_cache;
mod config_keys;
pub(crate) mod config_values;
mod const_eval;
pub(crate) mod database_schema;
mod env_vars;
mod expr_walker;
mod facade;
mod factory;
pub(crate) mod factory_count;
pub(crate) mod file_contributions;
mod folio;
pub(crate) mod gates;
pub(crate) mod helpers;
mod higher_order_proxy;
mod inflector;
mod macros;
mod magic_uses;
mod method_refinement;
mod model_extraction;
mod model_provider;
mod model_type;
mod morph_columns;
pub(crate) mod morph_map;
pub(crate) mod patches;
mod path_helpers;
mod pivots;
mod provider_resources;
mod relationships;
mod request_fields;
pub(crate) mod request_input;
mod route_groups;
mod route_names;
mod scopes;
mod storage;
mod string_keys;
mod table_names;
mod trans_catalog;
mod trans_json;
mod trans_keys;
pub(crate) mod typed_morph_columns;
mod unique_ids;
pub(crate) mod validated_shape;
pub(crate) mod validation_rules;
mod view_data;
mod view_names;
pub(crate) mod where_property;

pub(crate) use aliases::{LaravelAliasSlot, new_alias_slot};
pub(crate) use auth::{
    AUTH_FACADE_FQN, GUARD_FQN, REQUEST_FQN, patch_auth_user_class, resolve_auth_user_type,
};
pub(crate) use cache_state::LaravelCacheState;
pub(crate) use collections::{
    mentions_eloquent_collection, replace_eloquent_collections_in_param_type,
    replace_eloquent_collections_in_type, try_swap_custom_collection,
};
pub(crate) use commands::{
    EnclosingCommand, LaravelCommandIndex, command_enclosing_signature,
    command_signature_at_offset, is_command_accessor, is_command_directory_uri,
    resolve_accessor_type as resolve_command_accessor_type, scan_command_file,
};
pub(crate) use config_keys::find_config_references;
pub(crate) use config_keys::{
    find_all_config_references, resolve_config_key_declaration,
    resolve_config_key_declaration_exact, resolve_config_key_definition_fallback,
};
pub(crate) use const_eval::ClassContext;
pub(crate) use env_vars::{enumerate_env_keys, env_declaration, env_name_is_sensitive};
pub(crate) use gates::{
    LaravelGateIndex, enumerate_gate_abilities, model_policy_abilities, scan_gate_registrations,
};
pub(crate) use inflector::singularize_english_word;
pub(crate) use macros::{
    LaravelMacroIndex, MacroRegistration, extract_date_factory_class, extract_macro_registrations,
    extract_mixin_registrations, inject_macros, macro_closure_this_target,
    parse_installed_providers, parse_provider_class_list, parse_provider_referenced_classes,
    synthesize_mixin_macros,
};
pub(crate) use model_extraction::{
    attribute_entry_offset, extract_laravel_metadata, extract_laravel_trait_metadata,
    has_inheritable_model_metadata, has_scope_attribute, inherit_model_metadata,
    relationship_return_type,
};
pub(crate) use model_type::{expand_model_type, has_model_type_operator};
pub(crate) use morph_columns::is_morph_type_column;
pub(crate) use morph_map::{LaravelMorphMapIndex, MorphMapEntry, MorphMapScan, scan_morph_map};
pub(crate) use patches::STORAGE_FACADE_FQN;
pub(crate) use path_helpers::{
    collect_path_helper_links, is_path_helper, path_helper_base, resolve_path_helper_definition,
};
#[cfg(test)]
pub(crate) use provider_resources::ProviderResource;
pub(crate) use provider_resources::{
    ProviderIdentity, ProviderOrigin, ProviderResources, ProviderScan, ProviderScans,
    extract_provider_resources,
};
pub(crate) use request_fields::{request_fields_at_position, resolve_request_field_definition};
pub(crate) use route_names::{
    RouteDiscovery, enumerate_all_routes, find_route_registration_references, route_name_matches,
    route_uri_parameters,
};
pub(crate) use storage::{
    FILESYSTEM_MANAGER_FQN, LaravelStorageDriverIndex, StorageDriverRegistration,
    extract_storage_driver_registrations, patch_storage_disk_type,
};
pub(crate) use table_names::model_table_name;
pub(crate) use trans_catalog::TranslationCatalog;
pub(crate) use trans_json::find_json_trans_references;
pub(crate) use trans_keys::unresolved_trans_type;
pub(crate) use validation_rules::{safe_call_receiver_variable, safe_source_variable};
pub(crate) use view_data::{SharedViewVar, composer_class_vars, is_view_facade};
pub(crate) use view_names::canonical_view_name;

pub(crate) use builder_injection::{try_inject_builder_scopes, try_inject_mixin_builder_scopes};
pub(crate) use higher_order_proxy::{
    inject_higher_order_proxy_members, is_tagged_higher_order_proxy,
};
pub(crate) use string_keys::{find_laravel_string_key_references, resolve_laravel_string_key};

pub(crate) use expr_walker::walk_all_php_expressions;
pub use helpers::extends_eloquent_model;
pub(crate) use helpers::{accessor_method_candidates, camel_to_snake};
pub use model_provider::LaravelModelProvider;
pub(crate) use model_provider::soft_delete_column;

pub(crate) use accessors::is_accessor_or_mutator_method;
pub(crate) use magic_uses::{MagicMemberKind, declaring_method_names};
pub(crate) use where_property::where_property_method_to_column;

pub(crate) use pivots::{LaravelPivotIndex, build_pivot_index, inject_pivot};
pub(crate) use relationships::class_has_relation_method_ci;
pub(crate) use relationships::classify_relationship_typed;
pub(crate) use relationships::count_property_to_relationship_method;
pub use relationships::infer_relationship_from_body;
pub(crate) use relationships::{
    RELATION_QUERY_METHODS, related_model_type, relation_query_method, resolve_relation_chain,
    resolve_relation_chain_details, resolve_relation_type,
};
pub(crate) use relationships::{
    class_declares_pivot_relationship, extract_pivot_accessor, extract_pivot_using,
    extract_with_pivot_columns, source_may_declare_pivot_relationship,
};

pub use scopes::build_scope_methods_for_builder;

use std::collections::HashMap;

pub(crate) use builder::{custom_builder_fqn, model_builder_type};
pub(crate) use casts::qualify_cast_type;
pub use facade::LaravelFacadeProvider;
pub(crate) use facade::facade_concrete_class;
pub use factory::LaravelFactoryProvider;
pub(crate) use factory::{
    factory_model_type, is_factory_class, is_has_factory_trait, model_to_factory_fqn,
};
pub(crate) use factory_count::{
    carry_factory_count, fluent_factory_count, resolve_factory_count_return,
    resolve_factory_count_return_ast, tag_static_factory_call,
};

use crate::php_type::PhpType;

/// The fully-qualified name of the Eloquent base model.
pub(crate) const ELOQUENT_MODEL_FQN: &str = "Illuminate\\Database\\Eloquent\\Model";

/// The fully-qualified name of Laravel's `SoftDeletes` trait.
const SOFT_DELETES_FQN: &str = "Illuminate\\Database\\Eloquent\\SoftDeletes";

/// Whether a `used_traits` entry names Laravel's `SoftDeletes` trait.
///
/// `used_traits` may hold the FQN or the imported short name, so all three
/// forms match, mirroring the established `class_uses_conditionable`
/// detector.
pub(crate) fn is_soft_deletes_trait(name: &str) -> bool {
    name == SOFT_DELETES_FQN || name == "SoftDeletes" || name.ends_with("\\SoftDeletes")
}

/// The fully-qualified name of the Eloquent Builder class.
pub const ELOQUENT_BUILDER_FQN: &str = "Illuminate\\Database\\Eloquent\\Builder";

/// The fully-qualified name of the Eloquent relation base class, which
/// every relationship the framework ships extends.
pub(crate) const ELOQUENT_RELATION_FQN: &str =
    "Illuminate\\Database\\Eloquent\\Relations\\Relation";

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

/// The fully-qualified name of the response class the `response()` helper
/// builds when it is given content.
pub const RESPONSE_FQN: &str = "Illuminate\\Http\\Response";

/// Whether a call to the `response()` helper builds a response rather than
/// returning the response factory.
///
/// The helper's declared return type is
/// `($content is null ? ResponseFactory : Response)`, but its body branches
/// on `func_num_args()`: any call with an argument returns
/// `$factory->make($content ?? '', …)`, even when that argument is `null`.
/// Following the docblock would type every `response($maybeNull)` as the
/// union of both.
pub(crate) fn response_helper_returns_response(func_name: &str, text_args: &str) -> bool {
    if func_name.trim_start_matches('\\') != "response" {
        return false;
    }
    let args = text_args.trim();
    !args.is_empty() && args != "..."
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

#[cfg(test)]
mod tests;
