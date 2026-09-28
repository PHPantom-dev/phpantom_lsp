//! Walking a class's inheritance chain.
//!
//! Every feature that asks "which ancestor declares this member?" or
//! "does any parent carry this attribute?" used to write its own bounded
//! `parent_class` loop, most of them copying the whole `ClassInfo` on
//! every hop. These helpers walk the chain once, hand out the cached
//! `Arc` the loader already holds, and apply the same depth cap so a
//! mid-edit cycle cannot spin.

use std::collections::HashSet;
use std::sync::Arc;

use crate::atom::Atom;
use crate::types::{ClassInfo, ClassLikeKind, MAX_INHERITANCE_DEPTH, MAX_TRAIT_DEPTH};

/// A class loader, as every hierarchy walk receives it.
pub(crate) type ClassLoader<'a> = &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>;

/// The parents of `class`, nearest first, each with the name it was loaded
/// by.
///
/// Lazy on purpose: most callers stop at the first level that answers,
/// and the walk runs on hot paths (every member access the assembled
/// class does not carry), so materialising the chain would allocate a
/// vector to read one entry of it. The walk ends at the first parent the
/// loader cannot produce.
pub(crate) fn ancestors<'a>(
    class: &ClassInfo,
    class_loader: ClassLoader<'a>,
) -> impl Iterator<Item = (Atom, Arc<ClassInfo>)> + use<'a> {
    let mut next = class.parent_class;
    let mut depth = 0u32;
    std::iter::from_fn(move || {
        let name = next?;
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            return None;
        }
        let ancestor = class_loader(&name)?;
        next = ancestor.parent_class;
        Some((name, ancestor))
    })
}

/// The ancestor of `class` for which `declares` holds, with the name it
/// was loaded by, in PHP's member precedence order: the class's own traits
/// (and the traits they use), then each parent together with its traits,
/// then the interfaces the class and its parents implement, each followed
/// up its own extends chain.
///
/// `class` itself is not tested; a caller that wants the class's own
/// declaration to win checks it first.
pub(crate) fn find_declaring_ancestor(
    class: &ClassInfo,
    class_loader: ClassLoader<'_>,
    declares: &dyn Fn(&ClassInfo) -> bool,
) -> Option<(Atom, Arc<ClassInfo>)> {
    if let Some(found) = find_declaring_trait(&class.used_traits, class_loader, declares) {
        return Some(found);
    }

    // The interfaces of every level are checked only after the whole
    // class chain has been searched, since a concrete declaration
    // anywhere up the chain wins over an interface's.
    let mut interfaces: Vec<Atom> = class.interfaces.clone();
    for (name, parent) in ancestors(class, class_loader) {
        if declares(&parent) {
            return Some((name, parent));
        }
        if let Some(found) = find_declaring_trait(&parent.used_traits, class_loader, declares) {
            return Some(found);
        }
        for iface in &parent.interfaces {
            if !interfaces.contains(iface) {
                interfaces.push(*iface);
            }
        }
    }

    let mut visited: HashSet<Atom> = HashSet::new();
    interfaces
        .iter()
        .find_map(|iface| find_declaring_interface(*iface, class_loader, declares, &mut visited, 0))
}

/// The first of `trait_names`, or of the traits they use, for which
/// `declares` holds, with the name it was loaded by.
pub(crate) fn find_declaring_trait(
    trait_names: &[Atom],
    class_loader: ClassLoader<'_>,
    declares: &dyn Fn(&ClassInfo) -> bool,
) -> Option<(Atom, Arc<ClassInfo>)> {
    find_declaring_trait_at(trait_names, class_loader, declares, 0)
}

fn find_declaring_trait_at(
    trait_names: &[Atom],
    class_loader: ClassLoader<'_>,
    declares: &dyn Fn(&ClassInfo) -> bool,
    depth: u32,
) -> Option<(Atom, Arc<ClassInfo>)> {
    if depth > MAX_TRAIT_DEPTH {
        return None;
    }
    for name in trait_names {
        let Some(used) = class_loader(name) else {
            continue;
        };
        if declares(&used) {
            return Some((*name, used));
        }
        if let Some(found) =
            find_declaring_trait_at(&used.used_traits, class_loader, declares, depth + 1)
        {
            return Some(found);
        }
    }
    None
}

/// `iface_name` or the first interface it extends for which `declares`
/// holds. An interface's parents are recorded both in `interfaces` and,
/// for a single `extends`, in `parent_class`.
///
/// `visited` holds every interface already searched without a match, so
/// that the parent both fields name, or one reached along two extends
/// paths, is loaded once rather than once per path to it.
fn find_declaring_interface(
    iface_name: Atom,
    class_loader: ClassLoader<'_>,
    declares: &dyn Fn(&ClassInfo) -> bool,
    visited: &mut HashSet<Atom>,
    depth: u32,
) -> Option<(Atom, Arc<ClassInfo>)> {
    if depth > MAX_INHERITANCE_DEPTH || !visited.insert(iface_name) {
        return None;
    }
    let iface = class_loader(&iface_name)?;
    if declares(&iface) {
        return Some((iface_name, iface));
    }
    iface
        .interfaces
        .iter()
        .chain(iface.parent_class.as_ref())
        .find_map(|parent| {
            find_declaring_interface(*parent, class_loader, declares, visited, depth + 1)
        })
}

/// Every class and interface an instance of `class` also is: its parent
/// chain, every interface it or a parent implements, and every interface
/// those extend. `class` itself is not included, and traits are not
/// followed, since using a trait does not make a class an instance of it.
///
/// Each supertype comes once, with the name it was loaded by: a class's
/// interfaces (each followed up its extends chain) before its parent, so a
/// caller reading the result in order meets a class's own contracts before
/// its parent's. A supertype the loader cannot produce is left out, along
/// with whatever only it would have led to.
pub(crate) fn supertypes(
    class: &ClassInfo,
    class_loader: ClassLoader<'_>,
) -> Vec<(Atom, Arc<ClassInfo>)> {
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(class.fqn().to_ascii_lowercase());
    let mut out = Vec::new();
    collect_supertypes(class, class_loader, &mut visited, &mut out, 0);
    out
}

fn collect_supertypes(
    class: &ClassInfo,
    class_loader: ClassLoader<'_>,
    visited: &mut HashSet<String>,
    out: &mut Vec<(Atom, Arc<ClassInfo>)>,
    depth: u32,
) {
    if depth > MAX_INHERITANCE_DEPTH {
        return;
    }
    // An interface's first `extends` is recorded as its `parent_class`, and
    // it is one of its contracts like any other; a class's parent comes
    // after the interfaces it implements.
    let (first, then): (&[Atom], &[Atom]) = if class.kind == ClassLikeKind::Interface {
        (class.parent_class.as_slice(), &class.interfaces)
    } else {
        (&class.interfaces, class.parent_class.as_slice())
    };
    for name in first.iter().chain(then) {
        let Some(loaded) = class_loader(name) else {
            continue;
        };
        if !visited.insert(loaded.fqn().to_ascii_lowercase()) {
            continue;
        }
        out.push((*name, Arc::clone(&loaded)));
        collect_supertypes(&loaded, class_loader, visited, out, depth + 1);
    }
}
