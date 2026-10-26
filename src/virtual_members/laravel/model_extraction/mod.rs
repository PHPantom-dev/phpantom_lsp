//! Parse-time extraction of Eloquent model metadata from the AST.
//!
//! This module builds the [`LaravelMetadata`] attached to every parsed
//! class: factory `$model`, `$casts`/`casts()`, `$attributes`, `$dates`,
//! `$fillable`/`$guarded`/`$hidden`/`$visible`/`$appends`, `$timestamps` and the
//! `CREATED_AT`/`UPDATED_AT` constants, the `#[Connection]`/`#[Table]`
//! attributes (and their property fallbacks), custom builder/collection
//! overrides, and `belongsToMany`/`morphToMany` pivot configuration.
//!
//! This runs once per class during parsing (`parser::classes`), unlike the
//! rest of `virtual_members::laravel`, which resolves already-parsed
//! [`ClassInfo`]/[`LaravelMetadata`] into virtual members at completion
//! time.

use mago_syntax::cst::*;

use crate::atom::{Atom, atom, last_segment};
use crate::parser::DocblockCtx;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{CastSources, LaravelMetadata, MethodInfo, model_declaration};
use crate::util::short_name;

use super::relationships::is_inferable_relationship_short_name;
use super::{infer_relationship_from_body, is_soft_deletes_trait};

mod attributes;
use attributes::*;
mod properties;
pub(crate) use properties::*;
mod morph;
pub(crate) use morph::*;
#[cfg(test)]
mod tests;

/// Check whether a method has the `#[Scope]` attribute (Laravel 11+).
///
/// Scans the method's attribute lists for an attribute whose short name
/// is `Scope` (matching `#[Scope]`, `#[\Illuminate\Database\Eloquent\Attributes\Scope]`,
/// or any use-imported alias that ends with `Scope`).
pub(crate) fn has_scope_attribute(method: &class_like::method::Method<'_>) -> bool {
    for attr_list in method.attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            if last_segment(attr.name.value()) == b"Scope" {
                return true;
            }
        }
    }
    false
}

/// Try to infer an Eloquent relationship return type from a method's body.
///
/// When a method has no `@return` annotation and no native return type
/// hint, this function extracts the method body text and scans it for
/// patterns like `$this->hasMany(Post::class)`.  If found, it returns
/// a synthesized return type string (e.g. `HasMany<Post>`).
///
/// This enables relationship property synthesis on models whose
/// relationship methods carry no generic `@return` annotation.
fn infer_relationship_from_method<'a>(
    method: &class_like::method::Method<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> Option<PhpType> {
    let ctx = doc_ctx?;
    let class_like::method::MethodBody::Concrete(block) = &method.body else {
        return None;
    };
    let start = block.left_brace.start.offset as usize;
    let end = block.right_brace.end.offset as usize;
    if end > ctx.content.len() || start >= end {
        return None;
    }
    // Adjust to valid UTF-8 char boundaries.
    let start = ctx.content.floor_char_boundary(start);
    let end = ctx.content.floor_char_boundary(end);
    let body_text = &ctx.content[start..end];
    infer_relationship_from_body(body_text)
}

/// A method's return type, filled in from its body when that says more.
///
/// With no declared type, the body's `$this->hasMany(Post::class)` is the
/// only source.  A bare relationship class as the declared type
/// (`: HasMany`, the usual way to write a relationship without a
/// docblock) names the relationship but not the related model, which the
/// same body call supplies.
pub(crate) fn relationship_return_type<'a>(
    declared: Option<PhpType>,
    method: &class_like::method::Method<'a>,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> Option<PhpType> {
    let Some(declared) = declared else {
        return infer_relationship_from_method(method, doc_ctx);
    };
    let TypeKind::Named(name) = declared.kind() else {
        return Some(declared);
    };
    let declared_short = short_name(name);
    if !is_inferable_relationship_short_name(declared_short) {
        return Some(declared);
    }
    match infer_relationship_from_method(method, doc_ctx) {
        Some(inferred) if matches!(inferred.kind(), TypeKind::Generic(g) if short_name(&g.name) == declared_short) => {
            Some(inferred)
        }
        _ => Some(declared),
    }
}

/// Extract trait metadata needed to resolve a model's morph columns and table.
pub(crate) fn extract_laravel_trait_metadata<'a>(
    trait_def: &class_like::Trait<'a>,
    methods: &[MethodInfo],
    content: &str,
) -> Option<Box<LaravelMetadata>> {
    let morph_type_columns = extract_morph_type_columns(trait_def.members.iter(), content);
    let table_name = extract_string_property(trait_def.members.iter(), content, "table");
    let has_get_table_method = methods.iter().any(|method| {
        !method.is_abstract && !method.is_virtual && method.name.eq_ignore_ascii_case("getTable")
    });
    (!morph_type_columns.is_empty() || table_name.is_some() || has_get_table_method).then(|| {
        Box::new(LaravelMetadata {
            morph_type_columns,
            table_name,
            has_get_table_method,
            ..Default::default()
        })
    })
}

/// Build the [`LaravelMetadata`] for a class from its AST node.
///
/// `methods` must already be extracted (via
/// `Backend::extract_class_like_members`) since several sources here
/// (custom builder/collection, relationship-derived timestamps) key off
/// already-resolved method return types. `use_generics` is the merged
/// docblock + inline `@use` generics list, needed for
/// `HasBuilder`/`HasCollection` detection, and `used_traits` the traits
/// the class body uses, needed for `SoftDeletes` detection.
pub(crate) fn extract_laravel_metadata<'a>(
    class: &class_like::Class<'a>,
    methods: &[MethodInfo],
    used_traits: &[Atom],
    use_generics: &[(Atom, Vec<PhpType>)],
    content: &str,
    doc_ctx: Option<&DocblockCtx<'a>>,
) -> LaravelMetadata {
    let factory_model = extract_factory_model(class.members.iter());

    let custom_collection = extract_class_return_from_body(class.members.iter(), "newCollection")
        .filter(|ty| {
            ty.base_name().is_some_and(|name| {
                name != "Collection" && name != "Illuminate\\Database\\Eloquent\\Collection"
            })
        })
        .or_else(|| {
            extract_custom_collection(&class.attribute_lists, use_generics, methods, content)
        });

    let custom_builder = extract_class_return_from_body(class.members.iter(), "newEloquentBuilder")
        .filter(|ty| {
            ty.base_name().is_some_and(|name| {
                name != "Builder" && name != "Illuminate\\Database\\Eloquent\\Builder"
            })
        })
        .or_else(|| extract_custom_builder(&class.attribute_lists, use_generics, methods, content));

    let custom_factory = methods
        .iter()
        .find(|m| m.name == "newFactory")
        .and_then(|m| m.return_type.as_ref())
        .filter(|ty| {
            ty.base_name().is_some_and(|name| {
                name != "Factory" && name != "Illuminate\\Database\\Eloquent\\Factories\\Factory"
            })
        })
        .cloned()
        .or_else(|| extract_class_return_from_body(class.members.iter(), "newFactory"))
        .or_else(|| extract_class_property(class.members.iter(), "factory"))
        .or_else(|| {
            extract_class_constant_attribute(&class.attribute_lists, content, b"UseFactory")
                .map(|name| PhpType::named(atom(&name)))
        });

    let policy_class = extract_use_policy_attribute(&class.attribute_lists, content);

    let (property_casts, method_casts) = extract_cast_sources(class.members.iter(), content);
    let mut declared = 0;
    if property_casts.is_some() {
        declared |= model_declaration::CASTS_PROPERTY;
    }
    if method_casts.is_some() {
        declared |= model_declaration::CASTS_METHOD;
    }
    let (casts_definitions, cast_sources) = match (property_casts, method_casts) {
        (Some(property), Some(method)) => {
            let merged = overlay_casts(&property, &method);
            (merged, Some(Box::new(CastSources { property, method })))
        }
        (Some(only), None) | (None, Some(only)) => (only, None),
        (None, None) => (Vec::new(), None),
    };

    let belongs_to_many_pivots = extract_pivot_relations(class.members.iter(), content);
    let morph_type_columns = extract_morph_type_columns(class.members.iter(), content);

    let (attributes_definitions, attribute_defaults): (Vec<_>, Vec<_>) =
        extract_attributes(class.members.iter(), content)
            .into_iter()
            .map(|(key, php_type, value)| ((key.clone(), php_type), (key, value)))
            .unzip();
    if declares_property(class.members.iter(), "attributes") {
        declared |= model_declaration::ATTRIBUTES;
    }
    if declares_property(class.members.iter(), "dates") {
        declared |= model_declaration::DATES;
    }

    let (column_names, column_sources, column_lists) =
        extract_column_names(class.members.iter(), content);
    declared |= column_lists;

    let connection_name =
        extract_laravel_connection_attribute(&class.attribute_lists, content, doc_ctx)
            .or_else(|| extract_string_property(class.members.iter(), content, "connection"));

    let table_name = extract_laravel_table_attribute(&class.attribute_lists, content, doc_ctx)
        .or_else(|| extract_string_property(class.members.iter(), content, "table"));

    let has_get_connection_name_method = methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("getConnectionName"));
    let has_get_table_method = methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("getTable"));

    let primary_key = extract_string_property(class.members.iter(), content, "primaryKey");
    let key_type = extract_string_property(class.members.iter(), content, "keyType");
    let has_get_key_name_method = methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("getKeyName"));
    let unique_ids = methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("uniqueIds"))
        .then(|| extract_unique_ids(class.members.iter(), content))
        .flatten();

    let dates_definitions = extract_dates_definitions(class.members.iter(), content);

    let TimestampConfig {
        timestamps,
        created_at_name,
        updated_at_name,
        deleted_at_name,
    } = extract_timestamp_config(class.members.iter(), content);
    let soft_deletes = used_traits.iter().any(|t| is_soft_deletes_trait(t));

    // Gate the member walk on the already-extracted method list: all but
    // the handful of facades in a project declare no `getFacadeAccessor()`,
    // and the slice is still warm from the checks above.
    let facade_accessor = methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case("getFacadeAccessor"))
        .then(|| extract_facade_accessor(class.members.iter(), content))
        .flatten();

    LaravelMetadata {
        custom_factory,
        factory_model,
        custom_collection,
        casts_definitions,
        cast_sources,
        declared,
        dates_definitions,
        attributes_definitions,
        attribute_defaults,
        column_names,
        column_sources,
        connection_name,
        table_name,
        has_get_connection_name_method,
        has_get_table_method,
        primary_key,
        key_type,
        has_get_key_name_method,
        unique_ids,
        timestamps,
        created_at_name,
        updated_at_name,
        soft_deletes,
        deleted_at_name,
        custom_builder,
        policy_class,
        belongs_to_many_pivots,
        morph_type_columns,
        facade_accessor,
    }
}

/// The [`model_declaration`] flags of `meta`, counting a non-empty list
/// with no flag as declared.
///
/// Metadata built by hand rather than parsed fills in the lists without
/// the flags; the lists alone are then the only record of a declaration.
fn declared_flags(meta: &LaravelMetadata) -> u16 {
    use model_declaration::*;
    let mut flags = meta.declared;
    if flags & (CASTS_PROPERTY | CASTS_METHOD) == 0 && !meta.casts_definitions.is_empty() {
        flags |= CASTS_PROPERTY;
    }
    if !meta.dates_definitions.is_empty() {
        flags |= DATES;
    }
    if !meta.attributes_definitions.is_empty() || !meta.attribute_defaults.is_empty() {
        flags |= ATTRIBUTES;
    }
    if flags & COLUMN_LISTS == 0 && !meta.column_names.is_empty() {
        flags |= FILLABLE;
    }
    flags
}

/// The `$casts` property and `casts()` method entries of `meta`, `None`
/// for a source it does not declare.
fn cast_parts(meta: &LaravelMetadata, flags: u16) -> (Option<&[CastEntry]>, Option<&[CastEntry]>) {
    let property = flags & model_declaration::CASTS_PROPERTY != 0;
    let method = flags & model_declaration::CASTS_METHOD != 0;
    match (property, method, meta.cast_sources.as_deref()) {
        (true, true, Some(sources)) => (Some(&sources.property), Some(&sources.method)),
        (true, true, None) => (Some(&meta.casts_definitions), Some(&[])),
        (true, false, _) => (Some(&meta.casts_definitions), None),
        (false, true, _) => (None, Some(&meta.casts_definitions)),
        (false, false, _) => (None, None),
    }
}

/// Whether `meta` declares anything a subclass could inherit through
/// [`inherit_model_metadata`].
pub(crate) fn has_inheritable_model_metadata(meta: &LaravelMetadata) -> bool {
    declared_flags(meta) != 0
        || meta.primary_key.is_some()
        || meta.key_type.is_some()
        || meta.timestamps.is_some()
        || meta.created_at_name.is_some()
        || meta.updated_at_name.is_some()
        || meta.soft_deletes
        || meta.deleted_at_name.is_some()
        || meta.connection_name.is_some()
        || meta.table_name.is_some()
        || meta.has_get_connection_name_method
        || meta.has_get_table_method
        || meta.has_get_key_name_method
}

/// Fill in the model configuration `child` does not declare from its
/// parent's.
///
/// PHP hands a subclass every property and constant it does not
/// redeclare, so `$fillable`, `$casts`, `$primaryKey`, `CREATED_AT` and
/// the rest each come from the nearest class in the parent chain that
/// declares them. Called once per parent while walking up the chain
/// nearest-first, so a declaration a closer class already supplied is
/// never overwritten. Each column list and each of `$casts` and `casts()`
/// is inherited on its own: a subclass that redeclares `$hidden` still
/// sees the parent's `$fillable`.
pub(crate) fn inherit_model_metadata(child: &mut LaravelMetadata, parent: &LaravelMetadata) {
    use model_declaration::*;
    let child_flags = declared_flags(child);
    let parent_flags = declared_flags(parent);
    let inherited = parent_flags & !child_flags;

    if inherited & (CASTS_PROPERTY | CASTS_METHOD) != 0 {
        let (own_property, own_method) = cast_parts(child, child_flags);
        let (parent_property, parent_method) = cast_parts(parent, parent_flags);
        let property = own_property.or(parent_property).map(<[_]>::to_vec);
        let method = own_method.or(parent_method).map(<[_]>::to_vec);
        match (property, method) {
            (Some(property), Some(method)) => {
                child.casts_definitions = overlay_casts(&property, &method);
                child.cast_sources = Some(Box::new(CastSources { property, method }));
            }
            (Some(only), None) | (None, Some(only)) => child.casts_definitions = only,
            (None, None) => {}
        }
    }

    if inherited & DATES != 0 {
        child
            .dates_definitions
            .clone_from(&parent.dates_definitions);
    }
    if inherited & ATTRIBUTES != 0 {
        child
            .attributes_definitions
            .clone_from(&parent.attributes_definitions);
        child
            .attribute_defaults
            .clone_from(&parent.attribute_defaults);
    }

    let inherited_lists = inherited & COLUMN_LISTS;
    if inherited_lists != 0 {
        child
            .column_sources
            .resize(child.column_names.len(), child_flags & COLUMN_LISTS);
        for (i, name) in parent.column_names.iter().enumerate() {
            let lists = parent
                .column_sources
                .get(i)
                .copied()
                .unwrap_or(parent_flags & COLUMN_LISTS)
                & inherited_lists;
            if lists == 0 {
                continue;
            }
            if let Some(pos) = child.column_names.iter().position(|n| n == name) {
                child.column_sources[pos] |= lists;
            } else {
                child.column_names.push(name.clone());
                child.column_sources.push(lists);
            }
        }
    }

    child.declared = child_flags | inherited;

    fn inherit<T: std::clone::Clone>(own: &mut Option<T>, parent: &Option<T>) {
        if own.is_none() {
            own.clone_from(parent);
        }
    }
    inherit(&mut child.primary_key, &parent.primary_key);
    inherit(&mut child.key_type, &parent.key_type);
    inherit(&mut child.timestamps, &parent.timestamps);
    inherit(&mut child.created_at_name, &parent.created_at_name);
    inherit(&mut child.updated_at_name, &parent.updated_at_name);
    inherit(&mut child.deleted_at_name, &parent.deleted_at_name);
    child.soft_deletes |= parent.soft_deletes;
    inherit(&mut child.connection_name, &parent.connection_name);
    inherit(&mut child.table_name, &parent.table_name);
    child.has_get_connection_name_method |= parent.has_get_connection_name_method;
    child.has_get_table_method |= parent.has_get_table_method;
    child.has_get_key_name_method |= parent.has_get_key_name_method;
}
