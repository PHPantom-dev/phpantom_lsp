//! PHPDoc virtual member provider.
//!
//! Extracts `@method`, `@property` / `@property-read` / `@property-write`,
//! and `@mixin` tags from the class-level docblock and presents them as
//! virtual members.  This is the second-highest-priority virtual member
//! provider: framework providers (e.g. Laravel) take precedence, but
//! PHPDoc-sourced members beat all other virtual member sources.
//!
//! Within this provider, `@method` and `@property` tags take precedence
//! over `@mixin` members: if a class declares both `@property int $id`
//! and `@mixin SomeClass` where `SomeClass` also has an `$id` property,
//! the `@property` tag wins.

mod mixins;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::atom::{Atom, AtomSet};
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::inheritance;
use crate::inheritance::bind_template_args;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, DocblockMembers, MethodInfo, PropertyInfo, PropertySource};

use super::{VirtualMemberProvider, VirtualMembers};
use mixins::{
    MixinCollector, MixinDedup, collect_class_mixin_members, template_bound_ancestors,
    uses_trait_with_mixins,
};

pub use mixins::resolve_template_param_mixins;
pub(super) use mixins::uses_forwards_calls;

/// Build a virtual [`PropertyInfo`] from a parsed `@property` tag,
/// substituting template parameters in the declared type when `subs`
/// carries bindings for the docblock's owner.
fn doc_property(
    name: Atom,
    type_hint: Option<&PhpType>,
    subs: &HashMap<String, PhpType>,
) -> PropertyInfo {
    let type_hint = type_hint.map(|t| {
        if subs.is_empty() {
            t.clone()
        } else {
            t.substitute(subs)
        }
    });
    PropertyInfo::virtual_property_owned(name, type_hint)
}

/// Like [`doc_property`], but marked [`PropertySource::DocblockTag`].
///
/// Used for `@property` tags read from the class itself, its traits,
/// parents, and interfaces — declarations the user wrote *about this
/// class*, which override schema/convention-inferred types during the
/// virtual-member merge.  `@mixin`-borrowed tags stay unmarked: they
/// describe the mixin class, and the provider treats them as the
/// lowest-precedence source.
fn doc_tag_property(
    name: Atom,
    type_hint: Option<&PhpType>,
    subs: &HashMap<String, PhpType>,
) -> PropertyInfo {
    PropertyInfo {
        source: Some(PropertySource::DocblockTag),
        ..doc_property(name, type_hint, subs)
    }
}

/// Apply template substitutions to a parsed `@method` tag.
///
/// The tags are parsed once per class and shared behind an `Arc`, so the
/// common case (no substitution for this consumer) is a refcount bump
/// rather than a deep clone.
fn substituted_doc_method(
    method: &Arc<MethodInfo>,
    subs: &HashMap<String, PhpType>,
) -> Arc<MethodInfo> {
    if subs.is_empty() {
        return Arc::clone(method);
    }
    let mut cloned = (**method).clone();
    inheritance::apply_substitution_to_method(&mut cloned, subs);
    Arc::new(cloned)
}

/// Virtual member provider for `@method`, `@property`, and `@mixin` docblock tags.
///
/// When a class declares `@method` or `@property` tags in its class-level
/// docblock, those tags describe magic members accessible via `__call`,
/// `__get`, and `__set`.  When a class declares `@mixin ClassName`, all
/// public members of `ClassName` (and its inheritance chain) become
/// available via magic methods.
///
/// Resolution order within this provider:
/// 1. `@method` and `@property` tags (highest precedence)
/// 2. `@mixin` class members (lower precedence, never overwrite tags)
///
/// Mixins are inherited: if `User extends Model` and `Model` has
/// `@mixin Builder`, then `User` also gains Builder's public members.
/// The provider walks the parent chain to collect mixin declarations
/// from ancestors.
///
/// Mixin classes can themselves declare `@mixin`, so the provider
/// recurses up to [`MAX_MIXIN_DEPTH`] levels.
pub struct PHPDocProvider;

impl VirtualMemberProvider for PHPDocProvider {
    /// Returns `true` if the class carries `@method` / `@property` tags
    /// or declares `@mixin` tags (directly or via traits and ancestors).
    ///
    /// This is a cheap pre-check: the tags were parsed once when each
    /// class was extracted, so this only inspects already-structured data.
    fn applies_to(&self, class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
        // Declares @method/@property tags itself.
        if class.doc_members.is_some() {
            return true;
        }

        // Has direct @mixin declarations.
        if !class.mixins.is_empty() {
            return true;
        }

        // Has used traits that carry @method/@property tags.
        for trait_name in &class.used_traits {
            if let Some(trait_info) = class_loader(Fqn::new(trait_name))
                && trait_info.doc_members.is_some()
            {
                return true;
            }
        }

        if uses_trait_with_mixins(class, class_loader) {
            return true;
        }

        // Walk the parent chain to check for ancestor mixins or tags.
        for (_, parent) in inheritance::ancestors(class, class_loader) {
            if !parent.mixins.is_empty()
                || parent.doc_members.is_some()
                || uses_trait_with_mixins(&parent, class_loader)
            {
                return true;
            }
            // `provide` also reads tags off the traits an ancestor uses.
            for trait_name in &parent.used_traits {
                if let Some(trait_info) = class_loader(Fqn::new(trait_name))
                    && trait_info.doc_members.is_some()
                {
                    return true;
                }
            }
        }

        // Walk implemented interfaces (and the interfaces they extend),
        // which `provide` also collects tags from.
        let mut queue: Vec<Atom> = class.interfaces.to_vec();
        let mut visited: AtomSet = AtomSet::default();
        while let Some(iface_name) = queue.pop() {
            if !visited.insert(iface_name) {
                continue;
            }
            let Some(iface) = class_loader(Fqn::new(&iface_name)) else {
                continue;
            };
            if iface.doc_members.is_some() {
                return true;
            }
            queue.extend(iface.interfaces.iter().copied());
        }

        false
    }

    /// Collect `@method`, `@property`, and `@mixin` members for the class.
    ///
    /// The `@method` / `@property` tags were parsed into
    /// [`ClassInfo::doc_members`](crate::types::ClassInfo::doc_members)
    /// when each class was extracted, so this only reads structured data
    /// and applies the per-consumer generic substitutions.  It then
    /// collects public members from `@mixin` classes.  Within the
    /// provider, `@method` / `@property` tags take precedence over
    /// `@mixin` members.
    fn provide(
        &self,
        class: &ClassInfo,
        class_loader: &ResolvedClassLoader<'_>,
        cache: Option<&super::ResolvedClassCache>,
    ) -> VirtualMembers {
        let ancestors = template_bound_ancestors(class, class_loader);
        let tags = DocTags::of(class, &ancestors, class_loader);
        let mut collector = MixinCollector {
            methods: tags.methods,
            properties: tags.properties,
            constants: Vec::new(),
            dedup: MixinDedup {
                methods: tags.seen_methods,
                properties: tags.seen_props,
                constants: class.constants.iter().map(|c| c.name).collect(),
            },
        };
        collect_class_mixin_members(class, &ancestors, class_loader, &mut collector, cache);

        VirtualMembers {
            methods: collector.methods,
            properties: collector.properties,
            constants: collector.constants,
        }
    }
}

/// The `@method` and `@property` tags a class answers to, each name once.
///
/// Collected before the `@mixin` members, whose dedup sets these names
/// seed, so that a tag always beats a mixin member of the same name.
struct DocTags {
    methods: Vec<Arc<MethodInfo>>,
    properties: Vec<PropertyInfo>,
    /// Seeded from the base-resolved class's methods (real and inherited).
    seen_methods: AtomSet,
    /// Not seeded from the class's properties: the class's own
    /// `@property` tags are always emitted, so that
    /// `merge_virtual_members` can compare type specificity and keep the
    /// most specific type (`array<string>` from `@property` beats a bare
    /// `array` from `$casts`).  Their names then keep the lower-priority
    /// sources (trait and parent tags, `@mixin` members) from overriding
    /// them.
    seen_props: AtomSet,
}

impl DocTags {
    /// The tags of `class` itself, then of its traits, of each parent (the
    /// `ancestors`, with their bindings) and the traits that parent uses,
    /// and of its interfaces and the interfaces they extend, with each
    /// source's template parameters bound the way `class` binds them.
    ///
    /// Inherited tags are read here rather than by the inheritance merge:
    /// real trait and parent members are merged by then, but a tag exists
    /// only on the docblock that declares it.
    fn of(
        class: &ClassInfo,
        ancestors: &[(Arc<ClassInfo>, HashMap<String, PhpType>)],
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Self {
        let mut tags = DocTags {
            methods: Vec::new(),
            properties: Vec::new(),
            seen_methods: class.methods.iter().map(|m| m.name).collect(),
            seen_props: AtomSet::default(),
        };

        if let Some(doc) = class.doc_members.as_deref() {
            tags.add_own(doc);
        }
        tags.add_trait_tags(class, &HashMap::new(), class_loader);

        // `@extends Parent<…>` binds the parent's template parameters, so
        // `@method T get()` on a parent comes out as the concrete type.  A
        // trait the parent uses is merged into the parent, so its tags
        // reach the child just like the parent's own: Laravel's
        // higher-order collection proxies are declared on the
        // `EnumeratesValues` trait that `Support\Collection` uses.
        for (parent, level_subs) in ancestors {
            if let Some(doc) = parent.doc_members.as_deref() {
                tags.add(doc, level_subs);
            }
            tags.add_trait_tags(parent, level_subs, class_loader);
        }

        // `@implements I<…>` binds an interface's template parameters, and
        // the interfaces it extends (its `interfaces`) are walked with those
        // bindings carried along.
        let mut iface_queue: Vec<(Atom, HashMap<String, PhpType>)> = class
            .interfaces
            .iter()
            .filter_map(|iface_name| {
                let iface = class_loader(Fqn::new(iface_name))?;
                Some((*iface_name, build_interface_substitution_map(class, &iface)))
            })
            .collect();
        let mut visited: HashSet<Atom> = HashSet::new();
        while let Some((iface_name, subs)) = iface_queue.pop() {
            if !visited.insert(iface_name) {
                continue;
            }
            let Some(iface) = class_loader(Fqn::new(&iface_name)) else {
                continue;
            };
            if let Some(doc) = iface.doc_members.as_deref() {
                tags.add(doc, &subs);
            }
            for parent_iface_name in &iface.interfaces {
                if let Some(parent_iface) = class_loader(Fqn::new(parent_iface_name)) {
                    let parent_subs =
                        build_interface_extends_substitution_map(&iface, &parent_iface, &subs);
                    iface_queue.push((*parent_iface_name, parent_subs));
                }
            }
        }

        tags
    }

    /// Add every tag of the class's own docblock, whatever came before.
    fn add_own(&mut self, doc: &DocblockMembers) {
        for m in &doc.methods {
            self.seen_methods.insert(m.name);
            self.methods.push(Arc::clone(m));
        }
        for (name, type_hint) in &doc.properties {
            self.seen_props.insert(*name);
            self.properties
                .push(doc_tag_property(*name, type_hint.as_ref(), &HashMap::new()));
        }
    }

    /// Add the tags of `doc` that no nearer source declared, with `subs`
    /// binding the template parameters they are written against.
    fn add(&mut self, doc: &DocblockMembers, subs: &HashMap<String, PhpType>) {
        for m in &doc.methods {
            if self.seen_methods.insert(m.name) {
                self.methods.push(substituted_doc_method(m, subs));
            }
        }
        for (name, type_hint) in &doc.properties {
            if self.seen_props.insert(*name) {
                self.properties
                    .push(doc_tag_property(*name, type_hint.as_ref(), subs));
            }
        }
    }

    /// Add the tags of the traits `owner` uses, which are written against
    /// each trait's template parameters, bound by `owner`'s `@use Trait<…>`
    /// on top of `owner_subs`.
    fn add_trait_tags(
        &mut self,
        owner: &ClassInfo,
        owner_subs: &HashMap<String, PhpType>,
        class_loader: &ResolvedClassLoader<'_>,
    ) {
        for trait_name in &owner.used_traits {
            let Some(trait_info) = class_loader(Fqn::new(trait_name)) else {
                continue;
            };
            if let Some(doc) = trait_info.doc_members.as_deref() {
                let trait_subs = build_trait_substitution_map(owner, &trait_info, owner_subs);
                self.add(doc, &trait_subs);
            }
        }
    }
}

/// Build a substitution map for a directly implemented interface.
///
/// Maps the interface's template parameters to the concrete types provided
/// in the class's `@implements` generics.
fn build_interface_substitution_map(
    class: &ClassInfo,
    iface: &ClassInfo,
) -> HashMap<String, PhpType> {
    bind_template_args(iface, &class.implements_generics, &HashMap::new()).unwrap_or_default()
}

/// Build a substitution map for an interface's parent interface (interface extends).
///
/// Maps the parent interface's template parameters to concrete types by
/// resolving through the child interface's `@extends` generics and applying
/// the already-accumulated substitutions.
fn build_interface_extends_substitution_map(
    child_iface: &ClassInfo,
    parent_iface: &ClassInfo,
    active_subs: &HashMap<String, PhpType>,
) -> HashMap<String, PhpType> {
    bind_template_args(parent_iface, &child_iface.extends_generics, active_subs)
        .unwrap_or_else(|| active_subs.clone())
}

/// Map a used trait's template parameters onto the consuming class's
/// `@use Trait<…>` arguments, composed with the substitutions already in
/// force at that point in the inheritance walk.
///
/// Falls back to `active_subs` when the trait is not parameterised or the
/// consumer declared no `@use` generics — in that case the trait's tags are
/// written against whatever names the consumer happens to share, which is
/// the same assumption the class's own trait tags are read under.
fn build_trait_substitution_map(
    consumer: &ClassInfo,
    trait_info: &ClassInfo,
    active_subs: &HashMap<String, PhpType>,
) -> HashMap<String, PhpType> {
    bind_template_args(trait_info, &consumer.use_generics, active_subs)
        .unwrap_or_else(|| active_subs.clone())
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "phpdoc_tests.rs"]
mod tests;
