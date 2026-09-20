//! What `$this` is inside a trait method body.
//!
//! PHP never runs a trait's methods on the trait itself: the trait is
//! flattened into every class that uses it, so `$this` is always an
//! instance of one of those classes.  PHPStan models this by analysing a
//! trait body once per using class and never on its own; we analyse traits
//! standalone, so `$this` has to stand in for "whichever class uses this
//! trait" instead.
//!
//! Taking the trait as that stand-in is what produces the classic false
//! positive: `return $this;` from a method declared `: Type` reads as
//! "returning the trait where a `Type` is wanted", even though every class
//! using the trait implements `Type`.  Rather than the trait alone, this
//! module answers with everything all of its users are guaranteed to
//! satisfy, and intersects that with the trait so the trait's own members
//! stay reachable.

use std::sync::Arc;

use crate::atom::{Atom, atom};
use crate::types::{ClassInfo, ClassLikeKind, ResolvedType};

/// How many using classes are worth reading before the shared-ancestor
/// search gives up.
///
/// The set of shared ancestors only ever shrinks as more users are folded
/// in, so stopping early would report ancestors that some unread user does
/// not have — a guarantee we cannot make.  A trait with more users than
/// this (framework mixins like `Macroable`) therefore falls back to the
/// trait on its own rather than to a partial answer, which is what the
/// resolver did before any of this.  Traits written to be `$this`-typed
/// have a handful of users, not hundreds.
const MAX_TRAIT_USERS: usize = 24;

/// The bounds `$this` satisfies inside `trait_cls`'s method bodies, beyond
/// the trait's own members.
///
/// Two sources, in order of authority:
///
///  1. `@phpstan-require-extends` / `@phpstan-require-implements`, which
///     state the bound outright.  A class that uses the trait without
///     satisfying them is itself an error, so the tags can be trusted.
///  2. Failing a tag, the classes and interfaces every using class in the
///     project has in common.  `$this` is one of those users, so whatever
///     they all are, it is.
///
/// Returns an empty vector for a non-trait, and for a trait whose users
/// share nothing (or are too numerous to read — see [`MAX_TRAIT_USERS`]).
pub(crate) fn trait_this_bounds(
    trait_cls: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&crate::Backend>,
) -> Vec<Arc<ClassInfo>> {
    if trait_cls.kind != ClassLikeKind::Trait {
        return Vec::new();
    }

    let load = |name: &str| -> Option<Arc<ClassInfo>> {
        crate::class_lookup::find_class_by_name(all_classes, name)
            .map(Arc::clone)
            .or_else(|| class_loader(name))
    };

    let mut bounds: Vec<Arc<ClassInfo>> = Vec::new();
    if let Some(ref required) = trait_cls.require_extends
        && let Some(cls) = load(required)
    {
        ClassInfo::push_unique_arc(&mut bounds, cls);
    }
    for required in &trait_cls.require_implements {
        if let Some(cls) = load(required) {
            ClassInfo::push_unique_arc(&mut bounds, cls);
        }
    }
    if !bounds.is_empty() {
        return bounds;
    }

    for name in shared_user_ancestors(trait_cls, all_classes, class_loader, backend) {
        if let Some(cls) = load(&name) {
            ClassInfo::push_unique_arc(&mut bounds, cls);
        }
    }
    bounds
}

/// The classes that use `trait_cls`, loaded.
///
/// Where [`trait_this_bounds`] answers "what is every host guaranteed to
/// be" — a single set of bounds all of them satisfy — this answers with
/// the hosts themselves.  A context that can hold a union wants this
/// one: `$x instanceof self` inside a trait proves `$x` is one of the
/// hosts, and each host's own members are reachable from that union
/// even when the hosts share no ancestor that declares them.
///
/// Returns an empty vector for a non-trait, for a trait with no host in
/// the project, and for one with more hosts than [`MAX_TRAIT_USERS`].
pub(crate) fn trait_host_classes(
    trait_cls: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&crate::Backend>,
) -> Vec<Arc<ClassInfo>> {
    if trait_cls.kind != ClassLikeKind::Trait {
        return Vec::new();
    }
    let mut hosts: Vec<Arc<ClassInfo>> = Vec::new();
    for name in trait_users(trait_cls, all_classes, class_loader, backend) {
        let loaded = crate::class_lookup::find_class_by_name(all_classes, &name)
            .map(Arc::clone)
            .or_else(|| class_loader(&name));
        // A host we cannot read leaves the union describing fewer classes
        // than it should, which would rule out members the missing one
        // declares.  Answering with nothing keeps the caller on the
        // trait itself rather than on a union that is wrong.
        let Some(cls) = loaded else {
            return Vec::new();
        };
        ClassInfo::push_unique_arc(&mut hosts, cls);
    }
    hosts
}

/// The FQNs every class using `trait_cls` is an instance of.
///
/// A using class counts as an instance of itself, so a trait with a single
/// user reports that user; with several, only what they share survives the
/// intersection.  Traits are left out: a second trait says nothing about
/// `$this`'s class, and the trait we started from is already the other half
/// of the intersection.
fn shared_user_ancestors(
    trait_cls: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&crate::Backend>,
) -> Vec<String> {
    let users = trait_users(trait_cls, all_classes, class_loader, backend);
    if users.is_empty() {
        return Vec::new();
    }

    let mut shared: Option<Vec<String>> = None;
    for user in &users {
        let Some(cls) = class_loader(user) else {
            // A user we cannot read could be an instance of anything, so
            // there is nothing the whole set is guaranteed to share.
            return Vec::new();
        };
        let mut names = Vec::new();
        collect_instance_of(&cls, class_loader, &mut names, 0);
        shared = Some(match shared {
            None => names,
            Some(prev) => prev
                .into_iter()
                .filter(|p| names.iter().any(|n| n.eq_ignore_ascii_case(p)))
                .collect(),
        });
        if shared.as_ref().is_some_and(|s| s.is_empty()) {
            return Vec::new();
        }
    }
    shared.unwrap_or_default()
}

/// Maximum ancestry depth walked while collecting what a class is an
/// instance of.  Deeper than any real hierarchy; a backstop against a
/// malformed `extends` cycle the loader hands back.
const MAX_ANCESTRY_DEPTH: u8 = 15;

/// Append every class and interface FQN that an instance of `cls` also is,
/// `cls` itself included.
fn collect_instance_of(
    cls: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    out: &mut Vec<String>,
    depth: u8,
) {
    if depth > MAX_ANCESTRY_DEPTH || cls.kind == ClassLikeKind::Trait {
        return;
    }
    let fqn = cls.fqn().to_string();
    if out.iter().any(|n| n.eq_ignore_ascii_case(&fqn)) {
        return;
    }
    out.push(fqn);

    for ancestor in cls
        .parent_class
        .as_ref()
        .into_iter()
        .chain(cls.interfaces.iter())
    {
        if let Some(loaded) = class_loader(ancestor) {
            collect_instance_of(&loaded, class_loader, out, depth + 1);
        }
    }
}

/// The FQNs of the classes that `use` this trait, directly or through
/// another trait.
///
/// A trait that uses this one is not itself a possible class for `$this`;
/// it passes the requirement on to whatever uses *it*, so the walk follows
/// through and reports only real classes.
///
/// Returns an empty vector once the set passes [`MAX_TRAIT_USERS`] — see
/// there for why a partial answer is worse than none.
fn trait_users(
    trait_cls: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&crate::Backend>,
) -> Vec<String> {
    let mut users: Vec<String> = Vec::new();
    let mut queue: Vec<Atom> = vec![trait_cls.fqn()];
    let mut seen: Vec<Atom> = queue.clone();

    while let Some(trait_fqn) = queue.pop() {
        let mut candidates: Vec<String> = Vec::new();
        for cls in all_classes {
            if cls
                .used_traits
                .iter()
                .any(|t| t.eq_ignore_ascii_case(trait_fqn.as_str()))
            {
                candidates.push(cls.fqn().to_string());
            }
        }
        // The reverse inheritance index answers project-wide; the current
        // file's own classes are read above as well, because a file being
        // edited is re-indexed asynchronously and may not be in the index
        // yet.
        if let Some(backend) = backend {
            let gti = backend.symbols.gti_index.read();
            if let Some(children) = gti.get(trait_fqn.as_str()) {
                candidates.extend(children.iter().cloned());
            }
        }

        for candidate in candidates {
            let is_trait = class_loader(&candidate)
                .is_some_and(|cls| cls.kind == ClassLikeKind::Trait)
                || all_classes.iter().any(|c| {
                    c.kind == ClassLikeKind::Trait && c.fqn().eq_ignore_ascii_case(&candidate)
                });
            let key = atom(&candidate);
            if seen.iter().any(|s| s.eq_ignore_ascii_case(key.as_str())) {
                continue;
            }
            seen.push(key);
            if is_trait {
                queue.push(key);
                continue;
            }
            users.push(candidate);
            if users.len() > MAX_TRAIT_USERS {
                return Vec::new();
            }
        }
    }

    users
}

/// Attach `trait_cls`'s [`trait_this_bounds`] to a `$this` resolution.
///
/// The bounds are added as further entries so member lookup reaches them,
/// and the whole set is tagged as an intersection: `$this` is the trait's
/// members *and* every bound simultaneously, not a choice between them.
/// A union would make a compatibility check against one bound answer for
/// the trait as well, which is the false positive this exists to avoid.
pub(crate) fn extend_this_with_trait_bounds(
    this_types: &mut Vec<ResolvedType>,
    trait_cls: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&crate::Backend>,
) {
    if trait_cls.kind != ClassLikeKind::Trait {
        return;
    }
    let bounds = trait_this_bounds(trait_cls, all_classes, class_loader, backend);
    if bounds.is_empty() {
        return;
    }
    ResolvedType::extend_unique(
        this_types,
        bounds.into_iter().map(ResolvedType::from_arc).collect(),
    );
    ResolvedType::tag_as_intersection(this_types);
}
