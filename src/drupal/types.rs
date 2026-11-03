//! The types Drupal's own declarations give: a service `\Drupal::service()`
//! looks up, the storage `getStorage()` gives for an entity type ID, the
//! entities that storage loads, and the IDs an entity query finds.
//!
//! Shared by the two places the type engine resolves a call: an assignment
//! (`$x = …`) and a chain (`…->load(1)->`).  The storage is tagged with the
//! entity class as a type argument (`NodeStorage<Node>`), the way
//! phpstan-drupal's `EntityStorageType` carries the entity type, so a load
//! on it, chained or through a variable, knows what it loads.  An entity
//! query is tagged the same way (`QueryInterface<Node>`), and a counting one
//! carries a second argument (`QueryInterface<Node, 'count'>`); its fluent
//! methods return `$this`, which the engine already resolves to the tagged
//! receiver, so only `count()` and `execute()` are read here.

use std::sync::Arc;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};

use crate::symbol_map::DRUPAL_ENTITY_TYPE_MANAGER as ENTITY_TYPE_MANAGER;

const ENTITY_STORAGE: &str = "Drupal\\Core\\Entity\\EntityStorageInterface";
const ENTITY_QUERY: &str = "Drupal\\Core\\Entity\\Query\\QueryInterface";
const CONFIG_ENTITY: &str = "Drupal\\Core\\Config\\Entity\\ConfigEntityInterface";
const CONTENT_ENTITY: &str = "Drupal\\Core\\Entity\\ContentEntityInterface";
/// The second type argument of a query tag once `count()` was called on it.
const COUNT_MARKER: &str = "count";

/// The classes `\Drupal::service(id)` returns, when `class` is the global
/// `Drupal` class and the project declares `id`.
pub(crate) fn service_classes(
    backend: &Backend,
    class: &str,
    method_name: &str,
    id: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Vec<Arc<ClassInfo>>> {
    if !method_name.eq_ignore_ascii_case("service")
        || class.strip_prefix('\\').unwrap_or(class) != "Drupal"
        || !backend.is_drupal_project()
    {
        return None;
    }
    let classes: Vec<Arc<ClassInfo>> = backend
        .symfony_service_classes(id)
        .iter()
        .filter_map(|class| class_loader(Fqn::new(class)))
        .collect();
    (!classes.is_empty()).then_some(classes)
}

/// What `getStorage(id)` on `owner`, an entity type manager, returns: each
/// declaration's storage handler class with its tagged type.  `None` for
/// another call or an ID the project does not declare, so the call keeps the
/// declared `EntityStorageInterface`.
pub(crate) fn storage_type(
    backend: &Backend,
    owner: &ClassInfo,
    method_name: &str,
    id: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Vec<(Arc<ClassInfo>, PhpType)>> {
    if !method_name.eq_ignore_ascii_case("getStorage")
        || !backend.is_drupal_project()
        || !crate::class_lookup::is_subtype_of(owner, ENTITY_TYPE_MANAGER, class_loader)
    {
        return None;
    }
    let storages: Vec<(Arc<ClassInfo>, PhpType)> = backend
        .drupal_entity_types(id)
        .into_iter()
        .filter_map(|(entity, storage)| {
            let storage = storage?;
            // A storage class the project lacks would hide the declared type.
            let class = class_loader(Fqn::new(&storage))?;
            Some((
                class,
                PhpType::generic(storage, vec![PhpType::named(entity)]),
            ))
        })
        .collect();
    (!storages.is_empty()).then_some(storages)
}

/// How many entities a storage method returns, if it returns any.
enum Loads {
    /// `load()`, `loadUnchanged()`, `loadRevision()`: one, or `null`.
    MaybeOne,
    /// `create()`: one.
    One,
    /// `loadMultiple()`, `loadByProperties()`: an array keyed by ID.
    Many,
}

fn loads(method_name: &str) -> Option<Loads> {
    let is = |name: &str| method_name.eq_ignore_ascii_case(name);
    if is("load") || is("loadUnchanged") || is("loadRevision") {
        Some(Loads::MaybeOne)
    } else if is("create") {
        Some(Loads::One)
    } else if is("loadMultiple") || is("loadByProperties") {
        Some(Loads::Many)
    } else {
        None
    }
}

/// What a load on a storage tagged by [`storage_type`] returns: the entity
/// class the tag names.  `None` for another call or an untagged receiver.
pub(crate) fn load_type(
    method_name: &str,
    receiver: &[ResolvedType],
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    let loads = loads(method_name)?;
    let mut entities: Vec<Atom> = Vec::new();
    // Every class the receiver can be has to be a tagged storage: a union
    // with anything else keeps its own branches' types.
    for resolved in receiver {
        let Some(class) = resolved.class_info.as_deref() else {
            continue;
        };
        let TypeKind::Generic(generic) = resolved.type_string.kind() else {
            return None;
        };
        let [entity] = generic.args.as_slice() else {
            return None;
        };
        let TypeKind::Named(name) = entity.kind() else {
            return None;
        };
        if !crate::class_lookup::is_subtype_of(class, ENTITY_STORAGE, class_loader) {
            return None;
        }
        if !entities.contains(name) {
            entities.push(*name);
        }
    }
    if entities.is_empty() {
        return None;
    }
    let entity = PhpType::union(entities.into_iter().map(PhpType::named).collect());
    Some(match loads {
        Loads::MaybeOne => PhpType::nullable(entity),
        Loads::One => entity,
        Loads::Many => PhpType::generic_array(
            PhpType::union(vec![PhpType::int(), PhpType::string()]),
            entity,
        ),
    })
}

/// The entity classes a tag's type argument names: one class, or a union of
/// them when the ID has more than one declaration.
fn tagged_entities(argument: &PhpType) -> Option<Vec<Atom>> {
    match argument.kind() {
        TypeKind::Named(name) => Some(vec![*name]),
        TypeKind::Union(members) => members
            .iter()
            .map(|member| match member.kind() {
                TypeKind::Named(name) => Some(*name),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// The query tag for `entities`, counting or not.
fn query_tag(entities: Vec<Atom>, counting: bool) -> PhpType {
    let mut arguments = vec![PhpType::union(
        entities.into_iter().map(PhpType::named).collect(),
    )];
    if counting {
        arguments.push(PhpType::literal_string_value(COUNT_MARKER));
    }
    PhpType::generic(ENTITY_QUERY, arguments)
}

/// What `\Drupal::entityQuery(id)` returns: a query tagged with the entity
/// classes the project declares `id` for.  `None` for another call, an
/// aggregate query, which keeps core's type, or an ID nothing declares.
pub(crate) fn entity_query_type(
    backend: &Backend,
    class: &str,
    method_name: &str,
    id: &str,
) -> Option<PhpType> {
    if !method_name.eq_ignore_ascii_case("entityQuery")
        || class.strip_prefix('\\').unwrap_or(class) != "Drupal"
        || !backend.is_drupal_project()
    {
        return None;
    }
    let mut entities: Vec<Atom> = Vec::new();
    for (entity, _) in backend.drupal_entity_types(id) {
        if !entities.contains(&entity) {
            entities.push(entity);
        }
    }
    (!entities.is_empty()).then(|| query_tag(entities, false))
}

/// What a call on a tagged storage or a tagged query returns, the way
/// phpstan-drupal types entity queries:
///
/// - `getQuery()` on a storage tagged by [`storage_type`]: a query tagged
///   with the same entities;
/// - `count()` on a tagged query: the query, now counting;
/// - `execute()` on a tagged query: the number of matches when it counts,
///   otherwise the IDs, keyed by revision ID (`array<int, string>`) for a
///   content entity and by ID (`array<string, string>`) for a
///   configuration entity.
///
/// `None` for another call or an untagged receiver, and for the IDs of an
/// entity class that is not found, is neither a content nor a configuration
/// entity, or is not of the same kind as the others the query may be on:
/// those keep core's `int|array`.
pub(crate) fn query_call_type(
    method_name: &str,
    receiver: &[ResolvedType],
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    let is = |name: &str| method_name.eq_ignore_ascii_case(name);
    let (base, wants) = if is("getQuery") {
        (ENTITY_STORAGE, Query::Start)
    } else if is("count") {
        (ENTITY_QUERY, Query::Count)
    } else if is("execute") {
        (ENTITY_QUERY, Query::Execute)
    } else {
        return None;
    };
    let mut entities: Vec<Atom> = Vec::new();
    let mut counting = false;
    // Every class the receiver can be has to be tagged: a union with
    // anything else keeps its own branches' types.
    for resolved in receiver {
        let Some(class) = resolved.class_info.as_deref() else {
            continue;
        };
        let TypeKind::Generic(generic) = resolved.type_string.kind() else {
            return None;
        };
        if !crate::class_lookup::is_subtype_of(class, base, class_loader) {
            return None;
        }
        let (entity, rest) = generic.args.split_first()?;
        for name in tagged_entities(entity)? {
            if !entities.contains(&name) {
                entities.push(name);
            }
        }
        counting |= rest.iter().any(|argument| {
            matches!(argument.kind(), TypeKind::Literal(literal)
                if literal.string_content().as_deref() == Some(COUNT_MARKER))
        });
    }
    if entities.is_empty() {
        return None;
    }
    Some(match wants {
        Query::Start => query_tag(entities, false),
        Query::Count => query_tag(entities, true),
        Query::Execute if counting => PhpType::int_range("0", "max"),
        Query::Execute => {
            // Whether each entity class is a configuration entity (`true`)
            // or a content entity (`false`).
            let kind_of = |entity: &Atom| {
                let class = class_loader(Fqn::new(entity))?;
                if crate::class_lookup::is_subtype_of(&class, CONFIG_ENTITY, class_loader) {
                    Some(true)
                } else if crate::class_lookup::is_subtype_of(&class, CONTENT_ENTITY, class_loader) {
                    Some(false)
                } else {
                    None
                }
            };
            let mut kinds = entities.iter().map(kind_of);
            let is_config = kinds.next().flatten()?;
            if !kinds.all(|kind| kind == Some(is_config)) {
                return None;
            }
            let key = if is_config {
                PhpType::string()
            } else {
                PhpType::int()
            };
            PhpType::generic_array(key, PhpType::string())
        }
    })
}

/// Which call on a tagged storage or query [`query_call_type`] answers.
enum Query {
    Start,
    Count,
    Execute,
}
