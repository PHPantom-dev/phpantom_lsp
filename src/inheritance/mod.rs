//! Base class inheritance resolution.
//!
//! This module handles merging members from parent classes and traits
//! into a single `ClassInfo`.  The resulting merged class contains the
//! base set of members visible on an instance / static access,
//! respecting PHP's precedence rules:
//!
//!   class own > traits > parent chain
//!
//! `@mixin` members are handled separately by
//! [`PHPDocProvider`](crate::virtual_members::phpdoc::PHPDocProvider) in
//! the virtual member provider layer.
//!
//! This module also supports **generic type substitution**: when a child
//! class declares `@extends Parent<ConcreteType1, ConcreteType2>` and the
//! parent has `@template T1` / `@template T2`, the inherited methods and
//! properties have their template parameter references replaced with the
//! concrete types.

mod ancestor;
pub(crate) use ancestor::extract_generic_arg_from_ancestor;

pub mod enrichment;
pub mod generics;
pub mod traits;

use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::{Atom, AtomSet, atom};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH, Visibility};
use crate::virtual_members::{
    TransformFingerprint, intern_transformed_method, intern_transformed_property,
};

// Re-export functions that are used internally
pub(crate) use enrichment::enrich_method_arc_from_ancestor;
pub(crate) use enrichment::enrich_property_arc_from_ancestor;

#[cfg(test)]
pub(crate) use generics::apply_substitution;
pub(crate) use generics::{
    apply_generic_args, apply_substitution_to_conditional, apply_substitution_to_method,
    apply_substitution_to_property, bind_inherited_class_keywords, build_generic_subs,
    build_substitution_map, class_scoped_template_values, default_type_args, fill_template_bounds,
    method_has_inherited_class_keyword, method_references_params, property_references_params,
    template_values_with_defaults,
};

/// A borrow-or-owned handle to a `ClassInfo`, used to walk the parent
/// chain in [`resolve_class_with_inheritance`] without cloning the root
/// class.
///
/// The first iteration borrows the caller-provided `&ClassInfo` (zero
/// allocation).  Subsequent iterations hold the `Arc<ClassInfo>` returned
/// by the class loader (a cheap Arc move).
pub(crate) enum ClassRef<'a> {
    Borrowed(&'a ClassInfo),
    Owned(Arc<ClassInfo>),
}

impl std::ops::Deref for ClassRef<'_> {
    type Target = ClassInfo;
    #[inline]
    fn deref(&self) -> &ClassInfo {
        match self {
            ClassRef::Borrowed(r) => r,
            ClassRef::Owned(a) => a,
        }
    }
}

/// Bundles the trait-level configuration passed through
/// [`merge_traits_into`] so the function stays within clippy's
/// argument-count limit.
pub(crate) struct TraitContext<'a> {
    /// Generic type arguments for `@use Trait<Type>` declarations.
    pub use_generics: &'a [(Atom, Vec<PhpType>)],
    /// `insteadof` precedence declarations.
    pub precedences: &'a [crate::types::TraitPrecedence],
    /// `as` alias declarations.
    pub aliases: &'a [crate::types::TraitAlias],
}

/// Tracks member names already present during inheritance merging.
///
/// Passed through `resolve_class_with_inheritance` and `merge_traits_into`
/// (including recursive calls) so that every addition is checked in O(1)
/// instead of scanning the full member vectors.
pub(crate) struct MergeDedup {
    /// Method names already merged, lowercased (PHP method names are
    /// case-insensitive, so a child `getvalue()` overrides a parent
    /// `getValue()`).
    pub methods: AtomSet,
    /// Property names already merged.
    pub properties: AtomSet,
    /// Constant names already merged.
    pub constants: AtomSet,
}

impl MergeDedup {
    /// Build from the members already present on a `ClassInfo`.
    fn from_class(class: &ClassInfo) -> Self {
        Self {
            methods: class
                .methods
                .iter()
                .map(|m| crate::atom::ascii_lowercase_atom(&m.name))
                .collect(),
            properties: class.properties.iter().map(|p| p.name).collect(),
            constants: class.constants.iter().map(|c| c.name).collect(),
        }
    }

    /// Copy every member of `source` a subclass inherits into `merged`,
    /// sharing each one's `Arc` rather than cloning it.
    ///
    /// A private member is not inherited, and a name already merged wins
    /// over the one further up the chain, which is PHP's own precedence.
    /// This is the plain form, without the generic substitution and
    /// docblock enrichment the `extends` walk in
    /// [`resolve_class_with_inheritance`] layers on top.
    pub(crate) fn merge_visible_members(&mut self, source: &ClassInfo, merged: &mut ClassInfo) {
        for method in &source.methods {
            if method.visibility != Visibility::Private
                && self
                    .methods
                    .insert(crate::atom::ascii_lowercase_atom(&method.name))
            {
                merged.methods.push(Arc::clone(method));
            }
        }
        for property in &source.properties {
            if property.visibility != Visibility::Private && self.properties.insert(property.name) {
                merged.properties.push(Arc::clone(property));
            }
        }
        for constant in &source.constants {
            if constant.visibility != Visibility::Private && self.constants.insert(constant.name) {
                merged.constants.push(Arc::clone(constant));
            }
        }
    }
}

use crate::virtual_members::laravel::{factory_model_type, is_factory_class};

/// Resolve a class together with all inherited members from its parent
/// chain.
///
/// Walks up the `extends` chain via `class_loader`, collecting public and
/// protected methods, properties, and constants from each ancestor.
/// If a child already defines a member with the same name as a parent
/// member, the child's version wins (even if the signatures differ).
///
/// Private members are never inherited.
///
/// When the child declares `@extends Parent<Type1, Type2>` and the parent
/// has `@template` parameters, the inherited members have their template
/// parameter types replaced with the concrete types from the `@extends`
/// annotation.  This substitution chains through the entire ancestry.
///
/// A depth limit of 20 prevents infinite loops from circular inheritance.
pub(crate) fn resolve_class_with_inheritance(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> ClassInfo {
    let mut merged = class.clone();

    // Build dedup sets from the class's own members.  These are passed
    // through trait merging and the parent chain walk so that every
    // addition is tracked in O(1) across all recursion levels.
    let mut dedup = MergeDedup::from_class(&merged);

    // A `@method` tag names something `__call()` handles, and PHP only
    // reaches `__call()` when no accessible method of that name exists.
    // A tag that collides with a real method from a trait or the parent
    // chain is therefore dead documentation, however precise it looks:
    // the walk below merges the real method, and the PHPDoc provider's
    // virtual member loses to it during the virtual-member merge.

    // 1. Merge traits used by this class.
    //    PHP precedence: class methods > trait methods > inherited methods.
    //    Since `merged` already contains the class's own members, we only
    //    add trait members that don't collide with existing ones.
    traits::merge_traits_into(
        &mut merged,
        &class.used_traits,
        &TraitContext {
            use_generics: &class.use_generics,
            precedences: &class.trait_precedences,
            aliases: &class.trait_aliases,
        },
        class_loader,
        0,
        &mut dedup,
        &class.fqn(),
    );

    // 2. Walk up the `extends` chain and merge parent members.
    //
    // `current` holds a reference to the class whose `parent_class`,
    // `extends_generics`, `used_traits`, etc. we read at each level.
    // For the first iteration this is the root `class` (a borrow —
    // zero allocation).  After that it becomes the `Arc<ClassInfo>`
    // returned by `class_loader` (a cheap Arc move).
    let mut current: ClassRef<'_> = ClassRef::Borrowed(class);
    let mut depth = 0;

    // The substitution map accumulates as we walk the chain.
    // It maps template parameter names → concrete types, and is
    // re-computed at each level based on the `@extends` generics
    // of the current class and the `@template` params of the parent.
    let mut active_subs: HashMap<String, PhpType> = HashMap::new();

    // Seed the initial substitution map from the root class's
    // `@extends` generics.  If the root class has
    // `@extends Collection<int, Language>`, this will be applied
    // when we load `Collection` as the first parent.
    //
    // We don't apply it yet — it's matched against the parent's
    // template_params in the loop below.

    while let Some(ref parent_name) = current.parent_class {
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            break;
        }

        let parent = if let Some(p) =
            crate::class_lookup::load_ancestor(&current.fqn(), parent_name, class_loader)
        {
            p
        } else {
            break;
        };

        // `readonly` covers the whole hierarchy: PHP rejects a
        // non-readonly class extending a readonly one, so a subclass is
        // readonly whether or not it repeats the keyword, and the
        // properties it inherits are readonly as well.
        merged.is_readonly |= parent.is_readonly;

        // Build the substitution map for this parent level.
        //
        // Look through current's `extends_generics` for an entry
        // whose class name matches this parent, and zip its type
        // arguments with the parent's `template_params`.
        let mut level_subs = build_substitution_map(&current, &parent, &active_subs);

        // ── Laravel Factory model binding ────────────────────────
        // Set `TModel` from the source-aware factory hierarchy lookup:
        // a concrete generic binding, then `$model`, then the convention.
        if is_factory_class(parent_name)
            && !parent.template_params.is_empty()
            && let Some(model_type) = factory_model_type(class, class_loader)
        {
            // Re-apply the source-aware answer even when `level_subs` already
            // contains `Model`: an unbound template on an intermediate base is
            // filled from its bound during the preceding level, but that
            // fallback must not hide the concrete factory's `$model`. A real
            // explicit binding is returned by `factory_model_type` instead and
            // therefore remains authoritative here.
            for param in &parent.template_params {
                level_subs.insert(param.to_string(), model_type.clone());
            }
        }

        // ── Template bound fallback ─────────────────────────────
        // When a subclass extends a generic parent without providing
        // explicit `@extends` generics and no convention-based
        // substitution filled the map, fall back to the template
        // parameter bounds (e.g. `@template T of object` → `object`)
        // so that inherited methods don't leak raw template names.
        fill_template_bounds(&parent, &mut level_subs);

        // Merge traits used by the parent class as well, so that
        // grandparent-level trait members are visible.
        // Apply the current level's template substitutions to the
        // parent's `@use` generics.  Without this, a chain like:
        //
        //   /** @extends DataCollection<int, DeliveryOption> */
        //   class DeliveryOptionCollection extends DataCollection
        //
        // where DataCollection has:
        //   /** @use EnumerableMethods<TKey, TValue> */
        //
        // would pass the raw `TKey`/`TValue` template params to the
        // trait instead of the concrete `int`/`DeliveryOption` types.
        let substituted_use_generics: Vec<(Atom, Vec<PhpType>)> = if level_subs.is_empty() {
            parent.use_generics.clone()
        } else {
            parent
                .use_generics
                .iter()
                .map(|(name, args)| {
                    let substituted_args: Vec<PhpType> =
                        args.iter().map(|arg| arg.substitute(&level_subs)).collect();
                    (*name, substituted_args)
                })
                .collect()
        };

        traits::merge_traits_into(
            &mut merged,
            &parent.used_traits,
            &TraitContext {
                use_generics: &substituted_use_generics,
                precedences: &parent.trait_precedences,
                aliases: &parent.trait_aliases,
            },
            class_loader,
            0,
            &mut dedup,
            &parent.fqn(),
        );

        // Template parameter names substituted at this level, used to
        // decide per member whether substitution would change anything.
        // Empty when the parent is non-generic, in which case every
        // member below keeps its shared `Arc` / avoids the substitution
        // walk (copy-on-write).
        let sub_keys: Vec<String> = if level_subs.is_empty() {
            Vec::new()
        } else {
            level_subs.keys().cloned().collect()
        };

        // Interning fingerprints for the three transform shapes a parent
        // method can need (substitution, relative-keyword binding, or
        // both), computed once per level.  The keyword binding is fully
        // determined by the declaring parent (its own parent is what a bare
        // `parent` binds to), so the transformed copies are identical for
        // every subclass and can be shared through the interning store.
        let parent_fqn = parent.fqn();
        let declaring_parent = parent.parent_class.map(|a| a.to_string());
        let fp_sub = TransformFingerprint::new(Some(&level_subs), None, 0);
        let fp_self = TransformFingerprint::new(None, Some(parent_fqn.as_str()), 0);
        let fp_both = TransformFingerprint::new(Some(&level_subs), Some(parent_fqn.as_str()), 0);

        // Merge parent methods — skip private.
        // When the child already has a method with the same name,
        // enrich it with the parent's richer docblock types instead
        // of silently discarding the parent's type information.
        for method in &parent.methods {
            if method.visibility == Visibility::Private {
                continue;
            }
            let needs_sub = method_references_params(method, &sub_keys);
            if !dedup
                .methods
                .insert(crate::atom::ascii_lowercase_atom(&method.name))
            {
                // Child already has this method — enrich it from parent.
                // Only clone + substitute when the parent's signature
                // actually references a substituted template param;
                // otherwise enrich directly from the shared method.
                if let Some(existing) = merged
                    .methods
                    .make_mut()
                    .iter_mut()
                    .find(|m| m.name.eq_ignore_ascii_case(&method.name))
                {
                    if needs_sub {
                        let ancestor_method = intern_transformed_method(method, fp_sub, || {
                            let mut m = (**method).clone();
                            apply_substitution_to_method(&mut m, &level_subs);
                            m
                        });
                        enrich_method_arc_from_ancestor(existing, &ancestor_method);
                    } else {
                        enrich_method_arc_from_ancestor(existing, method);
                    }
                }
                continue;
            }
            // Bind bare `self` / `parent` in the return type and parameter
            // hints to the declaring (parent) class and its own parent, so
            // they resolve to the classes that the declaration meant rather
            // than to the inheriting child and its parent.
            let needs_keywords =
                method_has_inherited_class_keyword(method, declaring_parent.as_deref());
            if !needs_sub && !needs_keywords {
                // Substitution is a no-op and there is no relative keyword:
                // keep the shared `Arc` instead of deep-cloning.
                merged.methods.push(Arc::clone(method));
                continue;
            }
            let fp = match (needs_sub, needs_keywords) {
                (true, false) => fp_sub,
                (false, true) => fp_self,
                _ => fp_both,
            };
            let transformed = intern_transformed_method(method, fp, || {
                let mut ancestor_method = (**method).clone();
                if needs_sub {
                    apply_substitution_to_method(&mut ancestor_method, &level_subs);
                }
                if needs_keywords {
                    bind_inherited_class_keywords(
                        &mut ancestor_method,
                        &parent_fqn,
                        declaring_parent.as_deref(),
                    );
                }
                ancestor_method
            });
            merged.methods.push(transformed);
        }

        // Merge parent properties — same enrichment logic as methods.
        // The substitution transform reuses `fp_sub` (a property and a
        // method with the same substitution map fingerprint to the same
        // value, but they intern into separate maps keyed by origin type,
        // so there is no collision).
        for property in &parent.properties {
            if property.visibility == Visibility::Private {
                continue;
            }
            let needs_sub = property_references_params(property, &sub_keys);
            if !dedup.properties.insert(property.name) {
                // Child already has this property — enrich it from parent.
                if let Some(existing) = merged
                    .properties
                    .make_mut()
                    .iter_mut()
                    .find(|p| p.name == property.name)
                {
                    if needs_sub {
                        let ancestor_property =
                            intern_transformed_property(property, fp_sub, || {
                                let mut p = (**property).clone();
                                apply_substitution_to_property(&mut p, &level_subs);
                                p
                            });
                        enrich_property_arc_from_ancestor(existing, &ancestor_property);
                    } else {
                        enrich_property_arc_from_ancestor(existing, property);
                    }
                }
                continue;
            }
            if !needs_sub {
                // Substitution is a no-op: keep the shared `Arc`.
                merged.properties.push(Arc::clone(property));
                continue;
            }
            let transformed = intern_transformed_property(property, fp_sub, || {
                let mut p = (**property).clone();
                apply_substitution_to_property(&mut p, &level_subs);
                p
            });
            merged.properties.push(transformed);
        }

        // Merge parent constants.  Constants are never transformed on
        // inheritance, so an inherited constant is always a plain
        // `Arc::clone` of the declaring class's.
        for constant in &parent.constants {
            if constant.visibility == Visibility::Private {
                continue;
            }
            if !dedup.constants.insert(constant.name) {
                continue;
            }
            merged.constants.push(Arc::clone(constant));
        }

        // Carry the substitution map forward for the next level.
        // If `Collection` extends `AbstractCollection<TKey, TValue>`,
        // we need to apply the current substitutions to those type
        // arguments so that `TKey` → `int` flows through.
        active_subs = level_subs;
        current = ClassRef::Owned(parent);
    }

    // 3. Enrich methods from implemented interfaces.
    //    When a class overrides an interface method without a return type,
    //    propagate the interface method's return type (with template
    //    substitution from `@implements` generics).
    let class_fqn = class.fqn();
    for iface_name in &class.interfaces {
        let Some(iface) = crate::class_lookup::load_ancestor(&class_fqn, iface_name, class_loader)
        else {
            continue;
        };

        // Build substitution map from @implements/@template-implements generics.
        //
        // Any of the interface's own @template params this class doesn't
        // bind explicitly (e.g. a plain `implements ArrayAccess` with no
        // `@implements ArrayAccess<TKey, TValue>` at all) falls back to its
        // declared bound (or `mixed`) here. Without this, a docblock return
        // type like `ArrayAccess`'s own `@return TValue` would leak through
        // `enrich_method_arc_from_ancestor` below as if "TValue" were a real,
        // resolvable class, clobbering the class's own concrete override
        // (e.g. `offsetGet(): Pen`). This fallback is local to interface
        // enrichment; `build_substitution_map` itself must keep leaving an
        // absent annotation unresolved, since other consumers (the `extends`
        // chain walk, Laravel factory model detection) rely on that absence
        // to fall through to their own convention-based resolution.
        let mut iface_subs =
            build_substitution_map(&ClassRef::Borrowed(class), &iface, &HashMap::new());
        fill_template_bounds(&iface, &mut iface_subs);
        let iface_sub_keys: Vec<String> = iface_subs.keys().cloned().collect();
        let fp_iface = TransformFingerprint::new(Some(&iface_subs), None, 0);

        for method in &iface.methods {
            // Only enrich methods that the class already has (i.e. overrides).
            if let Some(existing) = merged
                .methods
                .make_mut()
                .iter_mut()
                .find(|m| m.name.eq_ignore_ascii_case(&method.name))
            {
                if method_references_params(method, &iface_sub_keys) {
                    let ancestor_method = intern_transformed_method(method, fp_iface, || {
                        let mut m = (**method).clone();
                        apply_substitution_to_method(&mut m, &iface_subs);
                        m
                    });
                    enrich_method_arc_from_ancestor(existing, &ancestor_method);
                } else {
                    enrich_method_arc_from_ancestor(existing, method);
                }
            }
        }
    }

    // Retype an inherited non-public property that the class documents
    // with a `@property` tag of its own.
    //
    // A tag documents a magic read, and PHP only reaches `__get()` when no
    // *accessible* property of that name exists.  An ancestor's
    // `protected` / `private` declaration is invisible from outside, so it
    // is not what the read yields and its type must not describe it — an
    // Eloquent model declaring `@property string $connection` means
    // `string`, not `Model::$connection`'s `\UnitEnum|string|null`. The
    // same shadowing is routine for `$table` and `$keyType`.
    //
    // A property the class declares *itself* is a different matter: it is
    // in scope everywhere the tag is, so it keeps its own type (the tag is
    // then a contradiction, and the real declaration is the truth).
    if let Some(doc) = class.doc_members.as_deref() {
        for (name, type_hint) in &doc.properties {
            let Some(hint) = type_hint else {
                continue;
            };
            if class.properties.iter().any(|p| p.name == *name) {
                continue;
            }
            // Look the index up immutably so a class with nothing to
            // retype keeps sharing its property vector.
            let Some(idx) = merged
                .properties
                .iter()
                .position(|p| p.name == *name && p.visibility != Visibility::Public)
            else {
                continue;
            };
            let prop = &mut merged.properties.make_mut()[idx];
            Arc::make_mut(prop).type_hint = Some(hint.clone());
        }
    }

    // Refine the `value` property on backed enums.  The `BackedEnum`
    // interface declares `public readonly int|string $value`, but each
    // concrete backed enum knows its specific backing type.  Replace
    // the generic union with the precise type so that hover, completion,
    // and diagnostics see `string` or `int` instead of `int|string`.
    if let Some(ref backed) = merged.backed_type {
        let specific_type = match backed {
            crate::types::BackedEnumType::String => PhpType::named(atom("string")),
            crate::types::BackedEnumType::Int => PhpType::named(atom("int")),
        };
        if let Some(prop) = merged
            .properties
            .make_mut()
            .iter_mut()
            .find(|p| p.name == "value")
        {
            Arc::make_mut(prop).type_hint = Some(specific_type);
        }
    }

    // Refine the `cases()` method on enums.  The `UnitEnum` interface
    // declares `public static function cases(): array`, which loses the
    // element type: `Country::cases()[0]` would resolve to `mixed`.
    // Every concrete enum returns a list of its own instances, so
    // replace the bare `array` with `list<EnumName>` (using the FQN so
    // the element resolves regardless of the call site's namespace).
    if merged.kind == crate::types::ClassLikeKind::Enum {
        let element = PhpType::named(merged.fqn());
        let list_type = PhpType::list(element);
        if let Some(cases) = merged
            .methods
            .make_mut()
            .iter_mut()
            .find(|m| m.name.eq_ignore_ascii_case("cases"))
        {
            Arc::make_mut(cases).return_type = Some(list_type);
        }
    }

    merged
}

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
            if !intermediates
                .iter()
                .any(|between| between.eq_ignore_ascii_case(name))
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

/// Look up a method's return type through the inheritance chain.
///
/// Resolves inheritance for `class`, finds the method named
/// `method_name`, and returns its `return_type`.  This is a
/// convenience wrapper around [`resolve_class_fully`](crate::virtual_members::resolve_class_fully)
/// that eliminates the repeated merge → find → extract pattern
/// used across many modules.
///
/// Uses full resolution (base inheritance + virtual member providers)
/// so that virtual methods from `@method` tags, `@mixin` classes,
/// and framework providers are included.
pub(crate) fn resolve_method_return_type(
    class: &ClassInfo,
    method_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    // Try the class directly first — it may already be fully resolved
    // with generic substitutions applied.  Falling through to the cache
    // would return the un-substituted base class (keyed by bare FQN),
    // losing template parameter substitutions like TModel → Product.
    //
    // A declaration that states no return type at all is not an answer,
    // though: an override written `public function getFileName()` with
    // `{@inheritDoc}` above it says nothing, and what it means is what the
    // ancestor said.  That type lives on the merged class, so fall through
    // to it rather than reporting the override's silence as "unknown".
    if let Some(m) = class.get_method(method_name)
        && m.return_type.is_some()
    {
        return m.return_type.clone();
    }
    let cache = crate::virtual_members::active_resolved_class_cache();
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(class, class_loader, cache);
    merged
        .methods
        .iter()
        .find(|m| m.name == method_name)
        .and_then(|m| m.return_type.clone())
}

/// Look up a property's type hint through the inheritance chain.
///
/// Resolves inheritance for `class`, finds the property named
/// `prop_name`, and returns its `type_hint`.  This is a
/// convenience wrapper around [`resolve_class_fully`](crate::virtual_members::resolve_class_fully)
/// that eliminates the repeated merge → find → extract pattern
/// used across many modules.
///
/// Uses full resolution (base inheritance + virtual member providers)
/// so that virtual properties from `@property` tags, `@mixin` classes,
/// and framework providers are included.
pub(crate) fn resolve_property_type_hint(
    class: &ClassInfo,
    prop_name: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    // Try the class directly first — it may already have the property
    // with generic substitutions applied.
    if let Some(p) = class.properties.iter().find(|p| p.name == prop_name)
        && p.type_hint.is_some()
    {
        let hint = p.type_hint.clone().unwrap();
        return Some(replace_self_in_property_type(hint, class));
    }
    let cache = crate::virtual_members::active_resolved_class_cache();
    let merged =
        crate::virtual_members::resolve_class_fully_maybe_cached(class, class_loader, cache);
    if let Some(hint) = merged
        .properties
        .iter()
        .find(|p| p.name == prop_name)
        .and_then(|p| p.type_hint.clone())
    {
        return Some(replace_self_in_property_type(hint, class));
    }

    // Eloquent relation properties resolve case-insensitively: access like
    // `$model->orderproducts` flows through `__get()` → `isRelation()` →
    // `method_exists()`, so a relation-backed virtual property matches the
    // relationship regardless of the case used at the access site.
    if crate::virtual_members::laravel::class_has_relation_method_ci(&merged, prop_name)
        && let Some(hint) = merged
            .properties
            .iter()
            .find(|p| p.is_virtual && p.name.eq_ignore_ascii_case(prop_name))
            .and_then(|p| p.type_hint.clone())
    {
        return Some(replace_self_in_property_type(hint, class));
    }

    // Fallback: if the class has a `__get` method with method-level
    // template parameters and an IndexAccess return type (e.g.
    // `@template K as key-of<TData>` / `@return TData[K]`), infer K
    // from the property name and evaluate the indexed access.
    // Try the original class first — it may already carry generic
    // substitutions (e.g. from `apply_generic_args`) so `__get`'s
    // return type is already concrete.
    if let Some(ty) = resolve_magic_get_return_type(class, prop_name) {
        return Some(ty);
    }
    resolve_magic_get_return_type(&merged, prop_name)
}

/// Replace `self`/`static`/`$this` references in a property type with
/// the owning class's fully qualified name.
///
/// Skips replacement for synthetic classes (like `__object_shape`) where
/// `self` refers to the caller's context, not the synthetic class itself.
fn replace_self_in_property_type(ty: PhpType, class: &ClassInfo) -> PhpType {
    if ty.contains_self_ref() && !class.name.starts_with("__") {
        ty.replace_self(&class.fqn())
    } else {
        ty
    }
}

/// Resolve the return type of a property access through a `__get` magic
/// method whose return type indexes a shape by the accessed property name.
///
/// For example, given `@return array{a: int, b: string}[K]` on `__get`
/// with a method-level `@template K`, accessing `$obj->a` infers `K = 'a'`
/// from the property name and evaluates the index access to `int`.
fn resolve_magic_get_return_type(class: &ClassInfo, prop_name: &str) -> Option<PhpType> {
    let get_method = class
        .methods
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("__get"))?;

    let return_type = get_method.return_type.as_ref()?;

    // When __get has no template params, return the declared return type
    // directly (with self/static resolved to the owning class).
    if get_method.template_params.is_empty() {
        let resolved = if return_type.contains_self_ref() {
            return_type.replace_self(&class.fqn())
        } else {
            return_type.clone()
        };
        return Some(resolved);
    }

    // Build a substitution map: for each method-level template parameter,
    // try to infer its value from the property name being accessed.
    let mut method_subs = std::collections::HashMap::new();
    for tparam in &get_method.template_params {
        // The template param is typically bounded by key-of<SomeShape>.
        // After class-level substitution the bound is already concrete
        // (e.g. key-of<array{a: int, b: string}> → 'a'|'b').
        // We infer the template value as a literal string matching the
        // property name.
        method_subs.insert(tparam.to_string(), PhpType::literal_string_value(prop_name));
    }

    let resolved = return_type.substitute(&method_subs);

    // Only return if the substitution actually resolved to something
    // concrete (not still an IndexAccess with an unresolved key).
    if matches!(&resolved.kind(), TypeKind::IndexAccess(_, _)) {
        return None;
    }

    Some(resolved)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "inheritance_tests.rs"]
mod tests;
