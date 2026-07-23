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
//!   inferred from the relationship's generic parameters (Larastan-style
//!   `@return HasMany<Post, $this>` annotations) or, as a fallback,
//!   from the first `::class` argument in the method body text.
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
//!   are resolved by loading the class and inspecting the `get()`
//!   method's return type.  When the `get()` method has no return type,
//!   the resolver falls back to the first generic argument from an
//!   `@implements CastsAttributes<TGet, TSet>` annotation on the cast
//!   class.  Enum casts resolve to the enum class itself.  Classes
//!   implementing `Castable` also resolve to themselves.  A `:argument`
//!   suffix (e.g. `Address::class.':nullable'`) is stripped before
//!   resolution.
//!
//! - **Attribute default properties.** Entries in the `$attributes`
//!   property array produce typed virtual properties as a fallback.
//!   Types are inferred from the literal default values: strings,
//!   booleans, integers, floats, `null`, and arrays.  Columns that
//!   already have a `$casts` entry are skipped, so casts always take
//!   priority.
//!
//! - **Column name properties.** Column names from `$fillable`,
//!   `$guarded`, `$hidden`, and `$appends` produce `mixed`-typed
//!   virtual properties as a last-resort fallback.  Columns already
//!   covered by `$casts` or `$attributes` are skipped.
//!
//! - **`where{PropertyName}()` dynamic methods.** Laravel's
//!   `Builder::__call()` translates calls like `whereBrandId($value)`
//!   into `where('brand_id', $value)`.  For each known column on the
//!   model (from all property sources: `$casts`, `$attributes`,
//!   `$fillable`/`$guarded`/`$hidden`/`$appends`, `$dates`, timestamps,
//!   relationship `*_count` properties, `@property` annotations, and
//!   accessor-derived properties), a virtual `where{StudlyCase}()`
//!   method is synthesized.  The method accepts a `mixed` value
//!   parameter and returns `Builder<ConcreteModel>`.  These methods
//!   appear as both instance methods on the Builder (for chaining:
//!   `$query->whereBrandId(42)`) and static methods on the model
//!   (for `User::whereName('Alice')`).

mod accessors;
mod aliases;
mod auth;
mod builder;
mod builder_injection;
mod casts;
mod config_keys;
pub(crate) mod config_values;
pub(crate) mod database_schema;
mod env_vars;
mod factory;
mod helpers;
mod macros;
mod model_extraction;
pub(crate) mod patches;
mod pivots;
mod provider_resources;
mod relationships;
mod route_names;
mod scopes;
mod string_keys;
mod trans_keys;
mod view_names;
pub(crate) mod where_property;

pub(crate) use aliases::{FacadeAccessor, LaravelAliases, parse_facade_accessor};
pub(crate) use auth::{GUARD_FQN, REQUEST_FQN, patch_auth_user_class, resolve_auth_user_type};
pub(crate) use config_keys::find_config_references;
pub(crate) use config_keys::{
    collect_laravel_config_declarations, find_all_config_references,
    laravel_config_prefix_from_uri, resolve_config_key_declaration,
    resolve_config_key_definition_fallback,
};
pub(crate) use env_vars::resolve_env_definition;
pub(crate) use macros::{
    LaravelMacroIndex, MacroRegistration, extract_date_factory_class, extract_macro_registrations,
    extract_mixin_registrations, inject_macros, macro_closure_this_target,
    parse_installed_providers, parse_provider_class_list, parse_provider_referenced_classes,
    synthesize_mixin_macros,
};
pub(crate) use model_extraction::{
    extract_laravel_metadata, has_scope_attribute, infer_relationship_from_method,
};
pub(crate) use provider_resources::{ProviderResources, extract_provider_resources};
pub(crate) use route_names::enumerate_all_route_names;
pub(crate) use trans_keys::collect_trans_declarations;

pub(crate) use builder_injection::{try_inject_builder_scopes, try_inject_mixin_builder_scopes};
pub(crate) use string_keys::{find_laravel_string_key_references, resolve_laravel_string_key};

pub use helpers::extends_eloquent_model;
pub(crate) use helpers::walk_all_php_expressions;
pub(crate) use helpers::{accessor_method_candidates, camel_to_snake};

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
    RelationshipKind, build_property_type, count_property_name, extract_related_type_typed,
};
pub(crate) use relationships::{
    class_declares_pivot_relationship, extract_pivot_using, extract_with_pivot_columns,
};

pub use scopes::build_scope_methods_for_builder;
use scopes::{build_scope_methods, is_scope_method};
use where_property::{build_where_property_methods_for_class, lowercase_method_names};

use std::collections::HashMap;
use std::sync::Arc;

use builder::build_builder_forwarded_methods;
use casts::cast_type_to_php_type;
pub use factory::LaravelFactoryProvider;
pub(crate) use factory::{factory_to_model_fqn, model_to_factory_fqn};

use crate::php_type::PhpType;
use crate::types::{
    AttributeDefaultSource, ClassInfo, DatabaseColumnSource, PropertyInfo, PropertySource,
};

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
// Called from `completion/resolver.rs` (`type_hint_to_classes_depth`) to
// apply Eloquent-specific post-processing after a class has been resolved
// and generic substitution applied.  Keeping the framework logic here
// rather than inline in the generic resolver avoids coupling the type
// engine to Laravel conventions.

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

/// Virtual member provider for Laravel Eloquent models.
///
/// When a class extends `Illuminate\Database\Eloquent\Model` (directly
/// or through an intermediate parent), this provider scans its methods
/// for Eloquent relationship return types and synthesizes virtual
/// properties for each one.
///
/// For example, a method `posts()` returning `HasMany<Post, $this>`
/// produces a virtual property `$posts` with type
/// `\Illuminate\Database\Eloquent\Collection<Post>`.
pub struct LaravelModelProvider;

/// Laravel date type used for date-related virtual properties.
fn carbon_type() -> PhpType {
    PhpType::Named(CONFIGURED_DATE_CLASS_FQN.to_owned())
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
            // ── Scope methods ───────────────────────────────────────
            if is_scope_method(method) {
                // Skip `#[Scope]`-attributed methods that also use
                // the `scopeX` prefix — the attribute takes priority
                // and the name is used as-is (no prefix stripping).
                let [instance_method, static_method] = build_scope_methods(method);
                methods.push(instance_method);
                methods.push(static_method);
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
                related_type
                    .and_then(|t| t.base_name().and_then(class_loader))
                    .and_then(|related_class| {
                        related_class
                            .laravel
                            .as_ref()
                            .and_then(|l| l.custom_collection.as_ref().map(|c| c.to_string()))
                    })
            } else {
                None
            };

            let type_hint = build_property_type(kind, related_type, custom_collection.as_deref());

            if let Some(ref th) = type_hint {
                // Attach any pivot configuration recovered from the
                // relationship body (`->using(...)` / `->withPivot(...)`) so
                // hover can surface the custom pivot class and extra columns.
                let (pivot_using, pivot_columns) = class
                    .laravel()
                    .and_then(|l| {
                        l.belongs_to_many_pivots
                            .iter()
                            .find(|p| p.method == method.name.as_str())
                    })
                    .map(|p| (p.using.clone(), p.columns.clone()))
                    .unwrap_or_default();
                properties.push(PropertyInfo {
                    source: Some(PropertySource::Relationship {
                        method: method.name.to_string(),
                        kind: relationship_kind_name(kind).to_string(),
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
            methods.push(m);
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
