use crate::atom::{Atom, AtomSet, atom};
use std::collections::HashMap;
/// Base class inheritance resolution.
///
/// This module handles merging members from parent classes and traits
/// into a single `ClassInfo`.  The resulting merged class contains the
/// base set of members visible on an instance / static access,
/// respecting PHP's precedence rules:
///
///   class own > traits > parent chain
///
/// `@mixin` members are handled separately by
/// [`PHPDocProvider`](crate::virtual_members::phpdoc::PHPDocProvider) in
/// the virtual member provider layer.
///
/// This module also supports **generic type substitution**: when a child
/// class declares `@extends Parent<ConcreteType1, ConcreteType2>` and the
/// parent has `@template T1` / `@template T2`, the inherited methods and
/// properties have their template parameter references replaced with the
/// concrete types.
use std::sync::Arc;

#[cfg(test)]
use std::borrow::Cow;

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
    pub precedences: &'a [TraitPrecedence],
    /// `as` alias declarations.
    pub aliases: &'a [TraitAlias],
}

/// Tracks member names already present during inheritance merging.
///
/// Passed through `resolve_class_with_inheritance` and `merge_traits_into`
/// (including recursive calls) so that every addition is checked in O(1)
/// instead of scanning the full member vectors.
pub(crate) struct MergeDedup {
    /// Method names already merged.
    pub methods: AtomSet,
    /// Property names already merged.
    pub properties: AtomSet,
    /// Constant names already merged.
    pub constants: AtomSet,
}

/// Reserve the names of `@method` tags declared in `docblock` into the
/// method dedup set.
///
/// A `@method` tag declares a method on the class that carries it.  That
/// declaration overrides any method of the same name inherited from a
/// superclass, exactly like a real overriding method would.  The virtual
/// members themselves are synthesized later by the PHPDoc provider; this
/// only stakes the claim so the inheritance walk stops merging the inherited
/// real method over the `@method` declaration.
fn reserve_method_tag_names(docblock: Option<&str>, dedup: &mut MergeDedup) {
    let Some(doc) = docblock else {
        return;
    };
    if !doc.contains("@method") {
        return;
    }
    for m in crate::docblock::extract_method_tags(doc) {
        dedup.methods.insert(m.name);
    }
}

impl MergeDedup {
    /// Build from the members already present on a `ClassInfo`.
    fn from_class(class: &ClassInfo) -> Self {
        Self {
            methods: class.methods.iter().map(|m| m.name).collect(),
            properties: class.properties.iter().map(|p| p.name).collect(),
            constants: class.constants.iter().map(|c| c.name).collect(),
        }
    }
}

use crate::php_type::PhpType;
use crate::types::{
    ClassInfo, MAX_INHERITANCE_DEPTH, MAX_TRAIT_DEPTH, MethodInfo, ParameterInfo, PropertyInfo,
    TraitAlias, TraitPrecedence, Visibility,
};
use crate::util::short_name;
use crate::virtual_members::laravel::{
    extends_eloquent_model, factory_to_model_fqn, model_to_factory_fqn,
};

// ─── Docblock Enrichment ────────────────────────────────────────────────────

/// Whether a child's effective type equals its native type, meaning no
/// docblock override was applied.
///
/// Returns `true` when the child wrote no `@return` / `@var` / `@param`
/// tag (so the effective type is just the native hint).  Returns `false`
/// when the child provided its own docblock type — in that case the
/// child's type is an intentional override and should not be replaced.
fn lacks_docblock_override(effective: &Option<PhpType>, native: &Option<PhpType>) -> bool {
    match (effective, native) {
        // No effective type at all — nothing to override.
        (None, _) => true,
        // Effective type present but no native type — the child wrote
        // a docblock-only type (e.g. `@return list<Pen>` with no native
        // hint).  That is an intentional override.
        (Some(_), None) => false,
        // Both present — if they are equivalent, the child didn't write
        // a docblock (the effective type is just the native hint echoed).
        (Some(eff), Some(nat)) => eff.equivalent(nat),
    }
}

/// Whether an ancestor's type is richer than the child's native type.
///
/// Returns `true` when the ancestor has an effective type that differs
/// from its own native type (meaning the ancestor wrote a docblock).
fn ancestor_has_richer_type(effective: &Option<PhpType>, native: &Option<PhpType>) -> bool {
    match (effective, native) {
        // Ancestor has an effective type but no native type — it came
        // from a docblock (e.g. interface method with `@return list<Pen>`
        // and no native hint).
        (Some(_), None) => true,
        // Both present — richer if they differ (docblock overrides native).
        (Some(eff), Some(nat)) => !eff.equivalent(nat),
        // No effective type — nothing richer to offer.
        _ => false,
    }
}

/// Enrich a child method with docblock information from an ancestor method.
///
/// Propagates return types, parameter types, descriptions, template
/// parameters, conditional return types, and type assertions from the
/// ancestor when the child lacks its own docblock overrides.
///
/// **Return type rule:** If the child's `return_type` equals its
/// `native_return_type` (no docblock), and the ancestor's `return_type`
/// differs from its `native_return_type` (has docblock), copy the
/// ancestor's `return_type` to the child.  If the child has no
/// `return_type` at all, always inherit the ancestor's.
///
/// **Parameter rule:** Match by position (not by name, since the child
/// may rename parameters).  Same effective-vs-native comparison as
/// return types.
///
/// **Description rule:** Inherit `description` and `return_description`
/// when the child has `None`.
pub(crate) fn enrich_method_from_ancestor(existing: &mut MethodInfo, ancestor: &MethodInfo) {
    // ── Return type ─────────────────────────────────────────────
    // Propagate when (a) the child has no return type at all, or
    // (b) the child's effective type equals its native type (no
    // docblock override) and the ancestor has a richer docblock type.
    if existing.return_type.is_none() && ancestor.return_type.is_some()
        || lacks_docblock_override(&existing.return_type, &existing.native_return_type)
            && ancestor_has_richer_type(&ancestor.return_type, &ancestor.native_return_type)
    {
        existing.return_type = ancestor.return_type.clone();
    }

    // ── Template parameters ─────────────────────────────────────
    if existing.template_params.is_empty() && !ancestor.template_params.is_empty() {
        existing.template_params = ancestor.template_params.clone();
        existing.template_param_bounds = ancestor.template_param_bounds.clone();
        existing.template_bindings = ancestor.template_bindings.clone();
        // Template return types like `T` only make sense when the
        // template params are present — inherit the return type too
        // if we haven't already set it.
        if existing.return_type.is_none() {
            existing.return_type = ancestor.return_type.clone();
        }
    }

    // ── Conditional return type ─────────────────────────────────
    if existing.conditional_return.is_none() && ancestor.conditional_return.is_some() {
        existing.conditional_return = ancestor.conditional_return.clone();
    }

    // ── Type assertions ─────────────────────────────────────────
    if existing.type_assertions.is_empty() && !ancestor.type_assertions.is_empty() {
        existing.type_assertions = ancestor.type_assertions.clone();
    }

    // ── Parameters ──────────────────────────────────────────────
    // For constructors, use **name-based** matching instead of
    // positional.  PHP constructors don't follow Liskov substitution
    // — a child constructor can have a completely different signature
    // (different parameter count, order, types).  Positional
    // enrichment would incorrectly map ancestor param types onto
    // unrelated child params (e.g. Exception's `$code` type `int`
    // onto a child's `$message` param at position 1).
    //
    // This follows PHPStan's `PhpDocInheritanceResolver`: for
    // `__construct` the positional parameter name list falls back to
    // the child's own names, so only same-named parameters inherit.
    if existing.name == "__construct" {
        enrich_constructor_parameters_by_name(&mut existing.parameters, &ancestor.parameters);
    } else {
        enrich_parameters_from_ancestor(&mut existing.parameters, &ancestor.parameters);
    }

    // ── Descriptions ────────────────────────────────────────────
    if existing.description.is_none() && ancestor.description.is_some() {
        existing.description = ancestor.description.clone();
    }
    if existing.return_description.is_none() && ancestor.return_description.is_some() {
        existing.return_description = ancestor.return_description.clone();
    }
}

/// Enrich child parameters from ancestor parameters, matched by position.
///
/// When a child parameter's `type_hint` equals its `native_type_hint`
/// (no docblock override) and the ancestor parameter has a richer type,
/// copy the ancestor's `type_hint`.  Also inherit `description` when
/// the child parameter has none.
fn enrich_parameters_from_ancestor(
    existing_params: &mut [ParameterInfo],
    ancestor_params: &[ParameterInfo],
) {
    for (existing_param, ancestor_param) in existing_params.iter_mut().zip(ancestor_params) {
        enrich_single_parameter(existing_param, ancestor_param);
    }
}

/// Enrich constructor parameters from ancestor parameters, matched by name.
///
/// Unlike regular methods (which follow Liskov substitution and can
/// safely use positional matching), constructors can have completely
/// different signatures.  Only parameters with the **same name** in
/// both the child and ancestor are enriched.
fn enrich_constructor_parameters_by_name(
    existing_params: &mut [ParameterInfo],
    ancestor_params: &[ParameterInfo],
) {
    for existing_param in existing_params.iter_mut() {
        if let Some(ancestor_param) = ancestor_params
            .iter()
            .find(|ap| ap.name == existing_param.name)
        {
            enrich_single_parameter(existing_param, ancestor_param);
        }
    }
}

/// Enrich a single child parameter from an ancestor parameter.
///
/// Copies the ancestor's `type_hint` when the child lacks a docblock
/// override, the ancestor has a richer type, **and** the child's
/// native type is not a specific concrete type.
///
/// PHP allows contravariant parameter types: a concrete class may
/// declare `?int` where the interface says `int`, or Carbon's
/// `setTimezone(DateTimeZone|string|int)` may widen DateTime's
/// `setTimezone(DateTimeZone)`.  In those cases the child's native
/// type is an intentional widening and must not be narrowed.
///
/// However, when the child's native type is a placeholder like
/// `object` or `mixed` (common in `@implements`/`@extends` generic
/// patterns where the interface declares `object $entity` and the
/// `@implements` tag substitutes the template to a concrete type),
/// the ancestor's enriched type should flow through.
fn enrich_single_parameter(existing_param: &mut ParameterInfo, ancestor_param: &ParameterInfo) {
    // Type hint enrichment — the child must lack a docblock override
    // AND the ancestor must have a richer type (docblock that goes
    // beyond its native hint).  Additionally, skip enrichment when
    // the child has a specific native type (not `object`/`mixed`)
    // because the child's declaration is intentional and may be
    // wider than the ancestor's (contravariant parameters).
    let child_has_specific_native = existing_param.native_type_hint.as_ref().is_some_and(|nt| {
        !nt.is_object() && !nt.is_mixed() && !nt.is_array_like() && !nt.is_iterable()
    });
    if !child_has_specific_native
        && lacks_docblock_override(&existing_param.type_hint, &existing_param.native_type_hint)
        && ancestor_has_richer_type(&ancestor_param.type_hint, &ancestor_param.native_type_hint)
    {
        existing_param.type_hint = ancestor_param.type_hint.clone();
    }
    // Description enrichment
    if existing_param.description.is_none() && ancestor_param.description.is_some() {
        existing_param.description = ancestor_param.description.clone();
    }
}

/// Enrich a child property with docblock information from an ancestor
/// property.
///
/// Propagates type hints and descriptions from the ancestor when the
/// child lacks its own docblock overrides.  The same
/// effective-vs-native comparison is used as for method return types.
pub(crate) fn enrich_property_from_ancestor(existing: &mut PropertyInfo, ancestor: &PropertyInfo) {
    // ── Type hint ───────────────────────────────────────────────
    // Same logic as method return types: propagate when the child
    // has no type or has only the native hint without a docblock
    // override, and the ancestor provides a richer type.
    if existing.type_hint.is_none() && ancestor.type_hint.is_some()
        || lacks_docblock_override(&existing.type_hint, &existing.native_type_hint)
            && ancestor_has_richer_type(&ancestor.type_hint, &ancestor.native_type_hint)
    {
        existing.type_hint = ancestor.type_hint.clone();
    }

    // ── Description ─────────────────────────────────────────────
    if existing.description.is_none() && ancestor.description.is_some() {
        existing.description = ancestor.description.clone();
    }
}

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

    // Stake a claim on the class's own `@method` tag names before merging
    // any inherited members.  A `@method` declaration overrides a method of
    // the same name inherited from a superclass, exactly like a real
    // overriding method would (the virtual members themselves are
    // synthesized later by the PHPDoc provider — here we only prevent the
    // inheritance walk from merging the inherited real method over them).
    reserve_method_tag_names(class.class_docblock.as_deref(), &mut dedup);

    // 1. Merge traits used by this class.
    //    PHP precedence: class methods > trait methods > inherited methods.
    //    Since `merged` already contains the class's own members, we only
    //    add trait members that don't collide with existing ones.
    merge_traits_into(
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

        let parent = if let Some(p) = class_loader(parent_name) {
            p
        } else {
            break;
        };

        // Stake a claim on this ancestor's `@method` tag names at its depth
        // in the hierarchy, so that a real method of the same name inherited
        // from a *farther* ancestor does not shadow the `@method`
        // declaration.  Reserved before the ancestor's own members are
        // merged so a real method on this same ancestor still wins over its
        // own `@method` tag.
        reserve_method_tag_names(parent.class_docblock.as_deref(), &mut dedup);

        // Build the substitution map for this parent level.
        //
        // Look through current's `extends_generics` for an entry
        // whose class name matches this parent, and zip its type
        // arguments with the parent's `template_params`.
        let mut level_subs = build_substitution_map(&current, &parent, &active_subs);

        // ── Convention-based Factory fallback ────────────────────
        // When a factory class extends `Factory` without
        // `@extends Factory<Model>`, derive the model class from
        // the naming convention (e.g. `Database\Factories\UserFactory`
        // → `App\Models\User`) and substitute `TModel` automatically.
        if level_subs.is_empty()
            && !parent.template_params.is_empty()
            && is_factory_class(parent_name)
        {
            let factory_fqn = current.fqn();
            if let Some(model_fqn) = factory_to_model_fqn(&factory_fqn)
                && class_loader(&model_fqn).is_some()
            {
                for param in &parent.template_params {
                    level_subs.insert(param.to_string(), PhpType::Named(model_fqn.clone()));
                }
            }
        }

        // ── Template bound fallback ─────────────────────────────
        // When a subclass extends a generic parent without providing
        // explicit `@extends` generics and no convention-based
        // substitution filled the map, fall back to the template
        // parameter bounds (e.g. `@template T of object` → `object`)
        // so that inherited methods don't leak raw template names.
        if !parent.template_params.is_empty() {
            for param_name in &parent.template_params {
                if !level_subs.contains_key(param_name.to_string().as_str()) {
                    let bound = parent
                        .template_param_bounds
                        .get(param_name)
                        .cloned()
                        .unwrap_or_else(PhpType::mixed);
                    level_subs.insert(param_name.to_string(), bound);
                }
            }
        }

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

        merge_traits_into(
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

        // Merge parent methods — skip private.
        // When the child already has a method with the same name,
        // enrich it with the parent's richer docblock types instead
        // of silently discarding the parent's type information.
        for method in &parent.methods {
            if method.visibility == Visibility::Private {
                continue;
            }
            if !dedup.methods.insert(method.name) {
                // Child already has this method — enrich it from parent.
                let mut ancestor_method = (**method).clone();
                if !level_subs.is_empty() {
                    apply_substitution_to_method(&mut ancestor_method, &level_subs);
                }
                if let Some(existing) = merged
                    .methods
                    .make_mut()
                    .iter_mut()
                    .find(|m| m.name == method.name)
                {
                    enrich_method_from_ancestor(Arc::make_mut(existing), &ancestor_method);
                }
                continue;
            }
            if level_subs.is_empty() {
                // Replace bare `self` in return type with the declaring
                // (parent) class name so that `self` resolves to the class
                // that defines the method, not the inheriting child.
                if method
                    .return_type
                    .as_ref()
                    .is_some_and(|r| r.contains_bare_self())
                {
                    let mut m = (**method).clone();
                    if let Some(ref mut rt) = m.return_type {
                        *rt = rt.replace_bare_self(&parent.fqn());
                    }
                    merged.methods.push(Arc::new(m));
                } else {
                    merged.methods.push(Arc::clone(method));
                }
            } else {
                let mut ancestor_method = (**method).clone();
                apply_substitution_to_method(&mut ancestor_method, &level_subs);
                // Replace bare `self` after substitution.
                if let Some(ref mut rt) = ancestor_method.return_type
                    && rt.contains_bare_self()
                {
                    *rt = rt.replace_bare_self(&parent.fqn());
                }
                merged.methods.push(Arc::new(ancestor_method));
            }
        }

        // Merge parent properties — same enrichment logic.
        for property in &parent.properties {
            if property.visibility == Visibility::Private {
                continue;
            }
            let mut ancestor_property = property.clone();
            if !level_subs.is_empty() {
                apply_substitution_to_property(&mut ancestor_property, &level_subs);
            }
            if !dedup.properties.insert(property.name) {
                // Child already has this property — enrich it from parent.
                if let Some(existing) = merged
                    .properties
                    .make_mut()
                    .iter_mut()
                    .find(|p| p.name == property.name)
                {
                    enrich_property_from_ancestor(existing, &ancestor_property);
                }
                continue;
            }
            merged.properties.push(ancestor_property);
        }

        // Merge parent constants
        for constant in &parent.constants {
            if constant.visibility == Visibility::Private {
                continue;
            }
            if !dedup.constants.insert(constant.name) {
                continue;
            }
            merged.constants.push(constant.clone());
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
    for iface_name in &class.interfaces {
        let Some(iface) = class_loader(iface_name) else {
            continue;
        };

        // Build substitution map from @implements/@template-implements generics.
        let iface_subs =
            build_substitution_map(&ClassRef::Borrowed(class), &iface, &HashMap::new());

        for method in &iface.methods {
            // Only enrich methods that the class already has (i.e. overrides).
            if let Some(existing) = merged
                .methods
                .make_mut()
                .iter_mut()
                .find(|m| m.name == method.name)
            {
                let mut ancestor_method = (**method).clone();
                if !iface_subs.is_empty() {
                    apply_substitution_to_method(&mut ancestor_method, &iface_subs);
                }
                enrich_method_from_ancestor(Arc::make_mut(existing), &ancestor_method);
            }
        }
    }

    // Refine the `value` property on backed enums.  The `BackedEnum`
    // interface declares `public readonly int|string $value`, but each
    // concrete backed enum knows its specific backing type.  Replace
    // the generic union with the precise type so that hover, completion,
    // and diagnostics see `string` or `int` instead of `int|string`.
    if let Some(ref backed) = merged.backed_type {
        let specific_type = match backed {
            crate::types::BackedEnumType::String => PhpType::Named("string".to_string()),
            crate::types::BackedEnumType::Int => PhpType::Named("int".to_string()),
        };
        if let Some(prop) = merged
            .properties
            .make_mut()
            .iter_mut()
            .find(|p| p.name == "value")
        {
            prop.type_hint = Some(specific_type);
        }
    }

    merged
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
    if let Some(m) = class.get_method(method_name) {
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

/// Try to resolve a property access through a `__get` magic method that
/// uses method-level `@template` with `key-of` bounds and `T[K]` return.
///
/// For example, given:
/// ```php
/// /** @template TData as array */
/// abstract class DataBag {
///     /** @template K as key-of<TData> @return TData[K] */
///     public function __get(string $property) { ... }
/// }
/// /** @extends DataBag<array{a: int, b: string}> */
/// class FooBag extends DataBag {}
/// ```
/// After class-level substitution, `__get` on the merged `FooBag` has
/// return type `array{a: int, b: string}[K]`.  This function infers
/// `K = 'a'` from the property name and evaluates to `int`.
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
        method_subs.insert(tparam.to_string(), PhpType::Literal(prop_name.to_string()));
    }

    let resolved = return_type.substitute(&method_subs);

    // Only return if the substitution actually resolved to something
    // concrete (not still an IndexAccess with an unresolved key).
    if matches!(&resolved, PhpType::IndexAccess(_, _)) {
        return None;
    }

    Some(resolved)
}

/// Recursively merge members from the given traits into `merged`.
///
/// Traits can themselves `use` other traits (composition), so this
/// function recurses up to `MAX_TRAIT_DEPTH` levels.  Members that
/// already exist in `merged` (by name) are skipped — this naturally
/// implements the PHP precedence rule where the current class's own
/// members win over trait members, and earlier-listed traits win
/// over later ones.
///
/// Private trait members *are* merged (unlike parent class private
/// members), because PHP copies trait members into the using class
/// regardless of visibility.
///
/// When `use_generics` contains an entry for a trait (e.g.
/// `@use SomeTrait<ConcreteType>`) and the trait declares
/// `@template T`, the inherited methods and properties have their
/// template parameter types replaced with the concrete types.
fn merge_traits_into(
    merged: &mut ClassInfo,
    trait_names: &[Atom],
    ctx: &TraitContext<'_>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    depth: u32,
    dedup: &mut MergeDedup,
    self_class_name: &str,
) {
    if depth > MAX_TRAIT_DEPTH {
        return;
    }

    for trait_name in trait_names {
        let trait_info = if let Some(t) = class_loader(trait_name) {
            t
        } else {
            continue;
        };

        // Build a substitution map for this trait if the using class
        // declared `@use TraitName<Type1, Type2>` and the trait has
        // `@template` parameters.
        let mut trait_subs =
            build_trait_substitution_map(trait_name, &trait_info, ctx.use_generics);

        // ── Convention-based HasFactory fallback ─────────────────
        // When a model uses `HasFactory` without `@use HasFactory<X>`,
        // derive the factory class from the naming convention
        // (e.g. `App\Models\User` → `Database\Factories\UserFactory`)
        // and substitute `TFactory` automatically.
        if trait_subs.is_empty()
            && !trait_info.template_params.is_empty()
            && is_has_factory_trait(trait_name)
            && extends_eloquent_model(merged, class_loader)
        {
            let model_fqn = merged.fqn();
            let factory_fqn = model_to_factory_fqn(&model_fqn);
            if class_loader(&factory_fqn).is_some() {
                for param in &trait_info.template_params {
                    trait_subs.insert(param.to_string(), PhpType::Named(factory_fqn.clone()));
                }
            }
        }

        // ── Template bound fallback ─────────────────────────────
        // When a class uses a generic trait without `@use` generics
        // and no convention-based provider filled the map, fall back
        // to the template parameter bounds (e.g. `@template T of object`
        // → `object`) so inherited methods don't leak raw template names.
        if !trait_info.template_params.is_empty() {
            for param_name in &trait_info.template_params {
                if !trait_subs.contains_key(param_name.to_string().as_str()) {
                    let bound = trait_info
                        .template_param_bounds
                        .get(param_name)
                        .cloned()
                        .unwrap_or_else(PhpType::mixed);
                    trait_subs.insert(param_name.to_string(), bound);
                }
            }
        }

        // Recursively merge traits used by this trait (trait composition).
        // The sub-trait's own `@use` generics (from the trait's docblock)
        // apply, not the outer class's.
        if !trait_info.used_traits.is_empty() {
            merge_traits_into(
                merged,
                &trait_info.used_traits,
                &TraitContext {
                    use_generics: &trait_info.use_generics,
                    precedences: &trait_info.trait_precedences,
                    aliases: &trait_info.trait_aliases,
                },
                class_loader,
                depth + 1,
                dedup,
                self_class_name,
            );
        }

        // Walk the `parent_class` (extends) chain so that interface
        // inheritance is resolved.  For example, `BackedEnum extends
        // UnitEnum` — loading `BackedEnum` alone would miss `UnitEnum`'s
        // members (`cases()`, `$name`) unless we follow the chain here.
        // The same depth counter is shared to prevent infinite loops.
        let mut current = trait_info.clone();
        let mut parent_depth = depth;
        while let Some(ref parent_name) = current.parent_class {
            parent_depth += 1;
            if parent_depth > MAX_TRAIT_DEPTH {
                break;
            }
            let parent = if let Some(p) = class_loader(parent_name) {
                p
            } else {
                break;
            };

            // Also follow the parent's own used_traits.
            if !parent.used_traits.is_empty() {
                merge_traits_into(
                    merged,
                    &parent.used_traits,
                    &TraitContext {
                        use_generics: &parent.use_generics,
                        precedences: &parent.trait_precedences,
                        aliases: &parent.trait_aliases,
                    },
                    class_loader,
                    parent_depth + 1,
                    dedup,
                    self_class_name,
                );
            }

            // Merge parent methods (skip private, skip duplicates)
            for method in &parent.methods {
                if method.visibility == Visibility::Private {
                    continue;
                }
                if !dedup.methods.insert(method.name) {
                    continue;
                }
                merged.methods.push(Arc::clone(method));
            }

            // Merge parent properties
            for property in &parent.properties {
                if property.visibility == Visibility::Private {
                    continue;
                }
                if !dedup.properties.insert(property.name) {
                    continue;
                }
                merged.properties.push(property.clone());
            }

            // Merge parent constants
            for constant in &parent.constants {
                if constant.visibility == Visibility::Private {
                    continue;
                }
                if !dedup.constants.insert(constant.name) {
                    continue;
                }
                merged.constants.push(constant.clone());
            }

            current = parent;
        }

        // Merge trait methods — skip if already present.
        // Apply generic substitution if a `@use` mapping exists.
        // Also skip methods excluded by `insteadof` declarations.
        for method in &trait_info.methods {
            // Check if this method from this trait is excluded by an
            // `insteadof` declaration.  For example, if the class has
            // `TraitA::method insteadof TraitB`, then when merging
            // TraitB's methods, `method` should be skipped.
            let excluded = ctx.precedences.iter().any(|p| {
                p.method_name == method.name
                    && p.insteadof
                        .iter()
                        .any(|excluded_trait| excluded_trait == trait_name)
            });
            if excluded {
                continue;
            }

            if !dedup.methods.insert(method.name) {
                continue;
            }
            let mut method = (**method).clone();

            // Apply visibility-only `as` changes (no alias name).
            // For example, `TraitA::method as protected` changes the
            // visibility of `method` without creating an alias.
            for alias in ctx.aliases {
                if alias.method_name == method.name
                    && alias.alias.is_none()
                    && let Some(vis) = alias.visibility
                {
                    // Check trait name matches (if specified)
                    let name_matches = alias.trait_name.as_ref().is_none_or(|t| t == trait_name);
                    if name_matches {
                        method.visibility = vis;
                    }
                }
            }

            if !trait_subs.is_empty() {
                apply_substitution_to_method(&mut method, &trait_subs);
            }
            // Replace bare `self` with the using class name so that
            // `self` resolves to the class that imports the trait.
            if let Some(ref mut rt) = method.return_type
                && rt.contains_bare_self()
            {
                *rt = rt.replace_bare_self(self_class_name);
            }
            merged.methods.push(Arc::new(method));
        }

        // Merge trait properties — apply substitution.
        for property in &trait_info.properties {
            if !dedup.properties.insert(property.name) {
                continue;
            }
            let mut property = property.clone();
            if !trait_subs.is_empty() {
                apply_substitution_to_property(&mut property, &trait_subs);
            }
            merged.properties.push(property);
        }

        // Merge trait constants
        for constant in &trait_info.constants {
            if !dedup.constants.insert(constant.name) {
                continue;
            }
            merged.constants.push(constant.clone());
        }

        // Apply `as` alias declarations that create new method names.
        // For example, `TraitB::method as traitBMethod` creates a copy
        // of `method` accessible as `traitBMethod`.
        for alias in ctx.aliases {
            // Only process aliases that have a new name.
            let alias_name = match &alias.alias {
                Some(name) => name,
                None => continue,
            };

            // Check trait name matches (if specified).
            let name_matches = alias.trait_name.as_ref().is_none_or(|t| t == trait_name);
            if !name_matches {
                continue;
            }

            // Find the source method in this trait.
            let source_method = trait_info
                .methods
                .iter()
                .find(|m| m.name == alias.method_name);
            let source_method = match source_method {
                Some(m) => m,
                None => continue,
            };

            // Skip if an alias with this name already exists.
            let alias_atom = atom(alias_name);
            if !dedup.methods.insert(alias_atom) {
                continue;
            }

            let mut aliased = (**source_method).clone();
            aliased.name = alias_atom;
            if let Some(vis) = alias.visibility {
                aliased.visibility = vis;
            }
            if !trait_subs.is_empty() {
                apply_substitution_to_method(&mut aliased, &trait_subs);
            }
            merged.methods.push(Arc::new(aliased));
        }
    }
}

// ─── Generic Type Substitution ──────────────────────────────────────────────

/// Check whether a trait name is the Laravel `HasFactory` trait.
///
/// Matches the FQN `Illuminate\Database\Eloquent\Factories\HasFactory`
/// as well as the short name `HasFactory` (common in same-file tests).
fn is_has_factory_trait(trait_name: &str) -> bool {
    trait_name == "Illuminate\\Database\\Eloquent\\Factories\\HasFactory"
        || trait_name == "HasFactory"
}

/// Check whether a parent class name is the Laravel
/// `Illuminate\Database\Eloquent\Factories\Factory` base class.
fn is_factory_class(class_name: &str) -> bool {
    class_name == "Illuminate\\Database\\Eloquent\\Factories\\Factory" || class_name == "Factory"
}

/// Build a substitution map for a trait based on `@use` generics and the
/// trait's `@template` parameters.
///
/// If the using class declares `@use HasFactory<UserFactory>` and the
/// trait `HasFactory` has `@template TFactory`, the returned map is
/// `{TFactory => UserFactory}`.
fn build_trait_substitution_map(
    trait_name: &str,
    trait_info: &ClassInfo,
    use_generics: &[(Atom, Vec<PhpType>)],
) -> HashMap<String, PhpType> {
    if trait_info.template_params.is_empty() || use_generics.is_empty() {
        return HashMap::new();
    }

    let trait_short = short_name(trait_name);

    // Find the @use entry that matches this trait.
    let type_args = use_generics
        .iter()
        .find(|(name, _)| {
            let name_short = short_name(name);
            name_short == trait_short
        })
        .map(|(_, args)| args);

    let type_args = match type_args {
        Some(args) => args,
        None => return HashMap::new(),
    };

    let mut map = HashMap::new();
    // Right-align a short argument list to the trailing template params,
    // matching PHPStan/Psalm convention for `@use Collection<User>`.
    let offset = right_align_offset(
        &trait_info.template_params,
        &trait_info.template_param_bounds,
        type_args.len(),
    );
    for (i, param_name) in trait_info.template_params.iter().enumerate() {
        if i < offset {
            let fallback = trait_info
                .template_param_bounds
                .get(param_name)
                .cloned()
                .unwrap_or_else(PhpType::mixed);
            map.insert(param_name.to_string(), fallback);
            continue;
        }
        if let Some(arg) = type_args.get(i - offset) {
            map.insert(param_name.to_string(), arg.clone());
        }
    }
    map
}

/// Build a substitution map for a parent class based on the child's
/// `@extends` generics and the parent's `@template` parameters.
///
/// If the child declares `@extends Collection<int, Language>` and the
/// parent `Collection` has `@template TKey` and `@template TValue`,
/// the returned map is `{TKey => int, TValue => Language}`.
///
/// When `active_subs` is non-empty (from a higher-level ancestor), the
/// type arguments are first resolved through those substitutions.  This
/// handles chained generics like:
///
/// ```text
/// class A { @template U }
/// class B extends A { @template T, @extends A<T> }
/// class C extends B { @extends B<Foo> }
/// ```
///
/// When resolving `C`: at level 1 (B), `active_subs` is empty and we
/// build `{T => Foo}`.  At level 2 (A), `current` is B whose
/// `@extends A<T>` gets the active substitution `{T => Foo}` applied,
/// yielding `{U => Foo}`.
fn build_substitution_map(
    current: &ClassInfo,
    parent: &ClassInfo,
    active_subs: &HashMap<String, PhpType>,
) -> HashMap<String, PhpType> {
    if parent.template_params.is_empty() {
        return active_subs.clone();
    }

    let parent_short = short_name(&parent.name);

    // Search `current.extends_generics` for an entry matching this parent.
    // Also check `implements_generics` for interface inheritance.
    let type_args = current
        .extends_generics
        .iter()
        .chain(current.implements_generics.iter())
        .find(|(name, _)| {
            let name_short = short_name(name);
            name_short == parent_short
        })
        .map(|(_, args)| args);

    let type_args = match type_args {
        Some(args) => args,
        None => {
            // No @extends/@implements generics for this parent.
            // Carry forward any active substitutions — they may still
            // apply if the parent's methods reference template params
            // from a grandchild.
            return active_subs.clone();
        }
    };

    let mut map = HashMap::new();

    // Right-align a short argument list to the trailing template params,
    // matching `build_generic_subs` and PHPStan/Psalm convention so that
    // `@extends Collection<User>` binds `User` to the value parameter.
    let offset = right_align_offset(
        &parent.template_params,
        &parent.template_param_bounds,
        type_args.len(),
    );

    for (i, param_name) in parent.template_params.iter().enumerate() {
        if i < offset {
            // Skipped leading (key-like) param: fall back to its declared
            // bound or `mixed` so the raw template name never leaks.
            let fallback = parent
                .template_param_bounds
                .get(param_name)
                .cloned()
                .unwrap_or_else(PhpType::mixed);
            map.insert(param_name.to_string(), fallback);
            continue;
        }
        if let Some(arg) = type_args.get(i - offset) {
            // Apply any active substitutions to the type argument.
            // This handles chaining: if arg is "T" and active_subs has
            // {T => Foo}, the result is {param_name => Foo}.
            let resolved = if active_subs.is_empty() {
                arg.clone()
            } else {
                arg.substitute(active_subs)
            };
            map.insert(param_name.to_string(), resolved);
        }
    }

    map
}

/// Apply generic type substitution to a method's return type and parameter
/// type hints.
pub(crate) fn apply_substitution_to_method(
    method: &mut MethodInfo,
    subs: &HashMap<String, PhpType>,
) {
    if let Some(ref mut ret) = method.return_type {
        *ret = ret.substitute(subs);
    }
    if let Some(ref mut cond) = method.conditional_return {
        apply_substitution_to_conditional(cond, subs);
    }
    for param in &mut method.parameters {
        if let Some(ref mut hint) = param.type_hint {
            *hint = hint.substitute(subs);
        }
    }
}

/// Apply generic type substitution to a conditional return type tree.
///
/// Delegates to [`PhpType::substitute`] which recursively walks all
/// type variants (including nested conditionals) and replaces template
/// parameter names with their concrete types.
pub(crate) fn apply_substitution_to_conditional(
    cond: &mut PhpType,
    subs: &HashMap<String, PhpType>,
) {
    *cond = cond.substitute(subs);
}

/// Apply generic type substitution to a property's type hint.
pub(crate) fn apply_substitution_to_property(
    property: &mut PropertyInfo,
    subs: &HashMap<String, PhpType>,
) {
    if let Some(ref mut hint) = property.type_hint {
        *hint = hint.substitute(subs);
    }
}

/// Apply a substitution map to a type string.
///
/// Handles:
///   - Direct match: `"TValue"` → `"Language"`
///   - Nullable: `"?TValue"` → `"?Language"`
///   - Union types: `"TValue|null"` → `"Language|null"`
///   - Intersection types: `"TValue&Countable"` → `"Language&Countable"`
///   - Generic params: `"array<TKey, TValue>"` → `"array<int, Language>"`
///   - Nested generics: `"Collection<TKey, list<TValue>>"` →
///     `"Collection<int, list<Language>>"`
///   - Combinations: `"?Collection<TKey, TValue>|null"` → resolved correctly
///
/// Internally delegates to [`PhpType::substitute`] which walks the
/// parsed type tree.  This wrapper preserves the `&str → Cow<str>` API
/// for test assertions that compare type strings before and after
/// substitution.
#[cfg(test)]
pub(crate) fn apply_substitution<'a>(
    type_str: &'a str,
    subs: &HashMap<String, PhpType>,
) -> Cow<'a, str> {
    let s = type_str.trim();
    if s.is_empty() || subs.is_empty() {
        return Cow::Borrowed(s);
    }

    // ── Early exit: if the type string doesn't contain any of the
    // substitution keys as a substring, no replacement can happen.
    // This skips the vast majority of type strings that don't reference
    // template parameters, avoiding all allocation and recursion.
    if !subs.keys().any(|key| s.contains(key.as_str())) {
        return Cow::Borrowed(s);
    }

    let parsed = PhpType::parse(s);
    let substituted = parsed.substitute(subs);
    let result = substituted.to_string();

    // If the result is identical to the input, return borrowed to
    // avoid unnecessary allocation in callers that check for changes.
    if result == s {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(result)
    }
}

/// Build a substitution map from a class's template parameters and
/// concrete type arguments.
///
/// Handles right-alignment when fewer arguments than template parameters
/// are provided (see [`apply_generic_args`] for details on the heuristic).
///
/// Returns an empty map when no substitutions can be made (e.g. when
/// `template_params` or `type_args` is empty).
pub(crate) fn build_generic_subs(
    class: &ClassInfo,
    type_args: &[PhpType],
) -> HashMap<String, PhpType> {
    if class.template_params.is_empty() || type_args.is_empty() {
        return HashMap::new();
    }

    // When fewer type arguments are provided than template parameters,
    // right-align the args so that trailing (value) params get bound
    // and leading key-like params stay unbound.  This handles the
    // common PHP pattern of writing `Collection<Model>` instead of
    // `Collection<int, Model>` — the single arg should bind to
    // `TValue`/`TModel`, not `TKey`.
    //
    // The heuristic only activates when every skipped leading param
    // has an `array-key` (or `int` / `string`) bound, which is the
    // universal convention for collection key parameters.
    let offset = right_align_offset(
        &class.template_params,
        &class.template_param_bounds,
        type_args.len(),
    );

    let mut subs = HashMap::new();
    for (i, param_name) in class.template_params.iter().enumerate() {
        if i < offset {
            // Skipped (right-aligned) params: fall back to their
            // declared upper bound or `mixed` so the raw template
            // name never leaks into downstream consumers.
            let fallback = class
                .template_param_bounds
                .get(param_name)
                .cloned()
                .unwrap_or_else(PhpType::mixed);
            subs.insert(param_name.to_string(), fallback);
            continue;
        }
        if let Some(arg) = type_args.get(i - offset) {
            subs.insert(param_name.to_string(), arg.clone());
        } else {
            // Unbound param (more template params than type args and
            // right-alignment didn't apply): use upper bound or `mixed`.
            let fallback = class
                .template_param_bounds
                .get(param_name)
                .cloned()
                .unwrap_or_else(PhpType::mixed);
            subs.insert(param_name.to_string(), fallback);
        }
    }

    subs
}

/// Build default type arguments for a class whose template parameters
/// have no concrete bindings (e.g. `new Collection()` without a generic
/// annotation).
///
/// Each template parameter is mapped to its declared upper bound
/// (`@template T of Foo` → `Foo`) or `mixed` when no bound exists.
/// The returned vector is ordered to match `class.template_params`.
///
/// This follows PHPStan's `resolveToBounds()` semantics: unbound
/// template parameters are erased to their bounds so that downstream
/// consumers never see raw template names like `TValue`.
pub(crate) fn default_type_args(class: &ClassInfo) -> Vec<PhpType> {
    class
        .template_params
        .iter()
        .map(|p| {
            class
                .template_param_bounds
                .get(p)
                .cloned()
                .unwrap_or_else(PhpType::mixed)
        })
        .collect()
}

/// Apply explicit generic type arguments to a class's members.
///
/// When a type hint includes generic parameters (e.g. `Collection<int, User>`),
/// this function maps them to the class's `@template` parameters and rewrites
/// all method return types, method parameter types, and property type hints
/// with the concrete types.
///
/// If the class has no `template_params` or no `type_args` are provided,
/// returns a clone of the class unchanged.
///
/// # Example
///
/// Given a `Collection` class with `@template TKey` and `@template TValue`,
/// calling `apply_generic_args(&collection_class, &[PhpType::parse("int"), PhpType::parse("User")])`
/// will substitute every occurrence of `TKey` with `int` and `TValue` with `User`
/// in the class's methods and properties.
pub(crate) fn apply_generic_args(class: &ClassInfo, type_args: &[PhpType]) -> ClassInfo {
    let subs = build_generic_subs(class, type_args);

    if subs.is_empty() {
        return class.clone();
    }

    let mut result = class.clone();
    for method in result.methods.make_mut() {
        apply_substitution_to_method(Arc::make_mut(method), &subs);
    }
    for property in result.properties.make_mut() {
        apply_substitution_to_property(property, &subs);
    }

    // Substitute template params in generic annotations so that
    // downstream consumers (e.g. foreach element-type extraction)
    // see concrete types instead of raw template param names.
    // For example, `@implements IteratorAggregate<TKey, TValue>`
    // becomes `@implements IteratorAggregate<int, Customer>` when
    // TKey=int, TValue=Customer.
    apply_substitution_to_generics(&mut result.implements_generics, &subs);
    apply_substitution_to_generics(&mut result.extends_generics, &subs);
    apply_substitution_to_generics(&mut result.use_generics, &subs);

    result
}

/// Whether a template parameter bound represents a key-like type.
///
/// Returns `true` for `array-key`, `int`, `string`, and other types
/// that are conventionally used as collection key bounds.  This is
/// used by [`apply_generic_args`] to right-align generic arguments
/// when fewer arguments than template parameters are provided.
/// Compute the right-alignment offset when fewer type arguments are
/// provided than template parameters.
///
/// PHP/PHPStan/Psalm bind a short generic argument list to the *trailing*
/// template parameters: `Collection<User>` against `Collection<TKey,
/// TValue>` binds `TValue => User` and leaves `TKey` to its bound. The
/// heuristic only activates when every skipped leading parameter has a
/// key-like bound (`array-key`, `int`, or `string`), the universal
/// convention for collection key parameters. Otherwise it returns `0`
/// (left-aligned) so unrelated generics are not mis-bound.
fn right_align_offset(
    template_params: &[Atom],
    template_param_bounds: &crate::atom::AtomMap<PhpType>,
    num_args: usize,
) -> usize {
    if num_args >= template_params.len() {
        return 0;
    }
    let skip = template_params.len() - num_args;
    let all_skipped_are_key_like = template_params[..skip].iter().all(|param| {
        template_param_bounds
            .get(param)
            .is_some_and(is_key_like_bound)
    });
    if all_skipped_are_key_like { skip } else { 0 }
}

fn is_key_like_bound(bound: &PhpType) -> bool {
    match bound {
        PhpType::Named(_) => bound.is_array_key() || bound.is_int() || bound.is_string_type(),
        PhpType::Union(members) => {
            // `int|string` is equivalent to `array-key`.
            !members.is_empty() && members.iter().all(|m| m.is_int() || m.is_string_type())
        }
        _ => false,
    }
}

/// Apply a substitution map to a list of generic annotations.
///
/// Each entry is `(ClassName, [TypeArg1, TypeArg2, …])`.  Only the type
/// arguments are substituted; the class name is left unchanged.
fn apply_substitution_to_generics(
    generics: &mut [(Atom, Vec<PhpType>)],
    subs: &HashMap<String, PhpType>,
) {
    for (_class_name, type_args) in generics.iter_mut() {
        for arg in type_args.iter_mut() {
            let substituted = arg.substitute(subs);
            if substituted != *arg {
                *arg = substituted;
            }
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "inheritance_tests.rs"]
mod tests;
