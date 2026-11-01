//! Drupal calls whose return type an entity type decides: the storage
//! `getStorage()` gives for an entity type ID, and the entities that
//! storage loads.
//!
//! The storage is tagged with the entity class as a type argument
//! (`NodeStorage<Node>`), the way phpstan-drupal's `EntityStorageType`
//! carries the entity type, so a load on it, chained or through a
//! variable, knows what it loads.

use std::sync::Arc;

use mago_syntax::cst::*;

use crate::atom::Atom;
use crate::class_lookup::Fqn;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};

use crate::type_engine::resolver::VarResolutionCtx;

const ENTITY_TYPE_MANAGER: &str = "Drupal\\Core\\Entity\\EntityTypeManagerInterface";
const ENTITY_STORAGE: &str = "Drupal\\Core\\Entity\\EntityStorageInterface";

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

/// What `getStorage('node')` on an entity type manager returns: each
/// declaration's storage handler, tagged with its entity class.
///
/// `None` for an ID the project does not declare, so the call keeps the
/// declared `EntityStorageInterface`.
pub(super) fn try_resolve_entity_storage_type(
    owner: &ClassInfo,
    method_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    if !method_name.eq_ignore_ascii_case("getStorage") {
        return None;
    }
    let backend = ctx
        .file
        .backend
        .filter(|backend| backend.is_drupal_project())?;
    if !crate::class_lookup::is_subtype_of(owner, ENTITY_TYPE_MANAGER, ctx.file.class_loader) {
        return None;
    }
    let Some(Argument::Positional(argument)) = argument_list.arguments.first() else {
        return None;
    };
    let (id, _, _) = crate::virtual_members::laravel::helpers::resolved_string_literal(
        argument.value,
        ctx.file.content,
    )?;
    let mut classes: Vec<Arc<ClassInfo>> = Vec::new();
    let mut types = Vec::new();
    for (entity, storage) in backend.drupal_entity_types(id) {
        let Some(storage) = storage else {
            continue;
        };
        // A storage class the project lacks would hide the declared type.
        let Some(class) = (ctx.file.class_loader)(Fqn::new(&storage)) else {
            continue;
        };
        classes.push(class);
        types.push(PhpType::generic(storage, vec![PhpType::named(entity)]));
    }
    if classes.is_empty() {
        return None;
    }
    Some(if classes.len() == 1 {
        ResolvedType::from_classes_with_hint(classes, types.swap_remove(0))
    } else {
        classes
            .into_iter()
            .zip(types)
            .flat_map(|(class, ty)| ResolvedType::from_classes_with_hint(vec![class], ty))
            .collect()
    })
}

/// What a load on a storage tagged by [`try_resolve_entity_storage_type`]
/// returns: the entity class the tag names.
pub(super) fn try_resolve_entity_load_type(
    method_name: &str,
    receiver: &[ResolvedType],
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let loads = loads(method_name)?;
    let mut entities: Vec<Atom> = Vec::new();
    for resolved in receiver {
        let TypeKind::Generic(generic) = resolved.type_string.kind() else {
            continue;
        };
        let Some(class) = resolved.class_info.as_deref() else {
            continue;
        };
        if !crate::class_lookup::is_subtype_of(class, ENTITY_STORAGE, ctx.file.class_loader) {
            continue;
        }
        if let [entity] = generic.args.as_slice()
            && let TypeKind::Named(name) = entity.kind()
            && !entities.contains(name)
        {
            entities.push(*name);
        }
    }
    if entities.is_empty() {
        return None;
    }
    let entity = PhpType::union(entities.into_iter().map(PhpType::named).collect());
    let ty = match loads {
        Loads::MaybeOne => PhpType::nullable(entity),
        Loads::One => entity,
        Loads::Many => PhpType::generic_array(
            PhpType::union(vec![PhpType::int(), PhpType::string()]),
            entity,
        ),
    };
    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &ty,
        "",
        ctx.file.all_classes,
        ctx.file.class_loader,
    );
    Some(if classes.is_empty() {
        vec![ResolvedType::from_type_string(ty)]
    } else {
        ResolvedType::from_classes_with_hint(classes, ty)
    })
}
