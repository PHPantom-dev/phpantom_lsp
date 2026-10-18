//! Rebinding the generics a class fixes only through its parent.
//!
//! A class with no `@template` of its own can still stand in for a generic
//! ancestor (`class UserBuilder extends Builder {}`, or a collection whose
//! `@extends Collection<int, Item>` fixes the types), and a caller that
//! knows better type arguments has to be able to bind them.

use std::sync::Arc;

use crate::atom::{Atom, atom};
use crate::php_type::PhpType;
use crate::types::ClassInfo;

use super::resolve_class_with_inheritance;

/// Whether `class` declares no `@template` of its own but still takes
/// `new_arg_count` type arguments through its parent — the shape
/// [`rebind_extends_only_generics`] needs.
pub(crate) fn is_extends_only_generic_rebindable(
    class: &ClassInfo,
    new_arg_count: usize,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    rebindable_parent(class, new_arg_count, class_loader).is_some()
}

/// The ancestor whose `@extends` binding [`rebind_extends_only_generics`]
/// would override, together with the classes between it and the class
/// being rebound.
struct RebindTarget {
    /// The generic ancestor whose template parameters the rebind fills.
    ancestor: Atom,
    /// The ancestors between the class and `ancestor`, nearest first.
    /// Empty when `ancestor` is the direct parent.
    intermediates: Vec<Atom>,
}

/// The ancestor whose `@extends` binding [`rebind_extends_only_generics`]
/// would override, for a class that declares no `@template` of its own.
///
/// Two shapes qualify: the class fixes exactly one ancestor's generics
/// via `@extends`, or it names no generics at all and simply extends a
/// generic ancestor. The latter is how nearly every custom Eloquent
/// builder is written (`class UserBuilder extends Builder {}`): PHP has
/// no generics, so the subclass silently stands in for `Builder<TModel>`
/// and a caller that knows the model (`UserBuilder<User>`) has to be
/// able to bind it.
///
/// The generic ancestor need not be the direct parent: a project that
/// gives its builders a shared base (`class AdminUserBuilder extends
/// UserBuilder`, `class UserBuilder extends Builder`) stands in for
/// `Builder<TModel>` just as much, so the walk carries on up through
/// ancestors that declare neither templates nor a binding of their own.
/// One that does declare a binding has already fixed the generics, and
/// overriding it is not this function's business.
fn rebindable_parent(
    class: &ClassInfo,
    new_arg_count: usize,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<RebindTarget> {
    if !class.template_params.is_empty() {
        return None;
    }
    match class.extends_generics.as_slice() {
        [(parent, args)] if args.len() == new_arg_count => Some(RebindTarget {
            ancestor: *parent,
            intermediates: Vec::new(),
        }),
        [] => {
            let mut intermediates: Vec<Atom> = Vec::new();
            let mut ancestor = atom(class.parent_class.as_deref()?);
            loop {
                let parent = class_loader(&ancestor)?;
                if parent.template_params.len() == new_arg_count {
                    return Some(RebindTarget {
                        ancestor,
                        intermediates,
                    });
                }
                if !parent.template_params.is_empty() || !parent.extends_generics.is_empty() {
                    return None;
                }
                // A parent chain that loops back on itself is not valid PHP,
                // but an editor sees it while it is being written.
                if intermediates.contains(&ancestor) {
                    return None;
                }
                intermediates.push(ancestor);
                ancestor = atom(parent.parent_class.as_deref()?);
            }
        }
        _ => None,
    }
}

/// Re-derive `class` with its `@extends` generic binding replaced by
/// `new_args`, returning the overridden raw class alongside the class
/// re-merged through the parent chain under that binding.
///
/// A `static<TNewKey, TValue>` return type rebind (Laravel's
/// `Collection::keyBy()`/`groupBy()`/`mapWithKeys()` and friends) names
/// the calling class, but a concrete collection subclass that only fixes
/// its key/value types through `@extends` (`final class Sub extends
/// Collection {}` with `@extends Collection<int, Item>`) has no
/// `@template` of its own for [`apply_generic_args`] to substitute
/// against. Neither has a custom Eloquent builder, which usually names
/// no generics at all. Overriding the binding and re-running the parent
/// chain merge reproduces the same baking the original binding went
/// through, just with the rebind's args in place of the old ones.
///
/// Returns `None` when `class` is not shaped like [`is_extends_only_generic_rebindable`]
/// describes.
///
/// The caller is responsible for layering virtual members, interface
/// members, and framework patches back on — that is what the returned
/// raw class is for.
///
/// `class` may be a raw (unmerged) or already fully-resolved `ClassInfo`
/// — `extends_generics` passes through inheritance merge unchanged either
/// way. The merge base always starts from the raw class reloaded through
/// `class_loader` (the same reload the fully-resolved cache does before
/// merging), so a merged `class` (whose inherited members already bear
/// the *old* binding) is never used as the starting point — that would
/// fold the old members in as if they were the class's own and enrich
/// them from the parent instead of the override replacing them outright.
pub(crate) fn rebind_extends_only_generics(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    new_args: &[PhpType],
) -> Option<(ClassInfo, ClassInfo)> {
    let RebindTarget {
        ancestor,
        intermediates,
    } = rebindable_parent(class, new_args.len(), class_loader)?;
    let fqn = class.fqn();
    let raw = class_loader(fqn.as_str()).filter(|raw| raw.fqn().eq_ignore_ascii_case(fqn.as_str()));
    let mut overridden = raw.as_deref().unwrap_or(class).clone();
    overridden.extends_generics = vec![(ancestor, new_args.to_vec())];

    let merged = if intermediates.is_empty() {
        resolve_class_with_inheritance(&overridden, class_loader)
    } else {
        // The chain merge only reads the `@extends` binding of the class
        // whose own parent it is loading, so a binding that names an
        // ancestor further up is never seen at the level it belongs to.
        // Each intermediate is loaded carrying the same binding, which is
        // what the generic-free stand-in it is means anyway.
        let carry_binding = |name: &str| -> Option<Arc<ClassInfo>> {
            let loaded = class_loader(name)?;
            let fqn = name.strip_prefix('\\').unwrap_or(name);
            if !intermediates
                .iter()
                .any(|between| between.eq_ignore_ascii_case(fqn))
            {
                return Some(loaded);
            }
            let mut carrying = (*loaded).clone();
            carrying.extends_generics = vec![(ancestor, new_args.to_vec())];
            Some(Arc::new(carrying))
        };
        resolve_class_with_inheritance(&overridden, &carry_binding)
    };
    Some((overridden, merged))
}
