//! The types Drupal's own declarations give: a service `\Drupal::service()`
//! looks up, the storage `getStorage()` gives for an entity type ID, and
//! the entities that storage loads.
//!
//! Shared by the two places the type engine resolves a call: an assignment
//! (`$x = …`) and a chain (`…->load(1)->`).  The storage is tagged with the
//! entity class as a type argument (`NodeStorage<Node>`), the way
//! phpstan-drupal's `EntityStorageType` carries the entity type, so a load
//! on it, chained or through a variable, knows what it loads.

use std::sync::Arc;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};

use crate::symbol_map::DRUPAL_ENTITY_TYPE_MANAGER as ENTITY_TYPE_MANAGER;

const ENTITY_STORAGE: &str = "Drupal\\Core\\Entity\\EntityStorageInterface";

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
