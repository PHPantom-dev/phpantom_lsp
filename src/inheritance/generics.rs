//! Generic type substitution and application.
//!
//! This module handles type parameter binding and concrete type substitution
//! across inheritance chains. When a class declares `@extends Parent<ConcreteType>`
//! or `@use Trait<Type>`, this module maps template parameters to concrete types
//! and rewrites method/property signatures accordingly.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::atom::Atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, MethodInfo, PropertyInfo};
use crate::util::short_name;

/// Apply generic type substitution to a method's return type, type
/// assertions and parameter type hints.
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
    // `@phpstan-assert-if-true T $id` on a generic interface promises the
    // implementation's `T`, not the bound.
    for assertion in &mut method.type_assertions {
        assertion.asserted_type = assertion.asserted_type.substitute(subs);
    }
    // Only copy-on-write the shared parameter list when a parameter
    // actually references a substituted name — substitution usually
    // rewrites just the return type.
    let keys: Vec<String> = subs.keys().cloned().collect();
    let any_param_referenced = method.parameters.iter().any(|p| {
        p.type_hint
            .as_ref()
            .is_some_and(|h| h.references_any_template_param(&keys))
    });
    if any_param_referenced {
        for param in method.parameters.make_mut() {
            if let Some(ref mut hint) = param.type_hint {
                *hint = hint.substitute(subs);
            }
        }
    }
}

/// Whether [`replace_bare_self_in_method`] would rewrite anything in
/// `method`.
///
/// Checks the same fields the replacement touches (return type and
/// parameter hints) so callers can keep the shared `Arc<MethodInfo>`
/// when the rewrite would be a no-op.
pub(crate) fn method_has_bare_self(method: &MethodInfo) -> bool {
    method
        .return_type
        .as_ref()
        .is_some_and(|r| r.contains_bare_self())
        || method
            .parameters
            .iter()
            .any(|p| p.type_hint.as_ref().is_some_and(|h| h.contains_bare_self()))
}

/// Whether an inheritance merge would rewrite a relative class keyword in
/// `method`: bare `self`, or bare `parent` when the declaring class's own
/// parent is known.
pub(crate) fn method_has_inherited_class_keyword(
    method: &MethodInfo,
    declaring_parent: Option<&str>,
) -> bool {
    method_has_bare_self(method) || (declaring_parent.is_some() && method_has_bare_parent(method))
}

fn method_has_bare_parent(method: &MethodInfo) -> bool {
    method
        .return_type
        .as_ref()
        .is_some_and(|r| r.contains_bare_parent())
        || method.parameters.iter().any(|p| {
            p.type_hint
                .as_ref()
                .is_some_and(|h| h.contains_bare_parent())
        })
}

/// Bind the relative class keywords of an inherited method to concrete
/// classes: bare `self` to `class_name`, bare `parent` to
/// `declaring_parent`.
///
/// Both bind to the class that *declares* the method, so an inherited copy
/// must carry those classes rather than the keywords — resolving them against
/// the class a call is made on names the wrong class as soon as a subclass
/// sits in between. `static` is deliberately left alone: it binds late, to the
/// class the call is made on.
pub(crate) fn bind_inherited_class_keywords(
    method: &mut MethodInfo,
    class_name: &str,
    declaring_parent: Option<&str>,
) {
    replace_bare_self_in_method(method, class_name);
    let Some(declaring_parent) = declaring_parent else {
        return;
    };
    if let Some(ref mut ret) = method.return_type
        && ret.contains_bare_parent()
    {
        *ret = ret.replace_bare_parent(declaring_parent);
    }
    if method_has_bare_parent(method) {
        for param in method.parameters.make_mut() {
            if let Some(ref mut hint) = param.type_hint
                && hint.contains_bare_parent()
            {
                *hint = hint.replace_bare_parent(declaring_parent);
            }
        }
    }
}

/// Replace bare `self` in a method's return type and parameter hints
/// with `class_name`.
///
/// `self` binds to the class that declares the method, so an inherited
/// or trait-imported method must carry the declaring class rather than
/// the literal keyword. `static` is deliberately left alone: it binds
/// late, to the class the call is made on.
pub(crate) fn replace_bare_self_in_method(method: &mut MethodInfo, class_name: &str) {
    if let Some(ref mut ret) = method.return_type
        && ret.contains_bare_self()
    {
        *ret = ret.replace_bare_self(class_name);
    }
    let any_param = method
        .parameters
        .iter()
        .any(|p| p.type_hint.as_ref().is_some_and(|h| h.contains_bare_self()));
    if any_param {
        for param in method.parameters.make_mut() {
            if let Some(ref mut hint) = param.type_hint
                && hint.contains_bare_self()
            {
                *hint = hint.replace_bare_self(class_name);
            }
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

/// Whether [`apply_substitution_to_method`] would rewrite anything in
/// `method` given a substitution keyed by `template_params`.
///
/// When this returns `false` the substitution is a guaranteed no-op, so
/// callers can keep the shared `Arc<MethodInfo>` (copy-on-write) instead
/// of deep-cloning and re-substituting. This is the dominant memory
/// saving in the inheritance merge: an Eloquent subclass inherits
/// hundreds of parent methods, but only the handful that actually mention
/// a template parameter need a distinct, substituted copy. The checked
/// fields mirror [`apply_substitution_to_method`] exactly (return type,
/// conditional return, type assertions, parameter hints).
pub(crate) fn method_references_params(method: &MethodInfo, template_params: &[String]) -> bool {
    if template_params.is_empty() {
        return false;
    }
    method
        .return_type
        .as_ref()
        .is_some_and(|r| r.references_any_template_param(template_params))
        || method
            .conditional_return
            .as_ref()
            .is_some_and(|c| c.references_any_template_param(template_params))
        || method.type_assertions.iter().any(|a| {
            a.asserted_type
                .references_any_template_param(template_params)
        })
        || method.parameters.iter().any(|p| {
            p.type_hint
                .as_ref()
                .is_some_and(|h| h.references_any_template_param(template_params))
        })
}

/// Whether [`apply_substitution_to_property`] would rewrite `property`'s
/// type hint given a substitution keyed by `template_params`.
pub(crate) fn property_references_params(
    property: &PropertyInfo,
    template_params: &[String],
) -> bool {
    if template_params.is_empty() {
        return false;
    }
    property
        .type_hint
        .as_ref()
        .is_some_and(|h| h.references_any_template_param(template_params))
}

/// Fill in every template parameter of `source` that `subs` leaves
/// unbound, using the parameter's declared default (`@template T = Foo`),
/// else its bound (`@template T of object` → `object`), else `mixed`. A
/// default or bound naming another parameter reads what that one is bound
/// to.
///
/// A class that extends, uses, or implements a generic without saying what
/// it binds the parameters to would otherwise leak the raw template names
/// into the members it inherits.
pub(crate) fn fill_template_bounds(source: &ClassInfo, subs: &mut HashMap<String, PhpType>) {
    for param_name in &source.template_params {
        let key = param_name.to_string();
        if subs.contains_key(key.as_str()) {
            continue;
        }
        let fallback = default_type_arg(source, param_name).substitute(subs);
        subs.insert(key, fallback);
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
    let offset = generic_arg_offset(parent, type_args.len());

    let mut omitted: Vec<&Atom> = Vec::new();
    for (i, param_name) in parent.template_params.iter().enumerate() {
        match i.checked_sub(offset).and_then(|index| type_args.get(index)) {
            Some(arg) => {
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
            // A skipped leading (key-like) param falls back to its declared
            // default, bound, or `mixed` so the raw template name never
            // leaks.
            None if i < offset => omitted.push(param_name),
            // A trailing param the arguments stop short of takes its
            // declared default, which can name another parameter
            // (`@template EO of DI = DI`) and so stands for what that one
            // received. Without a default it is left for
            // `fill_template_bounds`.
            None if parent.template_param_defaults.contains_key(param_name) => {
                omitted.push(param_name);
            }
            None => {}
        }
    }
    let fallbacks: Vec<PhpType> = omitted
        .iter()
        .map(|param_name| default_type_arg(parent, param_name).substitute(&map))
        .collect();
    for (param_name, fallback) in omitted.into_iter().zip(fallbacks) {
        map.insert(param_name.to_string(), fallback);
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
    let offset = generic_arg_offset(class, type_args.len());

    let mut subs = HashMap::new();
    let mut omitted: Vec<&Atom> = Vec::new();
    for (i, param_name) in class.template_params.iter().enumerate() {
        // Skipped (right-aligned) params, and params left over once the
        // arguments run out, fall back to their declared default, upper
        // bound, or `mixed` so the raw template name never leaks into
        // downstream consumers.
        match i.checked_sub(offset).and_then(|index| type_args.get(index)) {
            Some(arg) => {
                subs.insert(param_name.to_string(), arg.clone());
            }
            None => omitted.push(param_name),
        }
    }
    // A default can name another parameter (`@template EO of DI = DI`), which
    // stands for whatever that parameter received here.
    let fallbacks: Vec<PhpType> = omitted
        .iter()
        .map(|param_name| default_type_arg(class, param_name).substitute(&subs))
        .collect();
    for (param_name, fallback) in omitted.into_iter().zip(fallbacks) {
        subs.insert(param_name.to_string(), fallback);
    }

    subs
}

/// Build default type arguments for a class whose template parameters
/// have no concrete bindings (e.g. `new Collection()` without a generic
/// annotation).
///
/// Each template parameter is mapped to its declared default when present
/// (`@template T of Foo = Bar` → `Bar`), otherwise to its upper bound
/// (`@template T of Foo` → `Foo`) or `mixed` when neither exists.
/// The returned vector is ordered to match `class.template_params`.
///
/// A declared default is the concrete argument an omitted generic parameter
/// receives. Parameters without one follow PHPStan's `resolveToBounds()`
/// semantics so downstream consumers never see raw names like `TValue`.
pub(crate) fn default_type_args(class: &ClassInfo) -> Vec<PhpType> {
    class
        .template_params
        .iter()
        .map(|p| default_type_arg(class, p))
        .collect()
}

/// Overlay call-site template values on a class's declared defaults.
///
/// Known receiver or method substitutions take precedence. The borrowed fast
/// path avoids work for the common case where the class declares no defaults,
/// or where the receiver already supplies every defaulted parameter.
pub(crate) fn template_values_with_defaults<'a>(
    class: &ClassInfo,
    values: &'a HashMap<String, PhpType>,
) -> Cow<'a, HashMap<String, PhpType>> {
    if class.template_param_defaults.is_empty()
        || class
            .template_param_defaults
            .keys()
            .all(|name| values.contains_key(name.as_str()))
    {
        return Cow::Borrowed(values);
    }

    let mut merged = HashMap::with_capacity(class.template_param_defaults.len() + values.len());
    merged.extend(
        class
            .template_param_defaults
            .iter()
            .map(|(name, default)| (name.to_string(), default.clone())),
    );
    merged.extend(
        values
            .iter()
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    Cow::Owned(merged)
}

/// Drop a method's own `@template` parameters from a call's template values.
///
/// A conditional keyed on a class-level parameter (`TAsync is false`) is
/// decided by the value that parameter holds, but one keyed on the method's
/// own (`T is int`) is decided by the argument that binds `T` — comparing it
/// against the substituted value instead reads a resolved type as though it
/// were a literal and picks the wrong branch. Stripping the method's names
/// leaves those conditionals to the argument-binding path.
pub(crate) fn class_scoped_template_values<'a>(
    values: &'a HashMap<String, PhpType>,
    method_template_params: &[Atom],
) -> Cow<'a, HashMap<String, PhpType>> {
    if method_template_params.is_empty()
        || !method_template_params
            .iter()
            .any(|name| values.contains_key(name.as_str()))
    {
        return Cow::Borrowed(values);
    }

    Cow::Owned(
        values
            .iter()
            .filter(|(name, _)| {
                !method_template_params
                    .iter()
                    .any(|param| param.as_str() == name.as_str())
            })
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
    )
}

#[inline]
fn default_type_arg(class: &ClassInfo, param: &Atom) -> PhpType {
    class
        .template_param_defaults
        .get(param)
        .or_else(|| class.template_param_bounds.get(param))
        .cloned()
        .unwrap_or_else(PhpType::mixed)
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
    let sub_keys: Vec<String> = subs.keys().cloned().collect();

    // Only copy-on-write the member vectors (and the individual members)
    // that actually reference a substituted template parameter.  A
    // generic variant like `Builder<User>` shares every method that does
    // not mention `TModel` with the base resolution instead of
    // deep-cloning the entire member set per instantiation.
    if result
        .methods
        .iter()
        .any(|m| method_references_params(m, &sub_keys))
    {
        for method in result.methods.make_mut() {
            // A method that redeclares one of these names as its own
            // `@template` shadows the class's: its signature must keep
            // the raw name for call-site binding, not the class-level
            // erasure, or the argument never gets a chance to bind it
            // (see `class_scoped_template_values`, which strips the same
            // shadowed names from the *values* side of a call).
            let method_subs = class_scoped_template_values(&subs, &method.template_params);
            if method_subs.is_empty() {
                continue;
            }
            let method_sub_keys: Vec<String> = method_subs.keys().cloned().collect();
            if method_references_params(method, &method_sub_keys) {
                let fp = crate::virtual_members::TransformFingerprint::new(
                    Some(method_subs.as_ref()),
                    None,
                    0,
                );
                let transformed =
                    crate::virtual_members::intern_transformed_method(method, fp, || {
                        let mut m = (**method).clone();
                        apply_substitution_to_method(&mut m, &method_subs);
                        m
                    });
                *method = transformed;
            }
        }
    }
    if result
        .properties
        .iter()
        .any(|p| property_references_params(p, &sub_keys))
    {
        let fp = crate::virtual_members::TransformFingerprint::new(Some(&subs), None, 0);
        for property in result.properties.make_mut() {
            if property_references_params(property, &sub_keys) {
                let transformed =
                    crate::virtual_members::intern_transformed_property(property, fp, || {
                        let mut p = (**property).clone();
                        apply_substitution_to_property(&mut p, &subs);
                        p
                    });
                *property = transformed;
            }
        }
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

/// Where the written arguments of a generic `class` start among its
/// template parameters.
///
/// [`right_align_offset`] decides it from the parameters' bounds, which
/// leaves the core iterable interfaces out: their `TKey` is unbounded, yet
/// `Iterator<Foo>` names the value, not the key. PHPStan hard-codes the same
/// reading for `Traversable`, `Iterator`, `IteratorAggregate` and
/// `Generator`.
pub(crate) fn generic_arg_offset(class: &ClassInfo, num_args: usize) -> usize {
    if num_args == 1
        && class.template_params.len() > 1
        && matches!(
            class.fqn().as_str(),
            "Traversable" | "Iterator" | "IteratorAggregate" | "Generator"
        )
    {
        return 1;
    }
    right_align_offset(
        &class.template_params,
        &class.template_param_bounds,
        num_args,
    )
}

/// Whether a template parameter bound represents a key-like type.
///
/// Returns `true` for `array-key`, `int`, `string`, and other types
/// that are conventionally used as collection key bounds.  This is
/// used by [`apply_generic_args`] to right-align generic arguments
/// when fewer arguments than template parameters are provided.
fn is_key_like_bound(bound: &PhpType) -> bool {
    match bound.kind() {
        TypeKind::Named(_) => bound.is_array_key() || bound.is_int() || bound.is_string_type(),
        TypeKind::Union(members) => {
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
