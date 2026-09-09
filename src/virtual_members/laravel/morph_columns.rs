//! Lookup of morph discriminator columns through model and trait inheritance.

use std::sync::Arc;

use crate::atom::Atom;
use crate::class_lookup::load_ancestor;
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH};

#[derive(Clone, Copy)]
enum MorphColumn {
    Fixed(Atom),
    MethodName(Atom),
}

impl MorphColumn {
    fn matches(self, column: &str) -> bool {
        match self {
            Self::Fixed(name) => name.as_str() == column,
            Self::MethodName(method) => column.strip_suffix("_type").is_some_and(|column| {
                method
                    .chars()
                    .enumerate()
                    .flat_map(|(position, character)| {
                        (position > 0 && character.is_ascii_uppercase())
                            .then_some('_')
                            .into_iter()
                            .chain(character.to_lowercase())
                    })
                    .eq(column.chars())
            }),
        }
    }
}

/// Whether a model declares a `morphTo()` relationship using `column`.
///
/// The loader must return unmerged classes so a non-morph override can
/// shadow a relationship inherited from a parent or trait. Callers verify
/// that the receiver is an Eloquent model before using this metadata.
pub(crate) fn is_morph_type_column(
    class: &ClassInfo,
    column: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    let raw = class_loader(&class.fqn());
    let class = raw.as_deref().unwrap_or(class);
    has_column(class, class, column, class_loader, 0)
}

fn has_column(
    class: &ClassInfo,
    receiver: &ClassInfo,
    column: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    depth: u32,
) -> bool {
    if depth > MAX_INHERITANCE_DEPTH {
        return false;
    }
    if let Some(metadata) = class.laravel()
        && metadata
            .morph_type_columns
            .iter()
            .any(|(method, declared)| {
                declared
                    .map_or(MorphColumn::MethodName(*method), MorphColumn::Fixed)
                    .matches(column)
                    && method_column(receiver, method, class_loader, 0)
                        .flatten()
                        .is_some_and(|effective| effective.matches(column))
            })
    {
        return true;
    }
    // An excluded trait method may remain available through an alias.
    if class.trait_aliases.iter().any(|alias| {
        alias.alias.is_some_and(|name| {
            method_column(receiver, &name, class_loader, 0)
                .flatten()
                .is_some_and(|effective| effective.matches(column))
        })
    }) {
        return true;
    }
    for name in &class.used_traits {
        if let Some(trait_info) = class_loader(name)
            && has_column(&trait_info, receiver, column, class_loader, depth + 1)
        {
            return true;
        }
    }
    class.parent_class.as_ref().is_some_and(|name| {
        load_ancestor(&class.fqn(), name, class_loader)
            .is_some_and(|parent| has_column(&parent, receiver, column, class_loader, depth + 1))
    })
}

// Some(None) is a declared method without known morph metadata and must
// stop lookup; None means this branch does not supply the method.
fn method_column(
    class: &ClassInfo,
    method: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    depth: u32,
) -> Option<Option<MorphColumn>> {
    if depth > MAX_INHERITANCE_DEPTH {
        return None;
    }
    if class
        .get_method_ci(method)
        .is_some_and(|method| !method.is_virtual && !method.is_abstract)
    {
        return Some(class.laravel().and_then(|metadata| {
            metadata
                .morph_type_columns
                .iter()
                .find_map(|(name, column)| {
                    name.eq_ignore_ascii_case(method)
                        .then(|| column.map_or(MorphColumn::MethodName(*name), MorphColumn::Fixed))
                })
        }));
    }
    for alias in &class.trait_aliases {
        let Some(alias_name) = alias.alias.filter(|name| name.eq_ignore_ascii_case(method)) else {
            continue;
        };
        for name in &class.used_traits {
            if alias.trait_name.is_some_and(|source| source != *name) {
                continue;
            }
            if let Some(trait_info) = class_loader(name)
                && let Some(column) =
                    method_column(&trait_info, &alias.method_name, class_loader, depth + 1)
            {
                return Some(column.map(|column| match column {
                    MorphColumn::Fixed(_) => column,
                    MorphColumn::MethodName(_) => MorphColumn::MethodName(alias_name),
                }));
            }
        }
    }
    for name in &class.used_traits {
        if class.trait_precedences.iter().any(|precedence| {
            precedence.method_name.eq_ignore_ascii_case(method)
                && precedence.insteadof.contains(name)
        }) {
            continue;
        }
        if let Some(trait_info) = class_loader(name)
            && let Some(column) = method_column(&trait_info, method, class_loader, depth + 1)
        {
            return Some(column);
        }
    }
    let parent = load_ancestor(&class.fqn(), class.parent_class.as_ref()?, class_loader)?;
    method_column(&parent, method, class_loader, depth + 1)
}

#[cfg(test)]
#[path = "morph_columns_tests.rs"]
mod tests;
