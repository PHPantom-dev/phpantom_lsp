//! The virtual members of an Eloquent model: properties for its columns,
//! accessors and relationships, and the scope, builder and
//! `where{Property}()` methods it answers.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::atom::{AtomSet, ascii_lowercase_atom, atom};
use crate::php_type::PhpType;
use crate::types::{
    AttributeDefaultSource, ClassInfo, DatabaseColumnSource, LaravelMetadata, MethodInfo,
    PivotAccessor, PropertyInfo, PropertySource,
};

use super::super::resolve::resolve_class_base_cached;
use super::super::{ResolvedClassCache, VirtualMemberProvider, VirtualMembers};
use super::accessors::{
    extract_modern_accessor_type, is_legacy_accessor, is_legacy_mutator, is_modern_accessor,
    legacy_accessor_property_name, legacy_mutator_property_name,
};
use super::builder::build_builder_forwarded_methods;
use super::casts::cast_type_to_php_type;
use super::collections::custom_collection_for_model;
use super::database_schema::SchemaTable;
use super::relationships::{
    RelationshipKind, build_property_type, classify_relationship_typed, count_property_name,
    extract_pivot_accessor_typed, extract_related_type_typed,
};
use super::scopes::{build_scope_methods, is_scope_method};
use super::table_names::model_schema_table;
use super::where_property::{build_where_property_methods_for_class, lowercase_method_names};
use super::{
    CONFIGURED_DATE_CLASS_FQN, ELOQUENT_MODEL_FQN, camel_to_snake, extends_eloquent_model,
    unique_ids,
};

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

/// The column `SoftDeletes` casts to a date, when the model uses the trait:
/// `DELETED_AT`, defaulting to `deleted_at`.
pub(crate) fn soft_delete_column(laravel: &crate::types::LaravelMetadata) -> Option<&str> {
    laravel
        .soft_deletes
        .then(|| laravel.deleted_at_name.as_deref().unwrap_or("deleted_at"))
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

/// The virtual property accessor `method` synthesizes as `prop_name`,
/// backed by the schema column of that name when the table has one.
fn accessor_property(
    method: &crate::types::MethodInfo,
    prop_name: &str,
    property_type: Option<&PhpType>,
    schema_table: Option<&SchemaTable>,
    mutator_methods: &HashMap<String, String>,
) -> PropertyInfo {
    let method_name = method.name.to_string();
    let mutator = mutator_methods.get(prop_name).cloned();
    let column = schema_column_source(schema_table, prop_name);
    let source = if column.is_some() {
        PropertySource::Accessor {
            method: method_name,
            mutator,
            column,
        }
    } else {
        PropertySource::ComputedProperty {
            method: method_name,
            mutator,
        }
    };
    PropertyInfo {
        deprecation_message: method.deprecation_message().map(str::to_owned),
        description: method.description.clone(),
        source: Some(source),
        ..PropertyInfo::virtual_property_typed(prop_name, property_type)
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
        let mut members = ModelMembers::new(class, class_loader, cache);
        if let Some(laravel) = class.laravel() {
            members.add_declared_column_properties(laravel);
            let timestamps = timestamp_columns(laravel);
            let soft_delete = soft_delete_column(laravel);
            members.add_schema_column_properties(&timestamps, soft_delete);
            members.add_implicit_column_properties(laravel, timestamps, soft_delete);
        }
        members.add_method_members();
        members.add_relationship_count_properties();
        members.add_builder_methods(cache);
        VirtualMembers {
            methods: members.methods,
            properties: members.properties,
            constants: Vec::new(),
        }
    }
}

/// The virtual members of one model, added kind by kind.  A column read
/// from a higher-priority source (`$casts` over `$dates` over
/// `$attributes` over the schema) keeps its property: `seen_props` holds
/// the names already added.
struct ModelMembers<'a> {
    class: &'a ClassInfo,
    class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    schema_table: Option<SchemaTable>,
    /// Method names declared by the base Eloquent Model, lowercased.
    base_model_methods: AtomSet,
    /// The legacy mutator (`setXAttribute`) of each property that has one.
    mutator_methods: HashMap<String, String>,
    seen_props: HashSet<String>,
    properties: Vec<PropertyInfo>,
    methods: Vec<Arc<MethodInfo>>,
}

impl<'a> ModelMembers<'a> {
    fn new(
        class: &'a ClassInfo,
        class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cache: Option<&ResolvedClassCache>,
    ) -> Self {
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
        Self {
            class,
            class_loader,
            schema_table: model_schema_table(class, cache),
            base_model_methods,
            mutator_methods: mutator_methods_by_property(class),
            seen_props: HashSet::new(),
            properties: Vec::new(),
            methods: Vec::new(),
        }
    }

    /// Whether `method` is a framework internal inherited from the base
    /// Model, which never describes a member of the user's model.
    fn is_base_model_method(&self, method: &MethodInfo) -> bool {
        self.base_model_methods
            .contains(&ascii_lowercase_atom(&method.name))
    }

    /// The columns the model declares itself: `$casts`, then `$dates`,
    /// then `$attributes` defaults.
    fn add_declared_column_properties(&mut self, laravel: &LaravelMetadata) {
        let class = self.class;
        let schema_table = self.schema_table.as_ref();

        // ── Cast properties ─────────────────────────────────────────
        for (column, cast_type) in &laravel.casts_definitions {
            let php_type = cast_type_to_php_type(cast_type, self.class_loader);
            self.seen_props.insert(column.clone());
            self.properties.push(PropertyInfo {
                source: Some(PropertySource::Cast {
                    cast: cast_type.clone(),
                    column: schema_column_source(schema_table, column),
                    attribute_default: attribute_default_source(class, column),
                    mutator: self.mutator_methods.get(column).cloned(),
                }),
                ..PropertyInfo::virtual_property_typed(column, Some(&php_type))
            });
        }

        // ── $dates properties (deprecated, lower priority than $casts) ──
        // Columns in `$dates` are typed as Carbon\Carbon unless already
        // covered by an explicit `$casts` entry.
        for column in &laravel.dates_definitions {
            if !self.seen_props.insert(column.clone()) {
                continue;
            }
            self.properties.push(PropertyInfo {
                source: Some(PropertySource::Cast {
                    cast: "date".to_string(),
                    column: schema_column_source(schema_table, column),
                    attribute_default: attribute_default_source(class, column),
                    mutator: self.mutator_methods.get(column).cloned(),
                }),
                ..PropertyInfo::virtual_property_typed(column, Some(&carbon_type()))
            });
        }

        // ── Attribute default properties (fallback) ─────────────
        // Only add properties for columns not already covered by $casts
        // or $dates.
        for (column, php_type) in &laravel.attributes_definitions {
            if !self.seen_props.insert(column.clone()) {
                continue;
            }
            self.properties.push(PropertyInfo {
                source: attribute_default_source(class, column).map(|default| {
                    PropertySource::AttributeDefault {
                        default,
                        column: schema_column_source(schema_table, column),
                        mutator: self.mutator_methods.get(column).cloned(),
                    }
                }),
                ..PropertyInfo::virtual_property_typed(column, Some(php_type))
            });
        }
    }

    /// The columns of the model's table in the schema, the timestamp and
    /// soft-delete columns typed as the configured date class.
    fn add_schema_column_properties(&mut self, timestamps: &[String], soft_delete: Option<&str>) {
        let Some(schema_table) = &self.schema_table else {
            return;
        };
        for column in &schema_table.columns {
            if !self.seen_props.insert(column.name.clone()) {
                continue;
            }
            let php_type = if timestamps.contains(&column.name) {
                carbon_type()
            } else if soft_delete == Some(column.name.as_str()) {
                PhpType::nullable(carbon_type())
            } else {
                column.php_type.clone()
            };
            self.properties.push(PropertyInfo {
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
                    attribute_default: attribute_default_source(self.class, &column.name),
                    mutator: self.mutator_methods.get(&column.name).cloned(),
                }),
                ..PropertyInfo::virtual_property_typed(&column.name, Some(&php_type))
            });
        }
    }

    /// The columns every model has whether or not anything describes them:
    /// the primary key, the timestamps and the soft-delete column, and then
    /// the untyped names `$fillable`, `$guarded`, `$hidden` and `$appends`
    /// list.
    fn add_implicit_column_properties(
        &mut self,
        laravel: &LaravelMetadata,
        timestamps: Vec<String>,
        soft_delete: Option<&str>,
    ) {
        // ── Implicit primary key ────────────────────────────────
        // Every Eloquent model exposes a primary key column (default `id`)
        // even when no migration or schema dump describes the table.
        // Synthesize it when schema/casts/attributes have not already
        // provided it, respecting `$primaryKey` and `$keyType` overrides.
        // Skip when `getKeyName()` is declared, since the key name is then
        // computed at runtime and cannot be resolved here.
        if !laravel.has_get_key_name_method {
            let primary_key = laravel.primary_key.as_deref().unwrap_or("id");
            if self.seen_props.insert(primary_key.to_string()) {
                let php_type = if laravel.key_type.as_deref() == Some("string")
                    || (unique_ids::uses_unique_string_ids(self.class, self.class_loader)
                        && unique_ids::unique_ids_include(
                            self.class,
                            primary_key,
                            self.class_loader,
                        )) {
                    PhpType::string()
                } else {
                    PhpType::int()
                };
                self.properties.push(PropertyInfo::virtual_property_typed(
                    primary_key,
                    Some(&php_type),
                ));
            }
        }

        // ── Timestamp properties ────────────────────────────────
        // Add timestamp properties only when schema/casts/attributes did not
        // already provide the column. Schema-backed timestamp columns keep
        // their database source and use the configured Laravel date class.
        for column in timestamps {
            if self.seen_props.insert(column.clone()) {
                self.properties.push(PropertyInfo::virtual_property_typed(
                    &column,
                    Some(&carbon_type()),
                ));
            }
        }

        // ── Soft-delete column ──────────────────────────────────
        // `SoftDeletes` casts its column to a date, which is null until the
        // model is trashed.
        if let Some(column) = soft_delete
            && self.seen_props.insert(column.to_string())
        {
            self.properties.push(PropertyInfo::virtual_property_typed(
                column,
                Some(&PhpType::nullable(carbon_type())),
            ));
        }

        // ── Column name properties (last-resort fallback) ───────
        // $fillable, $guarded, $hidden, and $appends provide column names
        // without type info.  Only add those not already covered.
        for column in &laravel.column_names {
            if !self.seen_props.insert(column.clone()) {
                continue;
            }
            self.properties.push(PropertyInfo::virtual_property_typed(
                column,
                Some(&PhpType::mixed()),
            ));
        }
    }

    /// The members the model's own methods imply: the query methods of its
    /// scopes, the properties of its accessors, and the property each
    /// relationship method loads.
    fn add_method_members(&mut self) {
        let class = self.class;
        for method in &class.methods {
            if self.is_base_model_method(method) {
                continue;
            }

            // ── Scope methods ───────────────────────────────────────
            if is_scope_method(method) {
                // A method that is both `#[Scope]`-attributed and named
                // `scopeX` keeps the attribute's name as-is; the prefix is
                // only stripped for convention-based scope methods.
                let [instance_method, static_method] = build_scope_methods(method);
                self.methods.push(Arc::new(instance_method));
                self.methods.push(Arc::new(static_method));
                continue;
            }

            // ── Legacy accessors (getXAttribute) ────────────────────
            if is_legacy_accessor(method) {
                let prop_name = legacy_accessor_property_name(&method.name);
                let property = accessor_property(
                    method,
                    &prop_name,
                    method.return_type.as_ref(),
                    self.schema_table.as_ref(),
                    &self.mutator_methods,
                );
                push_or_replace_property(&mut self.properties, property);
                continue;
            }

            // ── Modern accessors (Laravel 9+ Attribute casts) ───────
            if is_modern_accessor(method) {
                let prop_name = camel_to_snake(&method.name);
                let accessor_type = extract_modern_accessor_type(method);
                let property = accessor_property(
                    method,
                    &prop_name,
                    Some(&accessor_type),
                    self.schema_table.as_ref(),
                    &self.mutator_methods,
                );
                push_or_replace_property(&mut self.properties, property);
                continue;
            }

            if let Some(property) = self.relationship_property(method) {
                self.properties.push(property);
            }
        }
    }

    /// The property a relationship method loads, `None` when `method` is
    /// not one.
    fn relationship_property(&self, method: &MethodInfo) -> Option<PropertyInfo> {
        let return_type = method.return_type.as_ref()?;
        let kind = classify_relationship_typed(return_type)?;
        let related_type = extract_related_type_typed(return_type);

        // For collection relationships, use the *related* model's
        // custom_collection, not the owning model's.  For example, if
        // Product has `#[CollectedBy(ProductCollection)]` and Review has
        // `#[CollectedBy(ReviewCollection)]`, then `Product::reviews()`
        // returning `HasMany<Review, $this>` should produce
        // `ReviewCollection<Review>`, not `ProductCollection<Review>`.
        let custom_collection = if kind == RelationshipKind::Collection {
            related_type.and_then(|t| {
                // A self-referential relation (`HasMany<self, $this>`) names
                // the owning model, so the keyword has to be resolved before
                // the model can be looked up.
                let model = if t.is_self_ref() {
                    self.class.fqn().to_string()
                } else {
                    t.base_name()?.to_string()
                };
                custom_collection_for_model(&model, self.class_loader)
            })
        } else {
            None
        };

        let type_hint = build_property_type(kind, related_type, custom_collection.as_deref())?;

        // Attach any pivot configuration recovered from the relationship
        // body (`->as(...)` / `->using(...)` / `->withPivot(...)`) so hover
        // can surface the custom accessor, pivot class, and extra columns.
        let generic_pivot_accessor = extract_pivot_accessor_typed(return_type);
        let (body_pivot_accessor, pivot_using, pivot_columns) = self
            .class
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
        Some(PropertyInfo {
            source: Some(PropertySource::Relationship {
                method: method.name.to_string(),
                kind: relationship_kind_name(kind).to_string(),
                pivot_accessor,
                pivot_using,
                pivot_columns,
            }),
            description: method.description.clone(),
            ..PropertyInfo::virtual_property_typed(&method.name, Some(&type_hint))
        })
    }

    /// Relationship count properties (`*_count`).
    ///
    /// `withCount`/`loadCount` is one of the most common Eloquent patterns.
    /// For each relationship method, synthesize a `{snake_name}_count`
    /// property typed as `int`.  Skip if a property with that name already
    /// exists (e.g. from an explicit `@property` tag).
    fn add_relationship_count_properties(&mut self) {
        let class = self.class;
        for method in &class.methods {
            if self.is_base_model_method(method) {
                continue;
            }
            let Some(return_type) = method.return_type.as_ref() else {
                continue;
            };
            if classify_relationship_typed(return_type).is_none() {
                continue;
            }
            let count_name = count_property_name(&method.name);
            if !self.seen_props.insert(count_name.clone()) {
                continue;
            }
            self.properties.push(PropertyInfo {
                source: Some(PropertySource::RelationshipCount {
                    relationship: method.name.to_string(),
                }),
                ..PropertyInfo::virtual_property_typed(&count_name, Some(&PhpType::int()))
            });
        }
    }

    /// The builder's methods, which `Model::__callStatic()` forwards, and
    /// the `where{Property}()` methods the builder answers.
    fn add_builder_methods(&mut self, cache: Option<&ResolvedClassCache>) {
        // ── Builder-as-static forwarding ────────────────────────────
        let forwarded = build_builder_forwarded_methods(self.class, self.class_loader, cache);
        self.methods.extend(forwarded);

        // ── where{PropertyName}() static forwarding ─────────────────
        // Laravel's Model::__callStatic() delegates to Builder, which
        // handles where{Column}() calls.  Synthesize these as static
        // methods on the model so that User::whereName('Alice') resolves.
        let existing = lowercase_method_names(&self.methods);
        let where_static = build_where_property_methods_for_class(self.class, &existing);
        for mut m in where_static {
            m.is_static = true;
            self.methods.push(Arc::new(m));
        }
    }
}
