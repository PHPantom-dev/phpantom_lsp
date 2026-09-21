//! Declaration walk helpers for member definition resolution.
//!
//! These functions walk the class inheritance chain (parent classes,
//! traits, interfaces, mixins) to find the class that actually declares
//! a given member.  They are used by `resolve_member_definition_with`
//! to locate the source file and position of the member's declaration.

use crate::Backend;
use crate::atom::Atom;
use crate::inheritance::{ancestors, find_declaring_ancestor, find_declaring_trait};
use crate::types::*;
use std::sync::Arc;

use crate::util::build_fqn;

use super::MemberAccessHint;

impl Backend {
    /// Resolve a trait `as` alias on a class.
    ///
    /// If `member_name` matches a trait alias declared on the class, returns
    /// the original method name and (optionally) the source trait name.
    /// Otherwise returns `member_name` unchanged with no trait hint.
    pub(in crate::definition) fn resolve_trait_alias(
        class: &ClassInfo,
        member_name: &str,
    ) -> (String, Option<String>) {
        for alias in &class.trait_aliases {
            if alias.alias.as_deref() == Some(member_name) {
                return (
                    alias.method_name.to_string(),
                    alias.trait_name.map(|a| a.to_string()),
                );
            }
        }
        (member_name.to_string(), None)
    }

    /// Walk up the inheritance chain to find the class that actually declares
    /// the given member and the FQN (or best-known name) used to load it.
    ///
    /// Returns `Some((ClassInfo, fqn))` of the declaring class, or `None` if
    /// the member cannot be found in any ancestor.  The `fqn` is the name
    /// that was passed to `class_loader` to obtain the `ClassInfo`, which is
    /// a fully-qualified name for parents and traits.  For the class itself
    /// (when the member is declared directly), the FQN is reconstructed
    /// from `file_namespace` + `name` when a namespace is available so
    /// that `find_class_file_content` can disambiguate classes that share
    /// the same short name (e.g. `Eloquent\Builder` vs `Query\Builder`).
    ///
    /// Interfaces are searched too, since one can declare `@method` /
    /// `@property` tags worth jumping to; `@mixin` classes come last, on
    /// the class itself and then on each ancestor.
    pub(in crate::definition) fn find_declaring_class(
        class: &ClassInfo,
        member_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<(ClassInfo, String)> {
        let declares = |candidate: &ClassInfo| {
            Self::classify_member(candidate, member_name, MemberAccessHint::Unknown).is_some()
        };

        if declares(class) {
            let fqn = build_fqn(&class.name, class.file_namespace.as_deref());
            return Some((class.clone(), fqn));
        }

        if let Some((name, declaring)) = find_declaring_ancestor(class, class_loader, &declares) {
            return Some((Arc::unwrap_or_clone(declaring), name.to_string()));
        }

        if let Some(found) =
            Self::find_declaring_in_mixins(&class.mixins, member_name, class_loader, 0)
        {
            return Some(found);
        }

        // e.g. `User extends Model` where `Model` has `@mixin Builder`.
        ancestors(class, class_loader).find_map(|(_, parent)| {
            if parent.mixins.is_empty() {
                return None;
            }
            Self::find_declaring_in_mixins(&parent.mixins, member_name, class_loader, 0)
        })
    }

    /// Search through a list of trait names for one that declares `member_name`.
    ///
    /// Traits can themselves `use` other traits, so this recurses up to a
    /// depth limit to handle trait composition.
    ///
    /// Returns `(ClassInfo, fqn)` where `fqn` is the fully-qualified name
    /// that was used to load the declaring class from `class_loader`.
    pub(in crate::definition) fn find_declaring_in_traits(
        trait_names: &[Atom],
        member_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<(ClassInfo, String)> {
        let declares = |candidate: &ClassInfo| {
            Self::classify_member(candidate, member_name, MemberAccessHint::Unknown).is_some()
        };
        find_declaring_trait(trait_names, class_loader, &declares)
            .map(|(name, declaring)| (Arc::unwrap_or_clone(declaring), name.to_string()))
    }

    /// Search through `@mixin` class names for one that declares `member_name`.
    ///
    /// Mixin classes are resolved with their full inheritance chain (parent
    /// classes, traits) so that inherited members are found.  Mixin classes
    /// can themselves declare `@mixin`, so this recurses up to a depth
    /// limit.
    ///
    /// Returns `(ClassInfo, fqn)` where `fqn` is the fully-qualified name
    /// that was used to load the declaring class from `class_loader`.
    pub(in crate::definition) fn find_declaring_in_mixins(
        mixin_names: &[Atom],
        member_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        depth: usize,
    ) -> Option<(ClassInfo, String)> {
        if depth > MAX_MIXIN_DEPTH as usize {
            return None;
        }

        for mixin_name in mixin_names {
            let Some(mixin_class) = class_loader(mixin_name) else {
                continue;
            };

            // Try to find the declaring class within the mixin's own
            // hierarchy (itself, its traits, its parents).
            if let Some((declaring_class, fqn)) =
                Self::find_declaring_class(&mixin_class, member_name, class_loader)
            {
                // When find_declaring_class finds the member directly on
                // the mixin class, it returns the short name (e.g.
                // "Builder") because ClassInfo.name is always short.
                // Replace it with the fully-qualified mixin_name so that
                // find_class_file_content can disambiguate classes that
                // share the same short name (e.g. Eloquent\Builder vs
                // Query\Builder).
                if !fqn.contains('\\') && fqn == mixin_class.name {
                    return Some((declaring_class, mixin_name.to_string()));
                }
                return Some((declaring_class, fqn));
            }

            // Recurse into mixins declared by this mixin class.
            if !mixin_class.mixins.is_empty()
                && let Some(found) = Self::find_declaring_in_mixins(
                    &mixin_class.mixins,
                    member_name,
                    class_loader,
                    depth + 1,
                )
            {
                return Some(found);
            }
        }

        None
    }
}
