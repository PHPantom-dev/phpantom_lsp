//! Reverse index for Eloquent pivot accessors.
//!
//! A model gains a `$pivot` attribute only when it is reached *through* a
//! many-to-many (`belongsToMany`/`morphToMany`) relationship. A relation may
//! rename that attribute with `->as('name')`. Because member resolution is
//! keyed on class FQN, the related model has no way, on its own, to know it is
//! such a target. This module builds a project-wide reverse map from every
//! model's relationship methods, so the configured accessors are injected onto
//! exactly the many-to-many targets and typed from the relationship's
//! `TPivotModel` generic (with the parsed `->using(...)` class as a fallback,
//! then the base `Pivot`).
//!
//! The index is consulted at class-load time via [`inject_pivot`], mirroring
//! the `inject_macros` path. Like the macro index it is an LSP-time structure;
//! `analyze` leaves pivot accessors unmodelled, where model `__get` leniency
//! keeps them quiet.

use std::collections::HashSet;
use std::sync::Arc;

use crate::atom::{Atom, AtomMap, atom};
use crate::php_type::PhpType;
use crate::types::{
    ClassInfo, ELOQUENT_PIVOT_FQN, PivotAccessor, PivotRelation, PropertyInfo, PropertySource,
};

use super::relationships::{
    RelationshipKind, classify_relationship_typed, extract_pivot_accessor_typed,
    extract_pivot_type_typed, extract_related_type_typed, is_pivot_relationship,
    resolve_related_fqn,
};

struct PivotProperty {
    name: Atom,
    ty: PhpType,
}

/// Project-wide map from a related-model FQN to the pivot accessors exposed
/// when it is reached through a many-to-many relationship.
#[derive(Default)]
pub(crate) struct LaravelPivotIndex {
    /// related-model FQN → pivot accessors and their types.
    map: AtomMap<Vec<PivotProperty>>,
    /// URIs of files that declared at least one many-to-many relationship.
    /// Used to detect when an edit removes the last such relationship from a
    /// file so the index can be invalidated.
    contributing_uris: HashSet<String>,
}

impl LaravelPivotIndex {
    /// The type of `accessor` on `fqn`, if that model is a many-to-many target
    /// reached through a relationship with that accessor.
    #[cfg(test)]
    pub(crate) fn get(&self, fqn: &str, accessor: &str) -> Option<&PhpType> {
        self.map
            .get(&atom(fqn))?
            .iter()
            .find(|entry| entry.name == accessor)
            .map(|entry| &entry.ty)
    }

    /// Whether the index holds no targets at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Whether `uri` contributed a many-to-many relationship to this index.
    pub(crate) fn contributes(&self, uri: &str) -> bool {
        self.contributing_uris.contains(uri)
    }
}

fn base_pivot_type() -> PhpType {
    PhpType::named(atom(ELOQUENT_PIVOT_FQN))
}

/// Resolve the accessor name for one many-to-many relationship method.
///
/// A literal fourth generic is authoritative. Otherwise the parsed
/// `->as(...)` configuration is used, falling back to Laravel's `$pivot`
/// default. A dynamic accessor cannot be represented as a named property.
fn pivot_accessor_for(return_type: &PhpType, config: Option<&PivotRelation>) -> Option<Atom> {
    if let Some(accessor) = extract_pivot_accessor_typed(return_type) {
        return Some(accessor);
    }

    if let Some(config) = config {
        return match config.accessor {
            PivotAccessor::Default => Some(atom("pivot")),
            PivotAccessor::Custom(name) => Some(name),
            PivotAccessor::Unknown => None,
        };
    }

    Some(atom("pivot"))
}

/// Resolve the pivot type for one many-to-many relationship method.
///
/// Priority: the relationship's third generic (`TPivotModel`), then the
/// parsed `->using(...)` class, then the base `Pivot`.
fn pivot_type_for(
    declaring: &ClassInfo,
    return_type: &PhpType,
    config: Option<&PivotRelation>,
    loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> PhpType {
    if let Some(pivot) = extract_pivot_type_typed(return_type)
        && let Some(name) = pivot.base_name()
    {
        if let Some(cls) = resolve_related_fqn(name, declaring, loader) {
            return PhpType::named(cls.fqn());
        }
        return PhpType::named(atom(name.trim_start_matches(char::from(92))));
    }

    if let Some(using) = config.and_then(|pivot| pivot.using.as_deref()) {
        return PhpType::named(atom(using));
    }

    base_pivot_type()
}

/// Build the reverse pivot index from every parsed class.
///
/// `classes` is a snapshot of `(uri, class)` pairs; `loader` resolves related
/// and pivot class names to loadable FQNs. When two relationships target the
/// same related model and accessor with conflicting pivot types, that accessor
/// falls back to the base `Pivot` (the access path is ambiguous). Different
/// accessor names coexist on the target model.
pub(crate) fn build_pivot_index(
    classes: &[(String, Arc<ClassInfo>)],
    loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> LaravelPivotIndex {
    let mut index = LaravelPivotIndex::default();
    let base = base_pivot_type();

    for (uri, class) in classes {
        for method in class.methods.iter() {
            let Some(return_type) = method.return_type.as_ref() else {
                continue;
            };
            if classify_relationship_typed(return_type) != Some(RelationshipKind::Collection)
                || !is_pivot_relationship(return_type)
            {
                continue;
            }
            let Some(related) = extract_related_type_typed(return_type).and_then(|t| t.base_name())
            else {
                continue;
            };
            let Some(related_cls) = resolve_related_fqn(related, class, loader) else {
                continue;
            };
            let related_fqn = related_cls.fqn();

            index.contributing_uris.insert(uri.clone());
            let config = class.laravel().and_then(|laravel| {
                laravel
                    .belongs_to_many_pivots
                    .iter()
                    .find(|pivot| pivot.method == method.name.as_str())
            });
            let Some(accessor) = pivot_accessor_for(return_type, config) else {
                continue;
            };
            let pivot_ty = pivot_type_for(class, return_type, config, loader);

            let entries = index.map.entry(related_fqn).or_default();
            match entries.iter_mut().find(|entry| entry.name == accessor) {
                Some(existing) if existing.ty != pivot_ty => {
                    // Ambiguous: the same accessor is reached through
                    // relationships with different pivots. Fall back to base.
                    existing.ty = base.clone();
                }
                Some(_) => {}
                None => {
                    entries.push(PivotProperty {
                        name: accessor,
                        ty: pivot_ty,
                    });
                }
            }
        }
    }

    index
}

/// Inject each known pivot accessor onto `class` when it is a many-to-many
/// target, typed from the reverse index. Declared properties are left
/// untouched.
pub(crate) fn inject_pivot(index: &LaravelPivotIndex, class: Arc<ClassInfo>) -> Arc<ClassInfo> {
    let Some(pivots) = index.map.get(&class.fqn()) else {
        return class;
    };
    let mut cloned = None;
    for pivot in pivots {
        if class.properties.iter().any(|p| p.name == pivot.name) {
            continue;
        }
        let cloned = cloned.get_or_insert_with(|| ClassInfo::clone(&class));
        cloned.properties.push(Arc::new(PropertyInfo {
            source: Some(PropertySource::Pivot),
            ..PropertyInfo::virtual_property_typed(pivot.name.as_str(), Some(&pivot.ty))
        }));
    }
    cloned.map_or(class, Arc::new)
}

#[cfg(test)]
#[path = "pivots_tests.rs"]
mod tests;
