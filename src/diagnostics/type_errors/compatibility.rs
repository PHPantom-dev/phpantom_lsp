//! Type-compatibility policy layer used by the argument, return, and
//! property type-mismatch diagnostics.
//!
//! [`is_type_compatible`] sits on top of the core type system
//! (`crate::class_lookup::is_subtype_of_typed`). It adds diagnostic
//! policy — conservative "MAYBE" escape hatches that suppress a
//! diagnostic whenever the types *might* be compatible at runtime but
//! cannot be proven so statically — on top of strict subtype checks.
//! See the "Architecture note" on [`is_type_compatible`] for the
//! distinction between the two.

use std::sync::Arc;

use crate::class_lookup::is_subtype_of_typed;
use crate::php_type::{
    LiteralValue, PhpType, ShapeEntry, TypeKind, int_literal_is_within_range, is_array_like_name,
};
use crate::types::{ClassInfo, Visibility};

/// Returns `true` when the type names a class by an unqualified short name
/// the project cannot load.  Covers both `Foo` and `Foo<Bar>`: a generic
/// whose base name is unresolvable is just as unverifiable as a plain one.
fn is_unloadable_short_name(
    ty: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    let name = match ty.kind() {
        TypeKind::Named(n) => n.as_str(),
        TypeKind::Generic(g) => g.name.as_str(),
        _ => return false,
    };
    !name.contains('\\')
        && !crate::php_type::is_builtin_non_class_type(name)
        && !is_array_like_name(name)
        && class_loader(name).is_none()
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
fn is_bare_array(ty: &PhpType) -> bool {
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
pub(crate) fn missing_required_shape_keys(arg_type: &PhpType, param_type: &PhpType) -> Vec<String> {
    let (TypeKind::ArrayShape(arg_entries), TypeKind::ArrayShape(param_entries)) =
        (arg_type.kind(), param_type.kind())
    else {
        return Vec::new();
    };

    let arg_keys = shape_keys(arg_entries);
    shape_keys(param_entries)
        .into_iter()
        .zip(param_entries.iter())
        .filter(|(key, entry)| !entry.optional && !arg_keys.contains(key))
        .map(|(key, _)| key)
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

/// Check if an argument type is compatible with a parameter type.
///
/// Returns `true` if the argument type can be passed to the parameter
/// without a type error.  Conservative: returns `true` (compatible)
/// when in doubt.
pub(crate) fn is_type_compatible(
    arg_type: &PhpType,
    param_type: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    strict_types: bool,
) -> bool {
    // ── Architecture note ───────────────────────────────────────
    //
    // This function is a diagnostic-policy layer on top of the core
    // type system.  It contains two kinds of checks:
    //
    // 1. **MAYBE escape hatches** — cases where the types *might*
    //    be compatible at runtime but we can't prove it statically.
    //    These return `true` (suppress the diagnostic) to avoid
    //    false positives.  They encode diagnostic policy, not type
    //    theory, and are unique to this function.
    //
    // 2. **Hierarchy checks with permissive fallbacks** — nominal
    //    subtype checks that fall back to `true` when a class can't
    //    be loaded, rather than producing a false positive.
    //
    // Strict subtype relationships (Cat <: Animal, never <: T,
    // Closure(int): void <: callable, array<int, string> <: array,
    // Collection<Cat> <: object, ArrayIterator <: iterable,
    // array<int, Cat> <: Animal[]) are handled by the
    // `is_subtype_of_typed` fallback at the end of this function and
    // should NOT be duplicated here.  A rule that recurses into
    // `is_type_compatible` for a nested position is not a duplicate:
    // it carries the MAYBE rules into that position, which the core
    // engine deliberately does not.
    //
    // A future pass may tighten specific MAYBE relationships (reporting
    // them as mismatches rather than letting them pass) once we are
    // confident the narrowing does not introduce false positives.  Each
    // one that goes has to be paid for with resolver precision first:
    // see the class-hierarchy section below for how the supertype-where-
    // subtype hatch was retired.

    // A benevolent value came out of a builtin whose failure branch nobody
    // checks (`tempnam()`, `curl_init()`, `Redis::get()`, …), so the union
    // is satisfied as soon as one branch fits — the failure branch alone is
    // not worth a diagnostic.  See `crate::benevolent_builtins` for why, and
    // note that this relaxes nothing outside the diagnostics: the value
    // keeps its full union everywhere else, so `=== false` still narrows and
    // still reads as a live comparison.
    if arg_type.is_benevolent() {
        let stripped = arg_type.strip_benevolence();
        let members: &[PhpType] = match stripped.kind() {
            TypeKind::Union(members) => members,
            _ => return is_type_compatible(&stripped, param_type, class_loader, strict_types),
        };
        return members
            .iter()
            .any(|member| is_type_compatible(member, param_type, class_loader, strict_types));
    }
    // A benevolent *parameter* is the same bargain read from the other side:
    // it accepts whatever any of its branches accepts.
    if param_type.is_benevolent() {
        return is_type_compatible(
            arg_type,
            &param_type.strip_benevolence(),
            class_loader,
            strict_types,
        );
    }

    // A conditional that survived resolution (the call-site arguments that
    // would decide it were not available) is a type expression, not a set
    // of values — comparing a concrete type against it can only fail.
    // Compare against the union of its branches instead, which is what the
    // value satisfies however the condition resolves.
    if arg_type.contains_conditional() || param_type.contains_conditional() {
        return is_type_compatible(
            &arg_type.conditionals_as_branch_unions(),
            &param_type.conditionals_as_branch_unions(),
            class_loader,
            strict_types,
        );
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
        return true;
    }
    if param_type.contains_unevaluated_operator() {
        return is_type_compatible(
            arg_type,
            &param_type.unevaluated_operators_as_bounds(),
            class_loader,
            strict_types,
        );
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
        return is_type_compatible(
            &expand(arg_type),
            &expand(param_type),
            class_loader,
            strict_types,
        );
    }

    // Skip if either type is unresolved/unknown.
    if arg_type.is_untyped() || param_type.is_untyped() {
        return true;
    }
    if arg_type.is_mixed() || param_type.is_mixed() {
        return true;
    }
    // When the argument is a union that contains `mixed`, bail out
    // conservatively.  `mixed` means "we don't know" — the actual
    // runtime value could satisfy the parameter type, so reporting
    // an error would be a false positive.
    if let TypeKind::Union(members) = arg_type.kind()
        && members.iter().any(|m| m.is_mixed())
    {
        return true;
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
        return true;
    }
    // Skip Raw types (unparseable / unresolved type strings).
    if matches!(arg_type.kind(), TypeKind::Raw(_)) || matches!(param_type.kind(), TypeKind::Raw(_))
    {
        return true;
    }

    // Skip when the param type has a base name that can't be loaded as a
    // class and has no namespace separator — it's likely a @phpstan-type /
    // @psalm-type alias that we couldn't expand.  We can't verify
    // compatibility without the underlying type, so suppress to avoid
    // false positives.  Namespaced types (containing `\`) are real class
    // references that should still be checked.
    if is_unloadable_short_name(param_type, class_loader) {
        return true;
    }

    // Same escape hatch for the argument type — an unexpanded
    // @phpstan-type / @psalm-type alias on the arg side would also
    // cause false positives (e.g. `Payload` passed to `?array`).
    if is_unloadable_short_name(arg_type, class_loader) {
        return true;
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
        return true;
    }
    // Skip when arg type is `object` and param expects a specific class.
    // The developer's code may have narrowed the object (instanceof,
    // assert, etc.) before this call site.  We flag `$obj->method()`
    // as unknown-member instead — that's where the developer learns
    // they need better types.  MAYBE → stay silent.
    if arg_type.is_object() && param_type.base_name().is_some() {
        return true;
    }
    // ── Callable specification ↔ callable specification ─────────
    // Both sides carry a signature, so the return type — covariant,
    // like any other value the caller receives back — is a real
    // constraint to check.  It is also the half we can trust: a closure
    // arrives here with a return type only when one was declared or
    // resolved from its body, and with none at all when neither was
    // possible.  Its parameter list is not recorded on the resolved type
    // at all, so an empty `params` means "unknown", not "takes nothing",
    // and parameters therefore stay a MAYBE.
    //
    // This has to come before the bare-`callable` rule below, which
    // treats any two callable-ish types as compatible and would
    // otherwise answer for the pair first.
    if let (TypeKind::Callable(arg_sig), TypeKind::Callable(param_sig)) =
        (arg_type.kind(), param_type.kind())
    {
        return match (&arg_sig.return_type, &param_sig.return_type) {
            (Some(arg_return), Some(param_return)) => {
                is_type_compatible(arg_return, param_return, class_loader, strict_types)
            }
            _ => true,
        };
    }
    // Skip when param type is `callable` and arg type is Closure, callable, string, array,
    // or any object-like type (which might implement `__invoke`).
    if param_type.is_callable()
        && (arg_type.is_callable()
            || arg_type.is_closure()
            || arg_type.is_array_like()
            || arg_type.is_string_subtype()
            || arg_type.is_object_like())
    {
        return true;
    }
    // Skip when param or arg type contains `self`, `static`, `$this`, or
    // `parent` anywhere in the type tree (including inside unions like
    // `int|self|string`).  These keywords need class context to resolve
    // and would cause false positives without it.
    if contains_self_or_parent(param_type) || contains_self_or_parent(arg_type) {
        return true;
    }

    // ── Refined scalar subtypes: stay silent when uncertain ──────
    // `string` passed to `non-empty-string`, `int` to `positive-int`,
    // etc.  The base type *might* satisfy the refinement at runtime
    // and we can't prove otherwise from the type alone.  Only the
    // reverse (e.g. `non-empty-string` passed to `string`) is safe
    // to accept unconditionally — and `is_subtype_of` already does.
    if is_refined_scalar_pair(arg_type, param_type) {
        return true;
    }

    // ── int → int<min..max>: MAYBE ──────────────────────────────
    // A bare `int` *might* satisfy any int range constraint at
    // runtime.  We cannot prove otherwise from the type alone.
    if matches!(param_type.kind(), TypeKind::IntRange(..))
        && let TypeKind::Named(sub) = arg_type.kind()
        && matches!(sub.to_ascii_lowercase().as_str(), "int" | "integer")
    {
        return true;
    }

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
        return is_type_compatible(inner, param_type, class_loader, strict_types)
            && is_type_compatible(&PhpType::null(), param_type, class_loader, strict_types);
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
        return members
            .iter()
            .all(|m| is_type_compatible(m, param_type, class_loader, strict_types));
    }

    // ── Intersection argument handling ───────────────────────────
    // A value of intersection type `A&B` satisfies *every* member, so
    // it is compatible with the param when *any* member is.  This is
    // the standard subtyping rule for intersections and covers common
    // cases like PHPUnit's `MockObject&Foo` (a mock that is also a Foo)
    // being returned where `Foo` (or a union containing `Foo`) is
    // expected.
    if let TypeKind::Intersection(members) = arg_type.kind()
        && members
            .iter()
            .any(|m| is_type_compatible(m, param_type, class_loader, strict_types))
    {
        return true;
    }

    // ── Conservative union parameter handling ────────────────────
    // When the param is a union, accept if the arg is compatible
    // with *any* member.  This extends the structural check to use
    // our full compatibility logic (including MAYBE rules) for each
    // union branch.
    if let TypeKind::Union(members) = param_type.kind()
        && members
            .iter()
            .any(|m| is_type_compatible(arg_type, m, class_loader, strict_types))
    {
        return true;
    }

    // ── Conservative intersection parameter handling ─────────────
    // When the param is an intersection `A&B`, the value must satisfy
    // *every* member.  Stay silent unless the arg is definitely
    // incompatible with at least one member — i.e. accept when the arg
    // is compatible with all members we can check.  Combined with the
    // conservative rules above, this avoids false positives on mock
    // types like `MethodNode&MockObject`.
    if let TypeKind::Intersection(members) = param_type.kind()
        && members
            .iter()
            .all(|m| is_type_compatible(arg_type, m, class_loader, strict_types))
    {
        return true;
    }

    // ── Bare Closure/callable → callable specification: MAYBE ───
    // When the param is a callable specification like
    // `Closure(Builder<X>): mixed` and the arg is a bare `Closure`
    // or `callable`, we can't verify the signature — stay silent.
    // (The reverse direction — callable spec <: bare Closure — is a
    // strict YES handled by the `is_subtype_of_typed` fallback.)
    if matches!(param_type.kind(), TypeKind::Callable { .. })
        && (arg_type.is_closure() || arg_type.is_callable())
    {
        return true;
    }

    // ── Bare array ↔ typed array: MAYBE ─────────────────────────
    // A bare `array` is untyped — it *might* satisfy `array<K,V>`,
    // `list<X>`, `T[]`, or an array shape.  We can't prove it wrong.
    // (The reverse — typed array <: bare array — is a strict YES
    // handled by the `is_subtype_of_typed` fallback.)
    if is_bare_array(arg_type) && is_any_array_type(param_type) {
        return true;
    }

    // ── Non-nullable arg → nullable param: YES ──────────────────
    // Passing `X` where `?X` is expected is always valid.
    if let TypeKind::Nullable(inner) = param_type.kind()
        && is_type_compatible(arg_type, inner, class_loader, strict_types)
    {
        return true;
    }

    // ── Stringable objects accepted as string ────────────────────
    // PHP calls __toString() on Stringable objects when a string is
    // expected, but only outside strict_types: under
    // declare(strict_types=1) passing a Stringable object where a
    // `string` parameter is expected is a TypeError. We only accept
    // objects whose class implements \Stringable or declares
    // __toString(). For bare `object` types (no class name) we stay
    // permissive since we can't check.
    if !strict_types && param_type.is_string_type() && arg_type.is_object_like() {
        if let Some(class_name) = arg_type.base_name() {
            if let Some(cls) = class_loader(class_name) {
                let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                    &cls,
                    class_loader,
                    crate::virtual_members::active_resolved_class_cache(),
                );
                let implements_stringable =
                    crate::class_lookup::is_subtype_of(&cls, "Stringable", class_loader);
                let has_to_string = merged.get_method_ci("__toString").is_some();
                if implements_stringable || has_to_string {
                    return true;
                }
                // Class loaded but doesn't implement Stringable — fall through
            }
            // Class can't be loaded — fall through to other checks
        } else {
            // Bare `object` type — stay permissive
            return true;
        }
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
            TypeKind::Literal(l) => matches!(**l, LiteralValue::Int(_) | LiteralValue::Float(_)),
            _ => false,
        };
        if is_numeric_like {
            return true;
        }
    }

    // ── PHP type juggling: numeric-string → float/int[/range] ───
    // PHP coerces numeric strings to numbers in arithmetic and
    // function calls.  Under strict_types=1 string-to-int/float
    // coercion is forbidden.
    if !strict_types {
        let arg_is_numeric_string = match arg_type.kind() {
            TypeKind::Named(sub) => sub.eq_ignore_ascii_case("numeric-string"),
            TypeKind::Literal(l) => matches!(**l, LiteralValue::String(_)) && l.is_numeric_string(),
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
                    return true;
                }
                TypeKind::IntRange(min, max)
                    if let Some(lit @ LiteralValue::String(_)) = arg_type.as_literal()
                        && lit
                            .string_content()
                            .and_then(|content| content.parse::<i64>().ok())
                            .is_some_and(|value| int_literal_is_within_range(value, min, max)) =>
                {
                    return true;
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
        return true;
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
            TypeKind::Named(sub) => matches!(sub.to_ascii_lowercase().as_str(), "float" | "double"),
            TypeKind::Literal(l) => matches!(**l, LiteralValue::Float(_)),
            _ => false,
        };
        if arg_is_float
            && let TypeKind::Named(sup) = param_type.kind()
            && matches!(sup.to_ascii_lowercase().as_str(), "int" | "integer")
        {
            return true;
        }
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
        && g.args.len() == 1
    {
        let args = &g.args;
        if let TypeKind::Literal(lit) = arg_type.kind()
            && let Some(prop_name) = lit.string_content()
        {
            if let Some(model_name) = args[0].base_name()
                && let Some(cls) = class_loader(model_name)
            {
                let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
                    &cls,
                    class_loader,
                    crate::virtual_members::active_resolved_class_cache(),
                );
                let found = resolved.properties.iter().any(|p| *p.name == *prop_name)
                    || crate::virtual_members::laravel::where_property::collect_column_names(&cls)
                        .iter()
                        .any(|col| *col == *prop_name);
                return found;
            }
            return true;
        }
        if arg_type.is_string_type() || arg_type.is_string_subtype() {
            return true;
        }
    }

    // ── Array-like / traversable-like → generic iterable/Traversable ─
    // When the param is a generic `iterable<K,V>`, `Traversable<K,V>`,
    // or any interface that extends Traversable (e.g. `Arrayable<K,V>`),
    // we can't verify the generic type arguments covariantly at this
    // phase.  Stay silent (MAYBE) when the base types are compatible.
    if let TypeKind::Generic(g) = param_type.kind()
        && (g.name.eq_ignore_ascii_case("iterable")
            || class_loader(&g.name).is_some_and(|cls| {
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
            return true;
        }
        // Object-like args: check hierarchy for Traversable.
        // Can't load → stay permissive; bare `object` → stay permissive.
        if let Some(class_name) = arg_type.base_name() {
            if let Some(cls) = class_loader(class_name) {
                if crate::class_lookup::is_subtype_of(&cls, "Traversable", class_loader) {
                    return true;
                }
            } else {
                return true;
            }
        } else if arg_type.is_object_like() {
            return true;
        }
    }

    // ── Same-base generic covariance ────────────────────────────
    // When both arg and param are generics with the same base name
    // (e.g. `array<string, HtmlString|string>` vs `array<string, string>`),
    // check each type argument covariantly using `is_type_compatible`
    // (which has all our MAYBE rules) rather than falling through to
    // `is_subtype_of_typed` (which uses strict structural subtyping
    // and misses Stringable, type juggling, etc.).
    if let (TypeKind::Generic(ga), TypeKind::Generic(gp)) = (arg_type.kind(), param_type.kind()) {
        let (name_arg, args_arg) = (&ga.name, &ga.args);
        let (name_param, args_param) = (&gp.name, &gp.args);
        let base_arg = name_arg.to_ascii_lowercase();
        let base_param = name_param.to_ascii_lowercase();
        let bases_match = base_arg == base_param
            || (is_array_like_name(name_arg) && is_array_like_name(name_param));
        if bases_match && args_arg.len() == args_param.len() && !args_arg.is_empty() {
            // Only a class-like base is decided here — array-likes have
            // their own cross-form rules further down (`list<X>` ↔
            // `array<int, X>`, `X[]`) that a verdict at this point would
            // pre-empt.
            let class_like = !is_array_like_name(name_arg) && !is_array_like_name(name_param);
            // A type argument is not a coercion site.  PHP converts the
            // value handed to an `int` parameter, but never the `T` inside
            // a `Box<T>` it receives whole, so the juggling rules that a
            // missing `strict_types` turns on have nothing to act on
            // between two type arguments: `Box<string>` is no more a
            // `Box<int>` in a lenient file than in a strict one.
            let args_strict = strict_types || class_like;
            let all_args_compatible = args_arg
                .iter()
                .zip(args_param.iter())
                .all(|(a, p)| is_type_compatible(a, p, class_loader, args_strict));
            if all_args_compatible {
                return true;
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
                return !args_arg.iter().zip(args_param.iter()).any(|(a, p)| {
                    !is_type_compatible(a, p, class_loader, args_strict)
                        && !is_type_compatible(p, a, class_loader, args_strict)
                        && !may_name_same_class(a, p)
                });
            }
        }
    }

    // ── list<X> ↔ array<int, X>: MAYBE ─────────────────────────
    // A list is an array with sequential int keys starting at 0.
    // In practice, PHP codebases use these interchangeably and we
    // can't verify the structural guarantee statically.
    if let (TypeKind::Generic(ga), TypeKind::Generic(gp)) = (arg_type.kind(), param_type.kind()) {
        let (name_arg, args_arg) = (&ga.name, &ga.args);
        let (name_param, args_param) = (&gp.name, &gp.args);
        let arg_is_list = name_arg.eq_ignore_ascii_case("list");
        let param_is_list = name_param.eq_ignore_ascii_case("list");
        let arg_is_array = is_array_like_name(name_arg) && !arg_is_list;
        let param_is_array = is_array_like_name(name_param) && !param_is_list;

        // list<X> → array<int, X>
        if arg_is_list
            && param_is_array
            && args_arg.len() == 1
            && args_param.len() == 2
            && is_type_compatible(&args_arg[0], &args_param[1], class_loader, strict_types)
        {
            return true;
        }
        // array<int, X> → list<X>
        if arg_is_array
            && param_is_list
            && args_arg.len() == 2
            && args_param.len() == 1
            && is_type_compatible(&args_arg[1], &args_param[0], class_loader, strict_types)
        {
            return true;
        }

        // ── array<K, V> → array<V> (2-param to 1-param) ────────
        // `array<V>` means `array<array-key, V>`.  A 2-param array is
        // more specific — its value type always fits in the 1-param
        // form as long as the value types are compatible.  The strict
        // form of that is `is_subtype_of_typed`'s; what this adds is
        // the MAYBE rules in the value position, so an
        // `array<string, string>` still reaches a
        // `array<non-empty-string>` parameter.
        if arg_is_array
            && param_is_array
            && args_arg.len() == 2
            && args_param.len() == 1
            && is_type_compatible(&args_arg[1], &args_param[0], class_loader, strict_types)
        {
            return true;
        }

        // ── array<V> → array<K, V> (1-param to 2-param): MAYBE ─
        // `array<V>` is `array<mixed, V>` — we don't know the key
        // type, so it *might* satisfy `array<K, V>` at runtime.
        if arg_is_array
            && param_is_array
            && args_arg.len() == 1
            && args_param.len() == 2
            && is_type_compatible(&args_arg[0], &args_param[1], class_loader, strict_types)
        {
            return true;
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

    // ── Generic array → X[] ─────────────────────────────────────
    // `array<int, string>` → `string[]` — the value type of the
    // generic form is more specific than (or equal to) the slice.
    // `list<X>` is one of these too: it is array-like, and its single
    // argument is its value type.  As with the arity rules above, the
    // strict form lives in `is_subtype_of_typed`; this one carries the
    // MAYBE rules into the value position.
    if let TypeKind::Generic(g) = arg_type.kind()
        && is_array_like_name(&g.name)
        && let TypeKind::Array(inner) = param_type.kind()
    {
        let mixed = PhpType::mixed();
        let val = g.args.last().unwrap_or(&mixed);
        if is_type_compatible(val, inner, class_loader, strict_types) {
            return true;
        }
    }

    // ── X[] → Generic array : MAYBE ─────────────────────────────
    // `string[]` → `array<int, string>` — the key type is unknown,
    // it *might* be int at runtime.
    if let TypeKind::Array(inner) = arg_type.kind()
        && let TypeKind::Generic(g) = param_type.kind()
        && is_array_like_name(&g.name)
    {
        let mixed = PhpType::mixed();
        let val = g.args.last().unwrap_or(&mixed);
        if is_type_compatible(inner, val, class_loader, strict_types) {
            return true;
        }
    }

    // ── X[] → list<X> : MAYBE ──────────────────────────────────
    // `X[]` is `array<mixed, X>` — it might have non-sequential
    // keys, so it might not be a valid list.  Stay silent.
    if let TypeKind::Array(inner) = arg_type.kind()
        && let TypeKind::Generic(g) = param_type.kind()
        && g.name.eq_ignore_ascii_case("list")
        && g.args.len() == 1
        && is_type_compatible(inner, &g.args[0], class_loader, strict_types)
    {
        return true;
    }

    // ── Class hierarchy ─────────────────────────────────────────
    //
    // Only one direction is compatible: arg <: param (e.g. `Cat`
    // passed to `Animal`), a strict YES handled by the
    // `is_subtype_of_typed` fallback at the end of this function.
    //
    // The reverse direction (param <: arg — a supertype value where a
    // subtype is declared) is a downcast and *is* reported.  It used to
    // be treated as a MAYBE on the grounds that the value might be the
    // narrower concrete at runtime, which hid a whole class of genuine
    // mismatches.  Everything that made it look necessary in real
    // Laravel code was our own imprecision: models' custom Eloquent
    // collections, and the `view()` helper's contract-typed conditional
    // return.  Both are resolved precisely now, so the escape hatch is
    // gone.

    // ── Arrayable/ArrayAccess arg → bare array: MAYBE ───────────
    // Many collection-like types implement ArrayAccess or Arrayable
    // and are used interchangeably with arrays.  When the arg type
    // implements one of these interfaces and the param is bare
    // `array`, stay silent.  Note: Traversable alone does NOT
    // qualify — a Traversable is not substitutable for `array`
    // (e.g. you cannot pass a Collection to array_map()).
    if is_bare_array(param_type)
        && let Some(class_name) = arg_type.base_name()
        && let Some(cls) = class_loader(class_name)
    {
        let is_array_like = crate::class_lookup::is_subtype_of(&cls, "ArrayAccess", class_loader)
            || crate::class_lookup::is_subtype_of(&cls, "Arrayable", class_loader);
        if is_array_like {
            return true;
        }
    }

    // ── Array shape superset → subset: MAYBE ────────────────────
    // When an array shape has extra keys beyond what the parameter
    // shape requires, the extra keys are harmless.  PHP array shapes
    // are open by convention in most codebases.
    // Optional keys in the param shape (marked with `?`) do not need
    // to be present in the arg shape — they are, by definition,
    // not required.
    if let (TypeKind::ArrayShape(arg_entries), TypeKind::ArrayShape(param_entries)) =
        (arg_type.kind(), param_type.kind())
    {
        let all_param_keys_satisfied = param_entries.iter().all(|pe| {
            // Optional param keys don't need to appear in the arg.
            if pe.optional {
                // If present, check value compatibility; if absent, fine.
                return arg_entries
                    .iter()
                    .find(|ae| ae.key == pe.key)
                    .is_none_or(|ae| {
                        is_type_compatible(
                            &ae.value_type,
                            &pe.value_type,
                            class_loader,
                            strict_types,
                        )
                    });
            }
            arg_entries.iter().any(|ae| {
                ae.key == pe.key
                    && is_type_compatible(
                        &ae.value_type,
                        &pe.value_type,
                        class_loader,
                        strict_types,
                    )
            })
        });
        if all_param_keys_satisfied {
            return true;
        }
    }

    // ── ArrayShape → typed array: MAYBE, but only if every entry fits ──
    // `array{id: string, index: string, body: array}` should be
    // accepted where `array<string, mixed>` or similar is expected: the
    // shape is a more specific form of the typed array. But a shape
    // literal is a complete, statically-known list of values (unlike a
    // shape read off a real array, which may hold more at runtime), so
    // when every entry's value type is checked and one fails to fit the
    // declared value type, that is a genuine mismatch, not a MAYBE — a
    // `list<int>` parameter rejects an argument shape holding a string.
    if let TypeKind::ArrayShape(arg_entries) = arg_type.kind() {
        let value_type = match param_type.kind() {
            TypeKind::Generic(g) if is_array_like_name(&g.name) => g.args.last(),
            TypeKind::Array(elem) => Some(elem),
            _ => None,
        };
        if let Some(value_type) = value_type {
            if arg_entries
                .iter()
                .all(|e| is_type_compatible(&e.value_type, value_type, class_loader, strict_types))
            {
                return true;
            }
        } else if matches!(param_type.kind(), TypeKind::Generic(g) if is_array_like_name(&g.name))
            || matches!(param_type.kind(), TypeKind::Array(_))
        {
            return true;
        }
    }

    // ── Typed array → ArrayShape: MAYBE ─────────────────────────
    // `array<string, mixed>` or `array{index: string}` (fewer keys)
    // passed where a specific shape is expected.  The actual array
    // might have the right keys at runtime.  We can't prove
    // otherwise from the type alone.
    if matches!(param_type.kind(), TypeKind::ArrayShape(_)) && is_any_array_type(arg_type) {
        return true;
    }

    // ── Object shape structural match: MAYBE ────────────────────
    // `object{foo: int}` is a structural constraint: any class or
    // object literal with a public `foo` property assignable to `int`
    // satisfies it, regardless of its class hierarchy. Extra properties
    // on the argument are harmless — object shapes are structurally
    // open, like array shapes.
    if let TypeKind::ObjectShape(param_entries) = param_type.kind() {
        if let TypeKind::ObjectShape(arg_entries) = arg_type.kind() {
            let all_param_keys_satisfied = param_entries.iter().all(|pe| {
                if pe.optional {
                    return arg_entries
                        .iter()
                        .find(|ae| ae.key == pe.key)
                        .is_none_or(|ae| {
                            is_type_compatible(
                                &ae.value_type,
                                &pe.value_type,
                                class_loader,
                                strict_types,
                            )
                        });
                }
                arg_entries.iter().any(|ae| {
                    ae.key == pe.key
                        && is_type_compatible(
                            &ae.value_type,
                            &pe.value_type,
                            class_loader,
                            strict_types,
                        )
                })
            });
            if all_param_keys_satisfied {
                return true;
            }
        } else if let Some(class_name) = arg_type.base_name()
            && !crate::php_type::is_builtin_non_class_type(class_name)
            && !is_array_like_name(class_name)
        {
            match class_loader(class_name) {
                Some(cls) => {
                    let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
                        &cls,
                        class_loader,
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
                            Some(p) => match &p.type_hint {
                                Some(t) => is_type_compatible(
                                    t,
                                    &pe.value_type,
                                    class_loader,
                                    strict_types,
                                ),
                                None => true,
                            },
                            None => pe.optional,
                        }
                    });
                    if all_param_keys_satisfied {
                        return true;
                    }
                }
                None => {
                    // Class we can't load (likely unindexed vendor code) —
                    // stay permissive rather than risk a false positive.
                    return true;
                }
            }
        }
    }

    // Use the full subtype check including class hierarchy.
    is_subtype_of_typed(arg_type, param_type, class_loader)
}

/// Returns `true` when the argument is a base scalar and the parameter
/// is a refinement of that scalar.  In these cases we cannot prove the
/// value violates the refinement, so we stay silent.
///
/// Also handles the case where the parameter is a union type: if any
/// member of the union forms a refined scalar pair with the argument,
/// we stay silent (the value might satisfy that branch at runtime).
fn is_refined_scalar_pair(arg: &PhpType, param: &PhpType) -> bool {
    // When the param is a union, check each member individually.
    if let TypeKind::Union(members) = param.kind() {
        return members.iter().any(|m| is_refined_scalar_pair(arg, m));
    }

    let (arg_name, param_name) = match (arg.kind(), param.kind()) {
        (TypeKind::Named(a), TypeKind::Named(p)) => {
            (a.to_ascii_lowercase(), p.to_ascii_lowercase())
        }
        _ => return false,
    };

    matches!(
        (arg_name.as_str(), param_name.as_str()),
        (
            "string",
            "non-empty-string"
                | "numeric-string"
                | "class-string"
                | "interface-string"
                | "trait-string"
                | "enum-string"
                | "literal-string"
                | "non-empty-literal-string"
                | "callable-string"
                | "truthy-string"
                | "non-falsy-string"
                | "lowercase-string"
                | "non-empty-lowercase-string"
                | "uppercase-string"
                | "non-empty-uppercase-string"
        ) | (
            "int" | "integer",
            "positive-int"
                | "negative-int"
                | "non-negative-int"
                | "non-positive-int"
                | "non-zero-int"
        ) | (
            // bool *might* be true or false at runtime — we can't
            // prove otherwise from the type alone.  This arises when
            // template inference narrows a parameter to `true` or
            // `false` (e.g. Conditionable::when(true, …)).
            "bool" | "boolean",
            "true" | "false"
        )
    )
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
