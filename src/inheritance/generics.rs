//! Generic type substitution and application.
//!
//! This module handles type parameter binding and concrete type substitution
//! across inheritance chains. When a class declares `@extends Parent<ConcreteType>`
//! or `@use Trait<Type>`, this module maps template parameters to concrete types
//! and rewrites method/property signatures accordingly.

use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::Atom;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, MethodInfo, PropertyInfo};
use crate::util::short_name;

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
pub(crate) fn build_substitution_map(
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
) -> std::borrow::Cow<'a, str> {
    use std::borrow::Cow;
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
pub(crate) fn right_align_offset(
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

/// Whether a template parameter bound represents a key-like type.
///
/// Returns `true` for `array-key`, `int`, `string`, and other types
/// that are conventionally used as collection key bounds.  This is
/// used by [`apply_generic_args`] to right-align generic arguments
/// when fewer arguments than template parameters are provided.
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
