//! Type compatibility: whether a value of one type can be passed where
//! another is declared, as the argument, return, and property type-mismatch
//! diagnostics judge it.
//!
//! [`is_type_compatible`] sits on top of the subtype check
//! ([`is_subtype_of_typed`]). It adds conservative "MAYBE" escape hatches
//! that answer `true` whenever the types *might* be compatible at runtime
//! but cannot be proven so statically, and PHP's coercions outside
//! `strict_types`. See the "Architecture note" on [`is_type_compatible`]
//! for the distinction between the two.
//!
//! A framework's type operators (Laravel's `builder-of<Model>`, …) are
//! expanded by the caller before the check, which knows nothing of them.

use std::collections::{HashMap, HashSet};

use super::is_subtype_of_typed;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{
    CallableType, LiteralValue, PhpType, ShapeEntry, ShapeParts, TypeKind,
    int_literal_is_within_range, is_array_like_name, shape_key_type,
};
use crate::types::{ClassInfo, Visibility};

/// Returns `true` when the type names a class by an unqualified short name
/// the project cannot load.  Covers both `Foo` and `Foo<Bar>`: a generic
/// whose base name is unresolvable is just as unverifiable as a plain one,
/// and so is the `class-string<T>` of an enclosing template `T`.
fn is_unloadable_short_name(ty: &PhpType, class_loader: &ResolvedClassLoader<'_>) -> bool {
    let name = match ty.kind() {
        TypeKind::Named(n) => n.as_str(),
        TypeKind::Generic(g) => g.name.as_str(),
        TypeKind::ClassString(Some(inner)) | TypeKind::InterfaceString(Some(inner)) => {
            return is_unloadable_short_name(inner, class_loader);
        }
        _ => return false,
    };
    !name.contains('\\')
        && !crate::php_type::is_builtin_non_class_type(name)
        && !is_array_like_name(name)
        && class_loader(Fqn::new(name)).is_none()
}

/// Returns `true` when the type is an array that says nothing about its
/// values: bare `array`, `mixed[]`, or `array<K, mixed>`.
///
/// All three carry the same amount of information — none — so none of them
/// can contradict a typed array. `array<array-key, mixed>` in particular is
/// what a resolver produces when it knows a value is an array but nothing
/// more, which is precisely the case a mismatch must not be claimed for.
///
/// A bare `list`, `non-empty-array`, or `non-empty-list` counts the same
/// way: each refines the *shape* of an array whose values are still
/// unknown, so a truthy narrowing that turns a bare `array` into a bare
/// `non-empty-array` must not start contradicting typed parameters the
/// unrefined type reached fine.
///
/// An unsealed shape is not one, whatever its tail holds: it names the
/// entries it lists, which the plain array it widens to has lost.
fn is_bare_array(ty: &PhpType) -> bool {
    if ty.as_unsealed_shape().is_some() {
        return false;
    }
    match ty.kind() {
        TypeKind::Named(name) => !name.eq_ignore_ascii_case("iterable") && is_array_like_name(name),
        TypeKind::Array(inner) => inner.is_mixed(),
        TypeKind::Generic(generic) => {
            is_array_like_name(&generic.name)
                && generic.args.last().is_some_and(|value| value.is_mixed())
        }
        _ => false,
    }
}

/// The required keys a parameter's array shape declares that the
/// argument's array shape does not hold, in the order the parameter
/// declares them.
///
/// Empty unless both sides are shapes: only a shape enumerates its keys,
/// so only a shape can be found short of one.  Whether the argument's
/// shape can be trusted to be the *whole* array is the caller's call —
/// this only reports the difference between the two.
///
/// Either side may be unsealed; the keys it lists are the ones it holds.
pub(crate) fn missing_required_shape_keys(arg_type: &PhpType, param_type: &PhpType) -> Vec<String> {
    let (Some(arg), Some(param)) = (arg_type.shape_parts(), param_type.shape_parts()) else {
        return Vec::new();
    };

    let arg_keys = shape_keys(arg.entries);
    shape_keys(param.entries)
        .into_iter()
        .zip(param.entries.iter())
        .filter(|(key, entry)| !entry.optional && !arg_keys.contains(key))
        .map(|(key, _)| key)
        .collect()
}

/// The keys the argument's array shape lists that a sealed parameter shape
/// does not declare, in the order the argument lists them.
///
/// Empty unless both sides are shapes and the parameter's has no `...`.
pub(crate) fn undeclared_shape_keys(
    arg_type: &PhpType,
    param_type: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<String> {
    let (Some(arg), Some(param)) = (arg_type.shape_parts(), param_type.shape_parts()) else {
        return Vec::new();
    };
    if param.tail.is_some() {
        return Vec::new();
    }
    let param_keys = shape_keys(param.entries);
    shape_keys(arg.entries)
        .into_iter()
        .filter(|key| !param_keys.contains(key) && !may_be_listed(key, &param_keys, class_loader))
        .collect()
}

/// Whether the parameter demands a list: an array whose keys run
/// `0, 1, 2, …` in that order, which is what `array_is_list()` answers
/// `true` for.
fn requires_list(param_type: &PhpType) -> bool {
    if param_type.is_list_shape() {
        return true;
    }
    match param_type.kind() {
        TypeKind::Nullable(inner) => requires_list(inner),
        TypeKind::Named(name) => is_list_name(name),
        TypeKind::Generic(generic) => is_list_name(&generic.name),
        _ => false,
    }
}

fn is_list_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("list") || name.eq_ignore_ascii_case("non-empty-list")
}

/// Whether an array shape holds keys a list cannot have, so it cannot
/// satisfy `param_type`.
///
/// Only meaningful for a shape that enumerates every key the value has:
/// a shape built by watching assignments lists the keys we happened to
/// see, in the order we saw them, which says nothing about the order the
/// value's keys are really in.  The caller decides which it has.
pub(crate) fn shape_breaks_list_order(arg_type: &PhpType, param_type: &PhpType) -> bool {
    if !requires_list(param_type) {
        return false;
    }
    let TypeKind::ArrayShape(entries) = arg_type.kind() else {
        return false;
    };
    shape_keys(entries)
        .iter()
        .enumerate()
        .any(|(index, key)| key.parse::<usize>() != Ok(index))
}

/// What an array type asks of every array it accepts, read off its
/// spelling: `array<K, V>`, `list<V>`, `V[]`, `non-empty-array`, …
struct ArrayDemand<'a> {
    key: Option<&'a PhpType>,
    value: Option<&'a PhpType>,
    list: bool,
    non_empty: bool,
}

impl<'a> ArrayDemand<'a> {
    /// `None` for anything that is not an array type.  `iterable` is not
    /// one: it takes a `Traversable` as readily as an array.
    fn of(ty: &'a PhpType) -> Option<Self> {
        let (name, args): (&str, &'a [PhpType]) = match ty.kind() {
            TypeKind::Array(elem) => {
                return Some(Self {
                    key: None,
                    value: Some(elem),
                    list: false,
                    non_empty: false,
                });
            }
            TypeKind::Named(name) => (name.as_str(), &[]),
            TypeKind::Generic(g) => (g.name.as_str(), &g.args),
            _ => return None,
        };
        if !is_array_like_name(name) || name.eq_ignore_ascii_case("iterable") {
            return None;
        }
        let list = is_list_name(name);
        Some(Self {
            key: if args.len() == 2 { args.first() } else { None },
            value: args.last(),
            list,
            non_empty: crate::php_type::is_non_empty_array_name(name),
        })
    }
}

/// The key each shape entry stands for, filling in the sequential index
/// PHP assigns an entry written without one: `array{a: int, string}`
/// holds a `0` exactly as `array{a: int, 0: string}` does, so the two
/// spellings have to compare equal.
fn shape_keys(entries: &[ShapeEntry]) -> Vec<String> {
    let mut next_index: i64 = 0;
    entries
        .iter()
        .map(|entry| match &entry.key {
            Some(key) => {
                if let Ok(index) = key.parse::<i64>() {
                    next_index = next_index.max(index + 1);
                }
                key.clone()
            }
            None => {
                let key = next_index.to_string();
                next_index += 1;
                key
            }
        })
        .collect()
}

/// Whether an array shape passes where another is expected, either of them
/// possibly unsealed.
///
/// Only a key both shapes list can contradict: its value has to fit the
/// parameter's.  A required key the argument does not list is a MAYBE,
/// because a shape read off a variable lists the keys we saw assigned, not
/// every key the array has; the caller reports it for a literal, which does
/// enumerate them all.
///
/// A shape written without `...` declares every key it handles, so an
/// entry the argument lists and the parameter does not rules it out.  An
/// unsealed parameter's tail says what such an entry looks like instead:
/// every entry the parameter does not list, and the tail of an unsealed
/// argument, have to fit it.
fn shape_is_compatible(
    arg: &ShapeParts<'_>,
    param: &ShapeParts<'_>,
    class_loader: &ResolvedClassLoader<'_>,
    strict_types: bool,
) -> bool {
    let arg_keys = shape_keys(arg.entries);
    let param_keys = shape_keys(param.entries);
    let listed_fit = param_keys
        .iter()
        .zip(param.entries.iter())
        .all(|(key, pe)| {
            arg_keys
                .iter()
                .position(|arg_key| arg_key == key)
                .is_none_or(|i| {
                    is_type_compatible(
                        &arg.entries[i].value_type,
                        &pe.value_type,
                        class_loader,
                        strict_types,
                    )
                })
        });
    if !listed_fit {
        return false;
    }
    let Some((tail_key, tail_value)) = param.tail else {
        return arg_keys
            .iter()
            .all(|key| param_keys.contains(key) || may_be_listed(key, &param_keys, class_loader));
    };

    // The tail is judged the way the type arguments of an array are: its
    // keys and values are never coerced to fit, whatever the file's
    // `strict_types`.  A bare `...` takes any key, so there is no point in
    // building a type for each entry's key to find that out.
    let any_key = tail_key.is_array_key();
    let key_fits = |key: &PhpType| is_type_compatible(key, tail_key, class_loader, true);
    let value_fits = |value: &PhpType| {
        tail_value.is_mixed() || is_type_compatible(value, tail_value, class_loader, true)
    };

    let extras_fit = arg_keys
        .iter()
        .zip(arg.entries.iter())
        .filter(|(key, _)| !param_keys.contains(*key))
        .all(|(key, entry)| {
            (any_key || key_fits(&shape_key_type(key))) && value_fits(&entry.value_type)
        });
    extras_fit
        && arg.tail.is_none_or(|(arg_tail_key, arg_tail_value)| {
            (any_key || key_fits(arg_tail_key)) && value_fits(arg_tail_value)
        })
}

/// Whether `key`, which `listed` does not hold as spelled, may still be one
/// of its keys: either side may spell a key as `Foo::CONSTANT`, which is
/// the key the constant evaluates to, or any key at all when it cannot be
/// evaluated.
fn may_be_listed(key: &str, listed: &[String], class_loader: &ResolvedClassLoader<'_>) -> bool {
    let evaluate = |key: &str| {
        crate::php_type::class_constant_key(key)
            .map(|_| crate::class_lookup::evaluate_constant_shape_key(key, class_loader))
    };
    let key = match evaluate(key) {
        None => std::borrow::Cow::Borrowed(key),
        Some(Some(evaluated)) => std::borrow::Cow::Owned(evaluated),
        Some(None) => return true,
    };
    listed.iter().any(|other| match evaluate(other) {
        None => *other == *key,
        Some(evaluated) => evaluated.is_none_or(|evaluated| evaluated == *key),
    })
}

/// Whether an array shape, sealed or unsealed, passes where a typed array is
/// expected.
///
/// Every key and value the shape lists is one the array holds, so each has
/// to fit what the parameter declares, and a shape with no entries at all is
/// the empty array a `non-empty-*` parameter rejects.  What an unsealed
/// shape holds beyond them is known only by its tail, which is judged the
/// way the type arguments of an array are: its keys and values are never
/// coerced to fit, whatever the file's `strict_types`.
fn shape_fits_array(
    arg: &ShapeParts<'_>,
    demand: &ArrayDemand<'_>,
    class_loader: &ResolvedClassLoader<'_>,
    strict_types: bool,
) -> bool {
    if demand.non_empty && arg.entries.is_empty() {
        return false;
    }
    let listed_fit = shape_keys(arg.entries)
        .iter()
        .zip(arg.entries)
        .all(|(key, entry)| {
            // A key spelled `Foo::CONSTANT` is checked as the key the
            // constant evaluates to, when it can be.
            let evaluated = crate::class_lookup::evaluate_constant_shape_key(key, class_loader);
            let key = evaluated.as_deref().unwrap_or(key);
            (!demand.list || crate::php_type::is_canonical_int_key(key))
                && demand.key.is_none_or(|key_type| {
                    // A key is never coerced to fit, whatever the file's
                    // `strict_types`: PHP decides it when the array is built.
                    is_type_compatible(&shape_key_type(key), key_type, class_loader, true)
                })
                && demand.value.is_none_or(|value_type| {
                    is_type_compatible(&entry.value_type, value_type, class_loader, strict_types)
                })
        });
    listed_fit
        && arg.tail.is_none_or(|(tail_key, tail_value)| {
            demand
                .key
                .is_none_or(|key_type| is_type_compatible(tail_key, key_type, class_loader, true))
                && demand.value.is_none_or(|value_type| {
                    is_type_compatible(tail_value, value_type, class_loader, true)
                })
        })
}

/// `arg` seen as its generic ancestor `ancestor_name`, carrying the type
/// arguments its `@extends`/`@implements` chain binds on the way up:
/// `IntBox` with `@extends Box<int>` is a `Box<int>`.
///
/// `None` when the argument is not a class below that ancestor, or when a
/// link of the chain does not say what it binds the next level to, since
/// then the ancestor's arguments are unknown rather than contradicting.
fn as_generic_ancestor(
    arg: &PhpType,
    ancestor_name: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    let (name, args): (&str, &[PhpType]) = match arg.kind() {
        TypeKind::Named(name) => (name.as_str(), &[]),
        TypeKind::Generic(g) => (g.name.as_str(), &g.args),
        _ => return None,
    };
    if name.eq_ignore_ascii_case(ancestor_name)
        || is_array_like_name(name)
        || crate::php_type::is_builtin_non_class_type(name)
    {
        return None;
    }
    let class = class_loader(Fqn::new(name))?;
    let ancestor = class_loader(Fqn::new(ancestor_name))?;
    if ancestor.template_params.is_empty() || class.fqn() == ancestor.fqn() {
        return None;
    }

    let mut subs: HashMap<String, PhpType> = if args.len() == class.template_params.len() {
        class
            .template_params
            .iter()
            .map(|param| param.to_string())
            .zip(args.iter().cloned())
            .collect()
    } else {
        HashMap::new()
    };
    crate::inheritance::fill_template_bounds(&class, &mut subs);

    let mut visited = HashSet::new();
    let bound = bind_up_to_ancestor(&class, &ancestor, &subs, &mut visited, class_loader, 0)?;
    let ancestor_args = ancestor
        .template_params
        .iter()
        .map(|param| bound.get(param.as_str()).cloned())
        .collect::<Option<Vec<_>>>()?;
    Some(PhpType::generic(ancestor_name, ancestor_args))
}

/// The substitution for `ancestor`'s template parameters, threaded from
/// `current`'s (`subs`) through each `@extends`/`@implements` on the way.
fn bind_up_to_ancestor(
    current: &ClassInfo,
    ancestor: &ClassInfo,
    subs: &HashMap<String, PhpType>,
    visited: &mut HashSet<crate::atom::Atom>,
    class_loader: &ResolvedClassLoader<'_>,
    depth: u32,
) -> Option<HashMap<String, PhpType>> {
    if depth > crate::types::MAX_INHERITANCE_DEPTH {
        return None;
    }
    for name in current.parent_class.iter().chain(current.interfaces.iter()) {
        let Some(next) = class_loader(Fqn::new(name)) else {
            continue;
        };
        if !visited.insert(next.fqn()) {
            continue;
        }
        let next_short = crate::util::short_name(&next.name);
        let binds = current
            .extends_generics
            .iter()
            .chain(current.implements_generics.iter())
            .any(|(bound_name, _)| crate::util::short_name(bound_name) == next_short);
        if next.fqn() == ancestor.fqn() {
            return binds.then(|| crate::inheritance::build_substitution_map(current, &next, subs));
        }
        let next_subs = if next.template_params.is_empty() || !binds {
            let mut unbound = HashMap::new();
            crate::inheritance::fill_template_bounds(&next, &mut unbound);
            unbound
        } else {
            crate::inheritance::build_substitution_map(current, &next, subs)
        };
        if let Some(found) = bind_up_to_ancestor(
            &next,
            ancestor,
            &next_subs,
            visited,
            class_loader,
            depth + 1,
        ) {
            return Some(found);
        }
    }
    None
}

/// Returns `true` when two class-like types may name the same class
/// despite being spelled differently.
///
/// A name that arrives here unqualified is one the pipeline could not
/// place in its file's namespace, and resolving a bare name against the
/// whole project is a guess — in a codebase with several `User` classes
/// the loader picks one of them arbitrarily.  When that guess is the only
/// thing separating two types, there is no mismatch to report.
fn may_name_same_class(a: &PhpType, b: &PhpType) -> bool {
    let (Some(name_a), Some(name_b)) = (a.base_name(), b.base_name()) else {
        return false;
    };
    if name_a.contains('\\') && name_b.contains('\\') {
        return false;
    }
    crate::util::short_name(name_a).eq_ignore_ascii_case(crate::util::short_name(name_b))
}

/// The first parameter of `arg_sig` that rejects a value `param_sig`
/// promises to pass it, as `(zero-based position, passed type, accepted
/// type)`.
///
/// Parameters are contravariant, so the check runs from the
/// specification's type to the closure's.  A closure that declares no
/// parameters here is one whose list was never recorded, and a position
/// the closure has no parameter for simply drops the value (PHP ignores
/// surplus arguments to a closure), so neither is a rejection.
pub(crate) fn first_rejected_callable_param<'a>(
    arg_sig: &'a CallableType,
    param_sig: &'a CallableType,
    class_loader: &ResolvedClassLoader<'_>,
    strict_types: bool,
) -> Option<(usize, &'a PhpType, &'a PhpType)> {
    let accepting = &arg_sig.params;
    if accepting.is_empty() {
        return None;
    }
    let accepting_at = |i: usize| {
        accepting
            .get(i)
            .or_else(|| accepting.last().filter(|last| last.variadic))
    };
    for (i, passed) in param_sig.params.iter().enumerate() {
        // A variadic specification parameter fills every closure
        // parameter from its position on.
        let positions = if passed.variadic {
            i..accepting.len().max(i + 1)
        } else {
            i..i + 1
        };
        for pos in positions {
            let Some(accepts) = accepting_at(pos) else {
                break;
            };
            if !is_type_compatible(
                &passed.type_hint,
                &accepts.type_hint,
                class_loader,
                strict_types,
            ) {
                return Some((pos, &passed.type_hint, &accepts.type_hint));
            }
        }
    }
    None
}

/// Check if an argument type is compatible with a parameter type.
///
/// Returns `true` if the argument type can be passed to the parameter
/// without a type error.  Conservative: returns `true` (compatible)
/// when in doubt.
///
/// # Architecture note
///
/// This is a diagnostic-policy layer on top of the core type system.  It
/// contains two kinds of checks:
///
/// 1. **MAYBE escape hatches** — cases where the types *might* be
///    compatible at runtime but we can't prove it statically.  These
///    return `true` (suppress the diagnostic) to avoid false positives.
///    They encode diagnostic policy, not type theory, and are unique to
///    this function.
///
/// 2. **Hierarchy checks with permissive fallbacks** — nominal subtype
///    checks that fall back to `true` when a class can't be loaded, rather
///    than producing a false positive.
///
/// Strict subtype relationships (Cat <: Animal, never <: T,
/// Closure(int): void <: callable, array<int, string> <: array,
/// Collection<Cat> <: object, ArrayIterator <: iterable,
/// array<int, Cat> <: Animal[]) are handled by the [`is_subtype_of_typed`]
/// fallback at the end and should NOT be duplicated here.  A rule that
/// recurses into `is_type_compatible` for a nested position is not a
/// duplicate: it carries the MAYBE rules into that position, which the
/// core engine deliberately does not.
///
/// A future pass may tighten specific MAYBE relationships (reporting them
/// as mismatches rather than letting them pass) once we are confident the
/// narrowing does not introduce false positives.  Each one that goes has
/// to be paid for with resolver precision first: see the class-hierarchy
/// note in [`Compat::generics`] for how the supertype-where-subtype hatch
/// was retired.
///
/// The rules are grouped into phases, each answering for the pairs it
/// recognises and passing the rest on; the order of the phases, and of the
/// rules inside one, is significant.
pub(crate) fn is_type_compatible(
    arg_type: &PhpType,
    param_type: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
    strict_types: bool,
) -> bool {
    Compat {
        class_loader,
        strict_types,
    }
    .check(arg_type, param_type)
}

/// The settings one compatibility check runs under.
#[derive(Clone, Copy)]
struct Compat<'l> {
    class_loader: &'l ResolvedClassLoader<'l>,
    strict_types: bool,
}

impl Compat<'_> {
    fn check(self, arg: &PhpType, param: &PhpType) -> bool {
        self.rewritten(arg, param)
            .or_else(|| self.unverifiable(arg, param))
            .or_else(|| self.callables(arg, param))
            .or_else(|| self.class_keywords(arg, param))
            .or_else(|| self.composites(arg, param))
            .or_else(|| self.arrays(arg, param))
            .or_else(|| self.nullable_param(arg, param))
            .or_else(|| self.coercions(arg, param))
            .or_else(|| self.string_pseudo_types(arg, param))
            .or_else(|| self.generics(arg, param))
            .or_else(|| self.array_substitutes(arg, param))
            .or_else(|| self.object_shapes(arg, param))
            .unwrap_or_else(|| is_subtype_of_typed(arg, param, self.class_loader))
    }

    /// The same check with `strict_types` on: a type argument, an array
    /// key, or a `class-string`'s class is never coerced to fit.
    fn strictly(self) -> Self {
        Self {
            strict_types: true,
            ..self
        }
    }

    /// Spellings that are judged as another type: a benevolent union, a
    /// conditional or type operator that survived resolution, and
    /// `array-key`.
    fn rewritten(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        // A benevolent value came out of a builtin whose failure branch nobody
        // checks (`tempnam()`, `curl_init()`, `Redis::get()`, …), so the union
        // is satisfied as soon as one branch fits — the failure branch alone is
        // not worth a diagnostic.  See `crate::benevolent_builtins` for why, and
        // note that this relaxes nothing outside the diagnostics: the value
        // keeps its full union everywhere else, so `=== false` still narrows and
        // still reads as a live comparison.
        if arg_type.is_benevolent() {
            let stripped = arg_type.strip_benevolence();
            return Some(match stripped.kind() {
                TypeKind::Union(members) => {
                    members.iter().any(|member| self.check(member, param_type))
                }
                _ => self.check(&stripped, param_type),
            });
        }
        // A benevolent *parameter* is the same bargain read from the other side:
        // it accepts whatever any of its branches accepts.
        if param_type.is_benevolent() {
            return Some(self.check(arg_type, &param_type.strip_benevolence()));
        }

        // A conditional that survived resolution (the call-site arguments that
        // would decide it were not available) is a type expression, not a set
        // of values — comparing a concrete type against it can only fail.
        // Compare against the union of its branches instead, which is what the
        // value satisfies however the condition resolves.
        if arg_type.contains_conditional() || param_type.contains_conditional() {
            return Some(self.check(
                &arg_type.conditionals_as_branch_unions(),
                &param_type.conditionals_as_branch_unions(),
            ));
        }

        // A `key-of<T>` / `value-of<T>` / `T[K]` that survived resolution is a type
        // expression too: its operand was a class constant we could not read or a
        // template that was never substituted, so it names no set of values and
        // every comparison against it fails.  The two sides need opposite
        // treatment.  As an argument it is simply an unknown type — widening it to
        // the bound its result falls within and then demanding the *bound* fit the
        // parameter reports every call, however narrow the real value turns out to
        // be.  As a parameter the bound is a genuine constraint the value has to
        // satisfy however the operator evaluates, so it is worth keeping.
        if arg_type.contains_unevaluated_operator() {
            return Some(true);
        }
        if param_type.contains_unevaluated_operator() {
            return Some(self.check(arg_type, &param_type.unevaluated_operators_as_bounds()));
        }

        // `array-key` is exactly `int|string`, so it has to be judged as
        // that union: written as one name it matches neither half, and every
        // rule below that reasons about a union member (including the
        // int-to-string coercion PHP performs outside `strict_types`) would
        // pass it over. `is_subtype_of` expands it for the same reason.
        //
        // The union it expands to is *benevolent*, which is what PHPStan
        // resolves the name to as well. `array-key` is never something a
        // value was measured to be: it is the key type of an array nobody
        // said the keys of, so demanding that both halves fit reports a call
        // on the strength of a type the engine admits it does not know. One
        // half fitting is the whole bargain — and only here, in the
        // diagnostic, since the type itself keeps both halves everywhere
        // else.
        if arg_type.is_array_key() || param_type.is_array_key() {
            let expand = |ty: &PhpType| {
                if ty.is_array_key() {
                    PhpType::benevolent(PhpType::union(vec![PhpType::int(), PhpType::string()]))
                } else {
                    ty.clone()
                }
            };
            return Some(self.check(&expand(arg_type), &expand(param_type)));
        }
        None
    }

    /// Pairs that say too little to be judged: an unknown type on either
    /// side, a name that cannot be loaded, an anonymous class, a bare
    /// `object` argument.
    fn unverifiable(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        // Skip if either type is unresolved/unknown.
        if arg_type.is_untyped() || param_type.is_untyped() {
            return Some(true);
        }
        if arg_type.is_mixed() || param_type.is_mixed() {
            return Some(true);
        }
        // When the argument is a union that contains `mixed`, bail out
        // conservatively.  `mixed` means "we don't know" — the actual
        // runtime value could satisfy the parameter type, so reporting
        // an error would be a false positive.
        if let TypeKind::Union(members) = arg_type.kind()
            && members.iter().any(|m| m.is_mixed())
        {
            return Some(true);
        }
        // A `void` *parameter* is not a constraint any value can be judged
        // against — nothing produces a value of that type, so the annotation
        // that spelled it is what is wrong, not the argument.
        //
        // A `void` *argument* is a different matter and is reported: the call
        // it came from hands back no value at all, so whatever the parameter
        // asks for, it is not getting it.  PHP 8 quietly passes `null` there,
        // which makes it a bug that survives until the parameter is used.
        if param_type.is_void() {
            return Some(true);
        }
        // Skip Raw types (unparseable / unresolved type strings).
        if matches!(arg_type.kind(), TypeKind::Raw(_))
            || matches!(param_type.kind(), TypeKind::Raw(_))
        {
            return Some(true);
        }

        // Skip when the param type has a base name that can't be loaded as a
        // class and has no namespace separator — it's likely a @phpstan-type /
        // @psalm-type alias that we couldn't expand.  We can't verify
        // compatibility without the underlying type, so suppress to avoid
        // false positives.  Namespaced types (containing `\`) are real class
        // references that should still be checked.
        //
        // The same goes for the argument type — an unexpanded @phpstan-type /
        // @psalm-type alias on the arg side would also cause false positives
        // (e.g. `Payload` passed to `?array`).
        if is_unloadable_short_name(param_type, self.class_loader)
            || is_unloadable_short_name(arg_type, self.class_loader)
        {
            return Some(true);
        }

        // Skip anonymous class arguments.  Anonymous classes are stored
        // with synthetic names (`__anonymous@<offset>`) that are not
        // indexed globally, so the class loader cannot resolve their
        // hierarchy.  They almost always extend/implement the expected
        // type — the developer wrote `new class extends Foo { … }` for
        // exactly that purpose.  MAYBE → stay silent.
        if let Some(base) = arg_type.base_name()
            && base.contains("__anonymous@")
        {
            return Some(true);
        }
        // Skip when arg type is `object` and param expects a specific class.
        // The developer's code may have narrowed the object (instanceof,
        // assert, etc.) before this call site.  We flag `$obj->method()`
        // as unknown-member instead — that's where the developer learns
        // they need better types.  MAYBE → stay silent.
        if arg_type.is_object() && param_type.base_name().is_some() {
            return Some(true);
        }
        None
    }

    /// Callable specifications and the values a `callable` accepts.
    fn callables(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        // ── Callable specification ↔ callable specification ─────────
        // Both sides carry a signature, so the return type — covariant,
        // like any other value the caller receives back — is a real
        // constraint to check.  A closure arrives here with a return type
        // only when one was declared or resolved from its body, and with
        // none at all when neither was possible.
        //
        // The parameters are contravariant: every value the specification
        // promises to pass has to be accepted by the closure's parameter in
        // that position.  An empty `params` on the argument means the list
        // was not recorded (a first-class callable, a bare `Closure`), not
        // "takes nothing", so it stays a MAYBE.
        //
        // This has to come before the bare-`callable` rule below, which
        // treats any two callable-ish types as compatible and would
        // otherwise answer for the pair first.
        if let (TypeKind::Callable(arg_sig), TypeKind::Callable(param_sig)) =
            (arg_type.kind(), param_type.kind())
        {
            if let (Some(arg_return), Some(param_return)) =
                (&arg_sig.return_type, &param_sig.return_type)
                && !self.check(arg_return, param_return)
            {
                return Some(false);
            }
            return Some(
                first_rejected_callable_param(
                    arg_sig,
                    param_sig,
                    self.class_loader,
                    self.strict_types,
                )
                .is_none(),
            );
        }
        // Skip when param type is `callable` (or a callable specification)
        // and arg type is Closure, callable, string, array, or any
        // object-like type (which might implement `__invoke`).  A bare
        // `Closure` or `callable` says nothing about its signature, so a
        // specification cannot be proven unmet by it either.
        if param_type.is_callable()
            && (arg_type.is_callable()
                || arg_type.is_closure()
                || arg_type.is_array_like()
                || arg_type.is_string_subtype()
                || arg_type.is_object_like())
        {
            return Some(true);
        }
        None
    }

    /// Class keywords, which need a class context the type alone does not
    /// carry.
    fn class_keywords(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        // Skip when param or arg type contains `self`, `static`, `$this`, or
        // `parent` anywhere in the type tree (including inside unions like
        // `int|self|string`).  These keywords need class context to resolve
        // and would cause false positives without it.
        if contains_self_or_parent(param_type) || contains_self_or_parent(arg_type) {
            return Some(true);
        }
        None
    }

    /// Nullable, union and intersection types, on either side.
    fn composites(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        // ── Nullable argument handling ───────────────────────────────
        // `?T` is the union `T|null` written short, so it is judged as that
        // union: both halves have to satisfy the parameter, exactly as the
        // rule below demands of a union's members.  Which of the two
        // spellings a value ends up with is an accident of how it was
        // produced — a declared hint and a nullable docblock stay `?T`,
        // while a branch merge, a `??` chain or a resolver join comes out as
        // `T|null` — so judging them by different rules would make the
        // diagnostic fire on where a type came from rather than on what it
        // holds.
        if let TypeKind::Nullable(inner) = arg_type.kind() {
            return Some(self.check(inner, param_type) && self.check(&PhpType::null(), param_type));
        }

        // ── Union argument handling ───────────────────────────────────
        // The runtime value can be any member of the union, so every
        // member must independently satisfy the parameter type for the
        // call to be sound — a single compatible member does not make the
        // others disappear.  Each member is checked through the full
        // `is_type_compatible` recursion (not the stricter structural
        // `is_subtype_of_typed`) so it still benefits from the MAYBE rules
        // above (Stringable, refined scalars, unloadable short names, …).
        if let TypeKind::Union(members) = arg_type.kind() {
            return Some(members.iter().all(|m| self.check(m, param_type)));
        }

        // ── Intersection argument handling ───────────────────────────
        // A value of intersection type `A&B` satisfies *every* member, so
        // it is compatible with the param when *any* member is.  This is
        // the standard subtyping rule for intersections and covers common
        // cases like PHPUnit's `MockObject&Foo` (a mock that is also a Foo)
        // being returned where `Foo` (or a union containing `Foo`) is
        // expected.
        if let TypeKind::Intersection(members) = arg_type.kind()
            && members.iter().any(|m| self.check(m, param_type))
        {
            return Some(true);
        }

        // ── Conservative union parameter handling ────────────────────
        // When the param is a union, accept if the arg is compatible
        // with *any* member.  This extends the structural check to use
        // our full compatibility logic (including MAYBE rules) for each
        // union branch.
        if let TypeKind::Union(members) = param_type.kind()
            && members.iter().any(|m| self.check(arg_type, m))
        {
            return Some(true);
        }

        // ── Conservative intersection parameter handling ─────────────
        // When the param is an intersection `A&B`, the value must satisfy
        // *every* member.  Stay silent unless the arg is definitely
        // incompatible with at least one member — i.e. accept when the arg
        // is compatible with all members we can check.  Combined with the
        // conservative rules above, this avoids false positives on mock
        // types like `MethodNode&MockObject`.
        if let TypeKind::Intersection(members) = param_type.kind()
            && members.iter().all(|m| self.check(arg_type, m))
        {
            return Some(true);
        }
        None
    }

    /// An array shape against a shape or a typed array, and a bare array
    /// against any array.
    fn arrays(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        // ── Array shape → array shape ───────────────────────────────
        // Ahead of the rules below, which read an array by its `kind()`: that
        // is the generic array an unsealed shape widens to, and judging the
        // shape by it would never see the entries it lists.
        if let (Some(arg_shape), Some(param_shape)) =
            (arg_type.shape_parts(), param_type.shape_parts())
        {
            return Some(shape_is_compatible(
                &arg_shape,
                &param_shape,
                self.class_loader,
                self.strict_types,
            ));
        }

        // ── Array shape → typed array ───────────────────────────────
        // `array{id: string, index: string, body: array}` is accepted where
        // `array<string, mixed>` or similar is expected: the shape is a more
        // specific form of the typed array.  Like the rule above, it has to
        // come before the generic-array rules, which an unsealed shape would
        // reach as the array it widens to: that array's `mixed` tail swallows
        // the entries the shape lists.
        if let Some(arg_shape) = arg_type.shape_parts()
            && let Some(demand) = ArrayDemand::of(param_type)
        {
            return Some(shape_fits_array(
                &arg_shape,
                &demand,
                self.class_loader,
                self.strict_types,
            ));
        }

        // ── Bare array ↔ typed array: MAYBE ─────────────────────────
        // A bare `array` is untyped — it *might* satisfy `array<K,V>`,
        // `list<X>`, `T[]`, or an array shape.  We can't prove it wrong.
        // (The reverse — typed array <: bare array — is a strict YES
        // handled by the `is_subtype_of_typed` fallback.)
        if is_bare_array(arg_type) && is_any_array_type(param_type) {
            return Some(true);
        }
        None
    }

    /// Passing `X` where `?X` is expected is always valid.
    fn nullable_param(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        if let TypeKind::Nullable(inner) = param_type.kind()
            && self.check(arg_type, inner)
        {
            return Some(true);
        }
        None
    }

    /// The conversions PHP performs on a value outside `strict_types`.
    fn coercions(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        let strict_types = self.strict_types;
        // ── Stringable objects accepted as string ────────────────────
        // PHP calls __toString() on Stringable objects when a string is
        // expected, but only outside strict_types: under
        // declare(strict_types=1) passing a Stringable object where a
        // `string` parameter is expected is a TypeError. We only accept
        // objects whose class is \Stringable, which PHP makes every class
        // declaring __toString(). For bare `object` types (no class name) we
        // stay permissive since we can't check.
        if !strict_types && param_type.is_string_type() && arg_type.is_object_like() {
            let Some(class_name) = arg_type.base_name() else {
                // Bare `object` type — stay permissive
                return Some(true);
            };
            if let Some(cls) = (self.class_loader)(Fqn::new(class_name))
                && crate::class_lookup::is_subtype_of(&cls, "Stringable", self.class_loader)
            {
                return Some(true);
            }
            // Not Stringable, or not loadable — fall through to other checks
        }

        // ── PHP type juggling: int/float → string ───────────────────
        // PHP coerces ints and floats to strings in many contexts
        // (concatenation,
        // function calls with declare(strict_types=0)).  Under
        // strict_types=1 this is a TypeError, so we flag it.  Also
        // covers numeric literals (e.g. `42` or `1.0` passed to `string`).
        if !strict_types
            && let TypeKind::Named(sup) = param_type.kind()
            && sup.eq_ignore_ascii_case("string")
        {
            let is_numeric_like = match arg_type.kind() {
                TypeKind::Named(sub) => {
                    matches!(
                        sub.to_ascii_lowercase().as_str(),
                        "int" | "integer" | "float" | "double"
                    )
                }
                TypeKind::Literal(l) => {
                    matches!(**l, LiteralValue::Int(_) | LiteralValue::Float(_))
                }
                _ => false,
            };
            if is_numeric_like {
                return Some(true);
            }
        }

        // ── PHP type juggling: numeric-string → float/int[/range] ───
        // PHP coerces numeric strings to numbers in arithmetic and
        // function calls.  Under strict_types=1 string-to-int/float
        // coercion is forbidden.
        if !strict_types {
            let arg_is_numeric_string = match arg_type.kind() {
                TypeKind::Named(sub) => sub.eq_ignore_ascii_case("numeric-string"),
                TypeKind::Literal(l) => {
                    matches!(**l, LiteralValue::String(_)) && l.is_numeric_string()
                }
                _ => false,
            };
            if arg_is_numeric_string {
                match param_type.kind() {
                    TypeKind::Named(sup)
                        if matches!(
                            sup.to_ascii_lowercase().as_str(),
                            "float" | "double" | "int" | "integer" | "numeric"
                        ) =>
                    {
                        return Some(true);
                    }
                    TypeKind::IntRange(min, max)
                        if let Some(lit @ LiteralValue::String(_)) = arg_type.as_literal()
                            && lit
                                .string_content()
                                .and_then(|content| content.parse::<i64>().ok())
                                .is_some_and(|value| {
                                    int_literal_is_within_range(value, min, max)
                                }) =>
                    {
                        return Some(true);
                    }
                    _ => {}
                }
            }
        }

        // ── PHP type juggling: numeric → float/int ──────────────────
        // `numeric` is `int|float|numeric-string`; it can always be
        // coerced to float or int.  Under strict_types=1 the
        // numeric-string component would fail, but int→float is still
        // valid, so we stay silent regardless.
        if let TypeKind::Named(sub) = arg_type.kind()
            && sub.eq_ignore_ascii_case("numeric")
            && let TypeKind::Named(sup) = param_type.kind()
            && matches!(
                sup.to_ascii_lowercase().as_str(),
                "float" | "double" | "int" | "integer"
            )
        {
            return Some(true);
        }

        // ── PHP type juggling: float → int ──────────────────────────
        // `int / int` in PHP resolves to `int|float` (division only
        // stays an int when it divides evenly), so this hatch is what
        // actually gets exercised: the `float` half of that union
        // reaching an `int` position.  Outside `declare(strict_types=1)`
        // PHP truncates the float on the way in rather than raising a
        // TypeError, so there is nothing to report. Under strict_types=1
        // this coercion is forbidden and stays a real mismatch.  Covers
        // both a bare `float` type and a float literal (e.g. `1.5`),
        // since a literal keeps its own `Literal` kind rather than
        // widening to `Named("float")`.
        if !strict_types {
            let arg_is_float = match arg_type.kind() {
                TypeKind::Named(sub) => {
                    matches!(sub.to_ascii_lowercase().as_str(), "float" | "double")
                }
                TypeKind::Literal(l) => matches!(**l, LiteralValue::Float(_)),
                _ => false,
            };
            if arg_is_float
                && let TypeKind::Named(sup) = param_type.kind()
                && matches!(sup.to_ascii_lowercase().as_str(), "int" | "integer")
            {
                return Some(true);
            }
        }
        None
    }

    /// String pseudo-types whose members only the project settles.
    fn string_pseudo_types(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        // ── view-string: any string may name a template ─────────────
        // The Laravel PHPStan extensions' `view-string` is the subset of
        // `string` that names a Blade template, which only the project's
        // templates settle — a fact this layer cannot reach. Every string
        // is accepted here (MAYBE), and the argument diagnostic checks a
        // literal against the view index, where that fact lives, reporting
        // the name rather than the type.
        if param_type.is_view_string()
            && (arg_type.is_string_type() || arg_type.is_string_subtype())
        {
            return Some(true);
        }

        // ── model-property<Model>: string literal validation ────────
        // The Laravel PHPStan extensions' `model-property<Model>` is a string subtype
        // representing the property names of an Eloquent model.  When
        // the param is `model-property<Model>`, non-literal string
        // types are accepted (MAYBE — the string might be a valid
        // property name at runtime).  For string literals, we check
        // against the model's known properties when the class can be
        // loaded.
        if let TypeKind::Generic(g) = param_type.kind()
            && g.name.eq_ignore_ascii_case("model-property")
            && let [model] = g.args.as_slice()
        {
            if let TypeKind::Literal(lit) = arg_type.kind()
                && let Some(prop_name) = lit.string_content()
            {
                let Some(cls) = model
                    .base_name()
                    .and_then(|name| (self.class_loader)(Fqn::new(name)))
                else {
                    return Some(true);
                };
                let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
                    &cls,
                    self.class_loader,
                    crate::virtual_members::active_resolved_class_cache(),
                );
                let found = resolved.properties.iter().any(|p| *p.name == *prop_name)
                    || crate::virtual_members::laravel::where_property::collect_column_names(
                        &resolved,
                    )
                    .iter()
                    .any(|col| *col == *prop_name);
                return Some(found);
            }
            if arg_type.is_string_type() || arg_type.is_string_subtype() {
                return Some(true);
            }
        }
        None
    }

    /// Generic types: iterables, a class with its type arguments,
    /// `class-string<T>`, a subclass of a generic ancestor, and the array
    /// spellings that differ only in arity.
    ///
    /// For a plain class against a plain class only one direction is
    /// compatible: arg <: param (e.g. `Cat` passed to `Animal`), a strict
    /// YES the [`is_subtype_of_typed`] fallback handles.  The reverse (a
    /// supertype value where a subtype is declared) is a downcast and *is*
    /// reported.  It used to be treated as a MAYBE on the grounds that the
    /// value might be the narrower concrete at runtime, which hid a whole
    /// class of genuine mismatches.  Everything that made it look necessary
    /// in real Laravel code was our own imprecision: models' custom Eloquent
    /// collections, and the `view()` helper's contract-typed conditional
    /// return.  Both are resolved precisely now, so the escape hatch is
    /// gone.
    fn generics(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        let class_loader = self.class_loader;
        // ── Array-like / traversable-like → generic iterable/Traversable ─
        // When the param is a generic `iterable<K,V>`, `Traversable<K,V>`,
        // or any interface that extends Traversable (e.g. `Arrayable<K,V>`),
        // we can't verify the generic type arguments covariantly at this
        // phase.  Stay silent (MAYBE) when the base types are compatible.
        if let TypeKind::Generic(g) = param_type.kind()
            && (g.name.eq_ignore_ascii_case("iterable")
                || class_loader(Fqn::new(&g.name)).is_some_and(|cls| {
                    crate::class_lookup::is_subtype_of(&cls, "Traversable", class_loader)
                }))
        {
            // Array-like args always satisfy iterable/Traversable generics.
            if arg_type.is_array_like()
                || matches!(arg_type.kind(), TypeKind::Generic(n) if is_array_like_name(&n.name))
                || matches!(
                    arg_type.kind(),
                    TypeKind::Array(_) | TypeKind::ArrayShape(_)
                )
            {
                return Some(true);
            }
            // Object-like args: check hierarchy for Traversable.
            // Can't load → stay permissive; bare `object` → stay permissive.
            if let Some(class_name) = arg_type.base_name() {
                match class_loader(Fqn::new(class_name)) {
                    Some(cls) => {
                        if crate::class_lookup::is_subtype_of(&cls, "Traversable", class_loader) {
                            return Some(true);
                        }
                    }
                    None => return Some(true),
                }
            } else if arg_type.is_object_like() {
                return Some(true);
            }
        }

        // ── Same-base generic covariance ────────────────────────────
        // When both arg and param are generics with the same base name
        // (e.g. `array<string, Cat|Dog>` vs `array<string, Animal>`),
        // check each type argument covariantly using `is_type_compatible`
        // (which has all our MAYBE rules) rather than falling through to
        // `is_subtype_of_typed` (which uses strict structural subtyping
        // and would carry none of them into the arguments).
        if let (TypeKind::Generic(ga), TypeKind::Generic(gp)) = (arg_type.kind(), param_type.kind())
        {
            let (name_arg, args_arg) = (&ga.name, &ga.args);
            let (name_param, args_param) = (&gp.name, &gp.args);
            let bases_match = name_arg.eq_ignore_ascii_case(name_param)
                || (is_array_like_name(name_arg) && is_array_like_name(name_param));
            if bases_match && args_arg.len() == args_param.len() && !args_arg.is_empty() {
                // Only a class-like base is decided here — array-likes have
                // their own cross-form rules further down (`list<X>` ↔
                // `array<int, X>`, `X[]`) that a verdict at this point would
                // pre-empt.
                let class_like = !is_array_like_name(name_arg) && !is_array_like_name(name_param);
                // A type argument is not a coercion site.  PHP converts the
                // value handed to an `int` parameter, but never the `T` inside
                // a `Box<T>` it receives whole (nor the keys or values inside
                // an array passed whole), so the juggling rules that a missing
                // `strict_types` turns on have nothing to act on between two
                // type arguments: `Box<string>` is no more a `Box<int>` in a
                // lenient file than in a strict one, and `array<int, V>` is no
                // more an `array<string, V>` either.
                let args = self.strictly();
                let all_args_compatible = args_arg
                    .iter()
                    .zip(args_param.iter())
                    .all(|(a, p)| args.check(a, p));
                if all_args_compatible {
                    return Some(true);
                }
                if class_like {
                    // The same class with disjoint type arguments is a
                    // mismatch, and the nominal fallback below cannot see it:
                    // it compares base names, so `Box<string>` reaches
                    // `Box<int>` as `Box` <: `Box`.
                    //
                    // Disjoint, not merely "not a subtype": a class type
                    // argument we inferred is widened to the template's
                    // declared bound whenever nothing bound it, so an
                    // argument that is a *supertype* of the declared one says
                    // we learned too little, not that the value contradicts
                    // the annotation.  It also leaves
                    // `@template-contravariant` alone, where the wider
                    // argument is the correct one.  Both of those are the
                    // `true` here rather than a fall-through, so that the
                    // pair is answered on its arguments either way and the
                    // nominal name match never gets to speak for it.
                    return Some(!args_arg.iter().zip(args_param.iter()).any(|(a, p)| {
                        !args.check(a, p) && !args.check(p, a) && !may_name_same_class(a, p)
                    }));
                }
            }
        }

        // ── class-string<A> → class-string<B> ───────────────────────
        // The name of an `A` is the name of a `B` exactly when every `A` is a
        // `B`, so the inner types decide it, MAYBE rules included: an
        // enclosing template's `class-string<Collector<TNode, TValue>>` still
        // says nothing that contradicts `class-string<Collector<Node, mixed>>`.
        if let (TypeKind::ClassString(Some(arg_inner)), TypeKind::ClassString(Some(param_inner))) =
            (arg_type.kind(), param_type.kind())
        {
            return Some(self.strictly().check(arg_inner, param_inner));
        }

        // ── Subclass → generic ancestor ─────────────────────────────
        // `IntBox` reaches a `Box<string>` parameter with no type arguments
        // of its own, so the rule above never sees it and the nominal
        // fallback would only ask whether `IntBox` extends `Box`.  What its
        // `@extends Box<int>` says is the `Box` it is, and that is what the
        // parameter has to accept.
        if let TypeKind::Generic(gp) = param_type.kind()
            && !is_array_like_name(&gp.name)
            && let Some(as_ancestor) = as_generic_ancestor(arg_type, &gp.name, class_loader)
        {
            return Some(self.check(&as_ancestor, param_type));
        }

        self.array_arities(arg_type, param_type)
    }

    /// The array spellings that differ only in how many type arguments
    /// they write, or in whether they promise list keys.
    fn array_arities(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        // ── list<X> ↔ array<int, X>: MAYBE ─────────────────────────
        // A list is an array with sequential int keys starting at 0.
        // In practice, PHP codebases use these interchangeably and we
        // can't verify the structural guarantee statically.
        if let (TypeKind::Generic(ga), TypeKind::Generic(gp)) = (arg_type.kind(), param_type.kind())
        {
            let (name_arg, args_arg) = (&ga.name, ga.args.as_slice());
            let (name_param, args_param) = (&gp.name, gp.args.as_slice());
            let arg_is_list = name_arg.eq_ignore_ascii_case("list");
            let param_is_list = name_param.eq_ignore_ascii_case("list");
            let arg_is_array = is_array_like_name(name_arg) && !arg_is_list;
            let param_is_array = is_array_like_name(name_param) && !param_is_list;

            let fits = match (args_arg, args_param) {
                // list<X> → array<int, X>
                ([arg_value], [_, param_value]) if arg_is_list && param_is_array => {
                    self.check(arg_value, param_value)
                }
                // array<int, X> → list<X>
                ([_, arg_value], [param_value]) if arg_is_array && param_is_list => {
                    self.check(arg_value, param_value)
                }
                // ── array<K, V> → array<V> (2-param to 1-param) ────────
                // `array<V>` means `array<array-key, V>`.  A 2-param array is
                // more specific — its value type always fits in the 1-param
                // form as long as the value types are compatible.  The strict
                // form of that is `is_subtype_of_typed`'s; what this adds is
                // the MAYBE rules in the value position, so an
                // `array<string, string>` still reaches a
                // `array<non-empty-string>` parameter.
                ([_, arg_value], [param_value]) if arg_is_array && param_is_array => {
                    self.check(arg_value, param_value)
                }
                // ── array<V> → array<K, V> (1-param to 2-param): MAYBE ─
                // `array<V>` is `array<mixed, V>` — we don't know the key
                // type, so it *might* satisfy `array<K, V>` at runtime.
                ([arg_value], [_, param_value]) if arg_is_array && param_is_array => {
                    self.check(arg_value, param_value)
                }
                _ => false,
            };
            if fits {
                return Some(true);
            }
        }

        // ── X[] (Array slice) semantics ─────────────────────────────
        // `X[]` in PHPDoc means `array<mixed, X>` — an array with
        // unknown key type and value type X.  It is NOT `list<X>`,
        // which additionally guarantees sequential int keys from 0.
        //
        // Therefore:
        //   array<K, V> → V[]  is YES  (more specific → less specific)
        //   V[] → array<K, V>  is MAYBE (we don't know the key type)
        //   list<X> → X[]      is YES  (list is more specific than array)
        //   X[] → list<X>      is MAYBE (X[] might have gaps / non-int keys)
        let mixed = PhpType::mixed();
        match (arg_type.kind(), param_type.kind()) {
            // ── Generic array → X[] ─────────────────────────────────────
            // `array<int, string>` → `string[]` — the value type of the
            // generic form is more specific than (or equal to) the slice.
            // `list<X>` is one of these too: it is array-like, and its single
            // argument is its value type.  As with the arity rules above, the
            // strict form lives in `is_subtype_of_typed`; this one carries the
            // MAYBE rules into the value position.
            (TypeKind::Generic(g), TypeKind::Array(inner))
                if is_array_like_name(&g.name)
                    && self.check(g.args.last().unwrap_or(&mixed), inner) =>
            {
                Some(true)
            }
            // ── X[] → Generic array : MAYBE ─────────────────────────────
            // `string[]` → `array<int, string>` — the key type is unknown,
            // it *might* be int at runtime.  This covers `X[] → list<X>`
            // too: `X[]` is `array<mixed, X>`, which might have
            // non-sequential keys, so it might not be a valid list, and we
            // stay silent.
            (TypeKind::Array(inner), TypeKind::Generic(g))
                if is_array_like_name(&g.name)
                    && self.check(inner, g.args.last().unwrap_or(&mixed)) =>
            {
                Some(true)
            }
            _ => None,
        }
    }

    /// Classes that stand in for an array, and arrays that may hold the
    /// keys a shape demands.
    fn array_substitutes(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        let class_loader = self.class_loader;
        // ── Arrayable/ArrayAccess arg → bare array: MAYBE ───────────
        // Many collection-like types implement ArrayAccess or Arrayable
        // and are used interchangeably with arrays.  When the arg type
        // implements one of these interfaces and the param is bare
        // `array`, stay silent.  Note: Traversable alone does NOT
        // qualify — a Traversable is not substitutable for `array`
        // (e.g. you cannot pass a Collection to array_map()).
        if is_bare_array(param_type)
            && let Some(class_name) = arg_type.base_name()
            && let Some(cls) = class_loader(Fqn::new(class_name))
            && (crate::class_lookup::is_subtype_of(&cls, "ArrayAccess", class_loader)
                || crate::class_lookup::is_subtype_of(&cls, "Arrayable", class_loader))
        {
            return Some(true);
        }

        // ── Typed array → ArrayShape: MAYBE ─────────────────────────
        // `array<string, mixed>` or `array{index: string}` (fewer keys)
        // passed where a specific shape is expected.  The actual array
        // might have the right keys at runtime.  We can't prove
        // otherwise from the type alone.
        if matches!(param_type.kind(), TypeKind::ArrayShape(_)) && is_any_array_type(arg_type) {
            return Some(true);
        }
        None
    }

    /// `object{foo: int}` is a structural constraint: any class or object
    /// literal with a public `foo` property assignable to `int` satisfies
    /// it, regardless of its class hierarchy. Extra properties on the
    /// argument are harmless — object shapes are structurally open, like
    /// array shapes.  MAYBE when the argument's class cannot be loaded.
    fn object_shapes(self, arg_type: &PhpType, param_type: &PhpType) -> Option<bool> {
        let TypeKind::ObjectShape(param_entries) = param_type.kind() else {
            return None;
        };
        if let TypeKind::ObjectShape(arg_entries) = arg_type.kind() {
            let all_param_keys_satisfied = param_entries.iter().all(|pe| {
                if pe.optional {
                    return arg_entries
                        .iter()
                        .find(|ae| ae.key == pe.key)
                        .is_none_or(|ae| self.check(&ae.value_type, &pe.value_type));
                }
                arg_entries
                    .iter()
                    .any(|ae| ae.key == pe.key && self.check(&ae.value_type, &pe.value_type))
            });
            return all_param_keys_satisfied.then_some(true);
        }
        let class_name = arg_type.base_name()?;
        if crate::php_type::is_builtin_non_class_type(class_name) || is_array_like_name(class_name)
        {
            return None;
        }
        // A class we can't load (likely unindexed vendor code) — stay
        // permissive rather than risk a false positive.
        let Some(cls) = (self.class_loader)(Fqn::new(class_name)) else {
            return Some(true);
        };
        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            self.class_loader,
            crate::virtual_members::active_resolved_class_cache(),
        );
        let all_param_keys_satisfied = param_entries.iter().all(|pe| {
            let Some(key) = pe.key.as_deref() else {
                return pe.optional;
            };
            match resolved
                .properties
                .iter()
                .find(|p| p.visibility == Visibility::Public && &*p.name == key)
            {
                Some(p) => p
                    .type_hint
                    .as_ref()
                    .is_none_or(|t| self.check(t, &pe.value_type)),
                None => pe.optional,
            }
        });
        all_param_keys_satisfied.then_some(true)
    }
}

/// Returns `true` when the type is any form of array (bare, generic,
/// slice, or shape).  Used by the bare-array MAYBE rules to check
/// inside unions as well as at the top level.
fn is_any_array_type(ty: &PhpType) -> bool {
    matches!(ty.kind(), TypeKind::Array(_) | TypeKind::ArrayShape(_))
        || matches!(ty.kind(), TypeKind::Generic(g) if is_array_like_name(&g.name))
        || is_bare_array(ty)
}

/// Deep check: returns `true` when the type contains `self`, `static`,
/// `$this`, or `parent` anywhere in its structure — including inside
/// union/intersection members, nullable wrappers, and generic arguments.
fn contains_self_or_parent(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(s) => {
            let low = s.to_ascii_lowercase();
            matches!(low.as_str(), "self" | "static" | "$this" | "parent")
        }
        TypeKind::Nullable(inner) => contains_self_or_parent(inner),
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            members.iter().any(contains_self_or_parent)
        }
        TypeKind::Generic(g) => {
            let low = g.name.to_ascii_lowercase();
            matches!(low.as_str(), "self" | "static" | "$this" | "parent")
                || g.args.iter().any(contains_self_or_parent)
        }
        TypeKind::ClassString(inner) | TypeKind::InterfaceString(inner) => {
            inner.as_ref().is_some_and(contains_self_or_parent)
        }
        _ => false,
    }
}
