//! Reverse index for the Eloquent `$pivot` attribute.
//!
//! A model gains a `$pivot` attribute only when it is reached *through* a
//! many-to-many (`belongsToMany`/`morphToMany`) relationship. Because member
//! resolution is keyed on class FQN, the related model has no way, on its own,
//! to know it is such a target. This module builds a project-wide reverse map
//! (`related-model FQN → pivot type`) from every model's relationship methods,
//! so that `$pivot` is injected onto exactly the models that are many-to-many
//! targets — and typed from the relationship's `TPivotModel` generic (with the
//! parsed `->using(...)` class as a fallback, then the base `Pivot`).
//!
//! The index is consulted at class-load time via [`inject_pivot`], mirroring
//! the `inject_macros` path. Like the macro index it is an LSP-time structure;
//! `analyze` leaves `$pivot` unmodelled, where model `__get` leniency keeps it
//! quiet.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::types::{ClassInfo, PropertyInfo, PropertySource, ELOQUENT_PIVOT_FQN};
use crate::php_type::PhpType;

use super::relationships::{
    classify_relationship_typed, extract_pivot_type_typed, extract_related_type_typed,
    is_pivot_relationship, resolve_related_fqn, RelationshipKind,
};

/// Project-wide map from a related-model FQN to the pivot type exposed on its
/// `$pivot` attribute when reached through a many-to-many relationship.
#[derive(Default)]
pub(crate) struct LaravelPivotIndex {
    /// related-model FQN → pivot type.
    map: HashMap<String, PhpType>,
    /// URIs of files that declared at least one many-to-many relationship.
    /// Used to detect when an edit removes the last such relationship from a
    /// file so the index can be invalidated.
    contributing_uris: HashSet<String>,
}

impl LaravelPivotIndex {
    /// The pivot type for `fqn`, if that model is a many-to-many target.
    pub(crate) fn get(&self, fqn: &str) -> Option<&PhpType> {
        self.map.get(fqn)
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
    PhpType::Named(ELOQUENT_PIVOT_FQN.to_owned())
}

/// Resolve the pivot type for one many-to-many relationship method.
///
/// Priority: the relationship's third generic (`TPivotModel`), then the
/// parsed `->using(...)` class, then the base `Pivot`.
fn pivot_type_for(
    declaring: &ClassInfo,
    method_name: &str,
    return_type: &PhpType,
    loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> PhpType {
    if let Some(pivot) = extract_pivot_type_typed(return_type)
        && let Some(name) = pivot.base_name()
    {
        if let Some(cls) = resolve_related_fqn(name, declaring, loader) {
            return PhpType::Named(cls.fqn().to_string());
        }
        return PhpType::Named(name.trim_start_matches('\\').to_string());
    }

    if let Some(laravel) = declaring.laravel()
        && let Some(pivot) = laravel
            .belongs_to_many_pivots
            .iter()
            .find(|p| p.method == method_name)
        && let Some(using) = &pivot.using
    {
        return PhpType::Named(using.clone());
    }

    base_pivot_type()
}

/// Build the reverse pivot index from every parsed class.
///
/// `classes` is a snapshot of `(uri, class)` pairs; `loader` resolves related
/// and pivot class names to loadable FQNs. When two relationships target the
/// same related model with conflicting pivot types, the entry falls back to the
/// base `Pivot` (the access path is ambiguous).
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
            let related_fqn = related_cls.fqn().to_string();

            let pivot_ty = pivot_type_for(class, method.name.as_str(), return_type, loader);

            index.contributing_uris.insert(uri.clone());
            match index.map.get(&related_fqn) {
                Some(existing) if existing != &pivot_ty => {
                    // Ambiguous: the same related model is reached through
                    // relationships with different pivots. Fall back to base.
                    index.map.insert(related_fqn, base.clone());
                }
                Some(_) => {}
                None => {
                    index.map.insert(related_fqn, pivot_ty);
                }
            }
        }
    }

    index
}

/// Inject the `$pivot` attribute onto `class` when it is a many-to-many
/// target, typed from the reverse index. A declared `pivot` property is left
/// untouched.
pub(crate) fn inject_pivot(index: &LaravelPivotIndex, class: Arc<ClassInfo>) -> Arc<ClassInfo> {
    let Some(pivot_ty) = index.get(class.fqn().as_str()) else {
        return class;
    };
    if class.properties.iter().any(|p| p.name == "pivot") {
        return class;
    }

    let mut cloned = ClassInfo::clone(&class);
    cloned.properties.push(PropertyInfo {
        source: Some(PropertySource::Pivot),
        ..PropertyInfo::virtual_property_typed("pivot", Some(pivot_ty))
    });
    Arc::new(cloned)
}

#[cfg(test)]
#[path = "pivots_tests.rs"]
mod tests;
