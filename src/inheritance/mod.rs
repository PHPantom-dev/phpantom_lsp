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

pub mod enrichment;
pub mod generics;
pub mod traits;

use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::{Atom, AtomSet};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH, Visibility};

// Re-export functions that are used internally
pub(crate) use enrichment::enrich_method_from_ancestor;
pub(crate) use enrichment::enrich_property_from_ancestor;

#[cfg(test)]
pub(crate) use generics::apply_substitution;
pub(crate) use generics::{
    apply_generic_args, apply_substitution_to_conditional, apply_substitution_to_method,
    apply_substitution_to_property, build_generic_subs, build_substitution_map, default_type_args,
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
        dedup
            .methods
            .insert(crate::atom::ascii_lowercase_atom(&m.name));
    }
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
}

use crate::virtual_members::laravel::{factory_to_model_fqn, is_factory_class};

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

        // Merge parent methods — skip private.
        // When the child already has a method with the same name,
        // enrich it with the parent's richer docblock types instead
        // of silently discarding the parent's type information.
        for method in &parent.methods {
            if method.visibility == Visibility::Private {
                continue;
            }
            if !dedup
                .methods
                .insert(crate::atom::ascii_lowercase_atom(&method.name))
            {
                // Child already has this method — enrich it from parent.
                let mut ancestor_method = (**method).clone();
                if !level_subs.is_empty() {
                    apply_substitution_to_method(&mut ancestor_method, &level_subs);
                }
                if let Some(existing) = merged
                    .methods
                    .make_mut()
                    .iter_mut()
                    .find(|m| m.name.eq_ignore_ascii_case(&method.name))
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
                .find(|m| m.name.eq_ignore_ascii_case(&method.name))
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

    // Refine the `cases()` method on enums.  The `UnitEnum` interface
    // declares `public static function cases(): array`, which loses the
    // element type: `Country::cases()[0]` would resolve to `mixed`.
    // Every concrete enum returns a list of its own instances, so
    // replace the bare `array` with `list<EnumName>` (using the FQN so
    // the element resolves regardless of the call site's namespace).
    if merged.kind == crate::types::ClassLikeKind::Enum {
        let element = PhpType::Named(merged.fqn().to_string());
        let list_type = PhpType::Generic("list".to_string(), vec![element]);
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
    if matches!(&resolved, PhpType::IndexAccess(_, _)) {
        return None;
    }

    Some(resolved)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "inheritance_tests.rs"]
mod tests;
