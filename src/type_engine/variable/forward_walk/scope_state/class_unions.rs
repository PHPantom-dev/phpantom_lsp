use std::sync::Arc;

use super::*;
use crate::types::ClassInfo;

/// Simplify unions in a scope by collapsing child/parent class pairs.
///
/// When merging branches produces a union like `Child | Parent` where
/// `Child extends Parent`, the union is redundant — every value of
/// type `Child` is also a `Parent`.  This collapses such unions to
/// the broadest (parent) type.
///
/// Entries that do not name a class (scalars, array shapes, generics
/// whose base is not class-like) are left alone, as are the entries of a
/// variable with only one alternative.  A `?Child` alternative counts as
/// naming its inner class, and its nullability is carried over to the
/// parent that subsumes it — dropping `?Child` in favour of a
/// non-nullable `Parent` would silently lose the null.
pub(crate) fn simplify_class_hierarchy_unions(
    scope: &mut ScopeState,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) {
    let keys: Vec<Atom> = scope.locals.keys().copied().collect();
    for key in keys {
        // Decide what to drop under an immutable borrow so the class
        // names can be borrowed rather than cloned, then apply the
        // decision under a mutable one.
        let Some(types) = scope.locals.get(&key) else {
            continue;
        };
        if types.len() < 2 {
            continue;
        }

        // (index, class name, admits null) for every alternative that
        // names a class.
        let named: Vec<(usize, &str, bool)> = types
            .iter()
            .enumerate()
            .filter_map(|(idx, rt)| {
                rt.type_string
                    .unwrap_nullable()
                    .class_name()
                    .map(|name| (idx, name, rt.type_string.accepts_null()))
            })
            .collect();
        if named.len() < 2 {
            continue;
        }

        let mut dropped = vec![false; types.len()];
        let mut widen_to_nullable = vec![false; types.len()];
        for &(parent_idx, parent_name, parent_nullable) in &named {
            if dropped[parent_idx] {
                continue;
            }
            for &(child_idx, child_name, child_nullable) in &named {
                if child_idx == parent_idx || dropped[child_idx] {
                    continue;
                }
                if crate::class_lookup::is_subclass_of(child_name, parent_name, class_loader) {
                    dropped[child_idx] = true;
                    if child_nullable && !parent_nullable {
                        widen_to_nullable[parent_idx] = true;
                    }
                }
            }
        }
        if !dropped.iter().any(|d| *d) {
            continue;
        }

        let Some(types) = scope.locals.get_mut(&key) else {
            continue;
        };
        for (idx, widen) in widen_to_nullable.iter().enumerate() {
            if *widen && !dropped[idx] {
                let widened = types[idx].type_string.clone().or_null();
                types[idx].type_string = widened;
            }
        }
        let mut idx = 0;
        types.retain(|_| {
            let keep = !dropped[idx];
            idx += 1;
            keep
        });
    }
}
