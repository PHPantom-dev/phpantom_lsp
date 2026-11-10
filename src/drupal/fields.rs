//! The fields of Drupal's content entities, read as properties.
//!
//! Every content entity reads its fields through `ContentEntityBase::__get()`,
//! which also hands back any other value stored on the entity, so any
//! property name reads on the entity class, and writes every value through
//! `ContentEntityBase::__set()`.  The interfaces a receiver is usually typed
//! with (`NodeInterface`, `ContentEntityInterface`) declare neither, so
//! [`FieldableEntityProvider`] gives them the same untyped ones the class
//! has.
//!
//! [`receiver_entity_types`] maps such a receiver to the entity types it may
//! hold, whose fields it has.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::atom::{Atom, atom};
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::types::{
    ClassInfo, ClassLikeKind, MAX_INHERITANCE_DEPTH, MethodInfo, ParameterInfo, SharedVec,
};
use crate::virtual_members::{ResolvedClassCache, VirtualMemberProvider, VirtualMembers};

/// The interface every entity with fields implements.
pub(crate) const FIELDABLE_ENTITY: &str = "Drupal\\Core\\Entity\\FieldableEntityInterface";

/// What the resolved-class cache carries for Drupal, since virtual member
/// providers receive the cache but never the `Backend`.
pub(crate) struct DrupalCacheState {
    /// Whether the workspace holds a Drupal project, shared with
    /// [`Backend::is_drupal_project`](crate::Backend::is_drupal_project).
    /// Off in a cache made without a `Backend`, so code that resolves
    /// without a project pays nothing for Drupal.
    enabled: Arc<AtomicBool>,
}

impl Default for DrupalCacheState {
    fn default() -> Self {
        Self::new(Arc::new(AtomicBool::new(false)))
    }
}

impl DrupalCacheState {
    pub(crate) fn new(enabled: Arc<AtomicBool>) -> Self {
        Self { enabled }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
}

/// Whether `class` is `FieldableEntityInterface` or extends or implements
/// it.
pub(crate) fn is_fieldable(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    // Read first, as a class that extends and implements nothing is most
    // classes, and is no subtype of anything but itself.
    (extends_or_implements(class) || class.fqn() == FIELDABLE_ENTITY)
        && is_subtype_of(class, FIELDABLE_ENTITY, class_loader)
}

/// Whether `class` names a parent or an interface, without which it is a
/// subtype of nothing but itself.
pub(super) fn extends_or_implements(class: &ClassInfo) -> bool {
    class.parent_class.is_some() || !class.interfaces.is_empty()
}

/// The members a fieldable entity interface reads and writes its fields
/// with.
pub(crate) struct FieldableEntityProvider;

impl VirtualMemberProvider for FieldableEntityProvider {
    /// Interfaces only: an entity class has its own `__get()` and `__set()`
    /// from `ContentEntityBase`, and passes the interface's on to a class
    /// that lacks them.
    fn applies_to(&self, class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
        class.kind == ClassLikeKind::Interface
            && !class
                .methods
                .iter()
                .any(|method| method.name.eq_ignore_ascii_case("__get"))
            && is_fieldable(class, class_loader)
    }

    fn provide(
        &self,
        _class: &ClassInfo,
        _class_loader: &ResolvedClassLoader<'_>,
        _cache: Option<&ResolvedClassCache>,
    ) -> VirtualMembers {
        VirtualMembers {
            methods: vec![Arc::new(magic_get()), Arc::new(magic_set())],
            properties: Vec::new(),
            constants: Vec::new(),
        }
    }
}

/// `ContentEntityBase::&__get($name)`, which declares no return type.
fn magic_get() -> MethodInfo {
    MethodInfo {
        parameters: SharedVec::from(vec![untyped_parameter("$name")]),
        ..MethodInfo::virtual_method_typed("__get", None)
    }
}

/// `ContentEntityBase::__set($name, $value)`: a write on an interface
/// receiver is as opaque as on the class, so a later read is not narrowed
/// to the written value.
fn magic_set() -> MethodInfo {
    MethodInfo {
        parameters: SharedVec::from(vec![
            untyped_parameter("$name"),
            untyped_parameter("$value"),
        ]),
        ..MethodInfo::virtual_method_typed("__set", None)
    }
}

fn untyped_parameter(name: &str) -> ParameterInfo {
    ParameterInfo {
        name: atom(name),
        is_required: true,
        type_hint: None,
        native_type_hint: None,
        description: None,
        default_value: None,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
        param_out_type: None,
        callable_invocation: None,
        expected_values: None,
    }
}

/// The IDs of the entity types `receiver` may hold, among `entity_types`
/// (each ID with its entity class): those whose class is `receiver` or a
/// subtype of it, and the one whose class `receiver` extends, as a bundle
/// class extends its entity type's class.  None unless `receiver` is
/// fieldable: an `EntityInterface` holds every entity type, but has no
/// fields.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the field readers are its callers")
)]
pub(crate) fn receiver_entity_types<'a>(
    receiver: &ClassInfo,
    entity_types: impl IntoIterator<Item = (&'a str, Atom)>,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<&'a str> {
    if !is_fieldable(receiver, class_loader) {
        return Vec::new();
    }
    let entity_types: Vec<(&str, Atom)> = entity_types.into_iter().collect();
    let receiver_fqn = receiver.fqn();
    let mut found: Vec<&str> = Vec::new();
    for &(id, class) in &entity_types {
        let holds = class_loader(Fqn::new(&class))
            .is_some_and(|class| is_subtype_of(&class, &receiver_fqn, class_loader));
        if holds && !found.contains(&id) {
            found.push(id);
        }
    }
    // The nearest class up the receiver's parents that is an entity class,
    // unless the receiver is one itself.
    if receiver.kind != ClassLikeKind::Interface
        && !entity_types.iter().any(|(_, class)| *class == receiver_fqn)
    {
        let mut parent = receiver.parent_class;
        let mut depth = 0;
        while let Some(name) = parent
            && depth < MAX_INHERITANCE_DEPTH
        {
            depth += 1;
            let declared: Vec<&str> = entity_types
                .iter()
                .filter(|(_, class)| *class == name)
                .map(|(id, _)| *id)
                .collect();
            if !declared.is_empty() {
                for id in declared {
                    if !found.contains(&id) {
                        found.push(id);
                    }
                }
                break;
            }
            parent = class_loader(Fqn::new(&name)).and_then(|class| class.parent_class);
        }
    }
    found
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;
