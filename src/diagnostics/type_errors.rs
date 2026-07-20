//! Argument type mismatch diagnostics.
//!
//! Walk the precomputed [`CallSite`] entries in the symbol map and flag
//! every call where an argument's resolved type is incompatible with
//! the declared parameter type.
//!
//! This is Phase 1 of the type error diagnostic suite. Only clearly
//! incompatible types are flagged — when in doubt (unresolved types,
//! `mixed`, complex generics), the diagnostic is suppressed to avoid
//! false positives.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;
use mago_syntax::cst::call::Call;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::statement::Statement;
use mago_syntax::cst::{PartialArgument, Program};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::completion::resolver::{Loaders, VarResolutionCtx};
use crate::completion::variable::foreach_resolution::resolve_expression_type;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::php_type::{LiteralValue, PhpType, int_literal_is_within_range, is_array_like_name};
use crate::types::{ClassInfo, ResolvedCallableTarget};
use crate::util::is_subtype_of_typed;

use super::helpers::{find_innermost_enclosing_class, make_diagnostic};

/// Diagnostic code used for argument type mismatch diagnostics.
pub(crate) const TYPE_MISMATCH_ARGUMENT_CODE: &str = "type_mismatch_argument";

// ── Resolved argument info ──────────────────────────────────────────────────

/// A single argument's resolved type plus the byte range of the
/// expression in source.  Collected inside `with_parsed_program` so
/// we don't need to keep AST references alive.
struct ResolvedArg {
    /// The resolved type of the argument expression.
    ty: PhpType,
    /// Byte offset of the argument expression start (inclusive).
    start: usize,
    /// Byte offset of the argument expression end (exclusive).
    end: usize,
}

/// All resolved argument types for a single call site.
struct ResolvedCallArgs {
    args: Vec<ResolvedArg>,
}

// ── Type compatibility check ────────────────────────────────────────────────

/// Returns `true` when the type is a bare unparameterised `array`.
fn is_bare_array(ty: &PhpType) -> bool {
    matches!(ty, PhpType::Named(n) if n.eq_ignore_ascii_case("array"))
        || matches!(ty, PhpType::Array(inner) if inner.is_mixed())
}

/// Check if an argument type is compatible with a parameter type.
///
/// Returns `true` if the argument type can be passed to the parameter
/// without a type error.  Conservative: returns `true` (compatible)
/// when in doubt.
pub(super) fn is_type_compatible(
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
    // Closure(int): void <: callable, array<int, string> <: array)
    // are handled by the `is_subtype_of_typed` fallback at the end
    // of this function and should NOT be duplicated here.
    //
    // A future pass may tighten specific MAYBE relationships (reporting
    // them as mismatches rather than letting them pass) once we are
    // confident the narrowing does not introduce false positives.

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
    if let PhpType::Union(members) = arg_type
        && members.iter().any(|m| m.is_mixed())
    {
        return true;
    }
    // `void` should never appear as an argument but skip it conservatively.
    if arg_type.is_void() || param_type.is_void() {
        return true;
    }
    // Skip Raw types (unparseable / unresolved type strings).
    if matches!(arg_type, PhpType::Raw(_)) || matches!(param_type, PhpType::Raw(_)) {
        return true;
    }

    // Skip when the param type is a Named type that can't be loaded as a
    // class and has no namespace separator — it's likely a @phpstan-type /
    // @psalm-type alias that we couldn't expand.  We can't verify
    // compatibility without the underlying type, so suppress to avoid
    // false positives.  Namespaced types (containing `\`) are real class
    // references that should still be checked.
    if let PhpType::Named(name) = param_type
        && !name.contains('\\')
        && !crate::php_type::is_builtin_non_class_type(name)
        && class_loader(name).is_none()
    {
        return true;
    }

    // Same escape hatch for the argument type — an unexpanded
    // @phpstan-type / @psalm-type alias on the arg side would also
    // cause false positives (e.g. `Payload` passed to `?array`).
    if let PhpType::Named(name) = arg_type
        && !name.contains('\\')
        && !crate::php_type::is_builtin_non_class_type(name)
        && class_loader(name).is_none()
    {
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
    // Skip when param type is `object` and arg type is any class-like.
    // Any class instance IS an object — this is always YES.
    if param_type.is_object() && arg_type.base_name().is_some() {
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
    // Skip when param type is `iterable` and arg type is array-like or Traversable.
    if param_type.is_iterable()
        && (arg_type.is_array_like()
            || arg_type
                .base_name()
                .and_then(class_loader)
                .is_some_and(|cls| crate::util::is_subtype_of(&cls, "Traversable", class_loader)))
    {
        return true;
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
    if matches!(param_type, PhpType::IntRange(..))
        && let PhpType::Named(sub) = arg_type
        && matches!(sub.to_ascii_lowercase().as_str(), "int" | "integer")
    {
        return true;
    }

    // ── Conservative union argument handling ─────────────────────
    // When the argument is a union, require that *every* member is
    // definitely incompatible before flagging.  If at least one
    // member could be valid, the developer may have narrowed the
    // type in a way we can't see (assert, instanceof, etc.).
    // This avoids false positives like `null|Pen` vs `object|string`
    // where `Pen` is clearly fine and the null path may be guarded.
    if let PhpType::Union(members) = arg_type
        && members
            .iter()
            .any(|m| is_type_compatible(m, param_type, class_loader, strict_types))
    {
        return true;
    }

    // ── Intersection argument handling ───────────────────────────
    // A value of intersection type `A&B` satisfies *every* member, so
    // it is compatible with the param when *any* member is.  This is
    // the standard subtyping rule for intersections and covers common
    // cases like PHPUnit's `MockObject&Foo` (a mock that is also a Foo)
    // being returned where `Foo` (or a union containing `Foo`) is
    // expected.
    if let PhpType::Intersection(members) = arg_type
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
    if let PhpType::Union(members) = param_type
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
    if let PhpType::Intersection(members) = param_type
        && members
            .iter()
            .all(|m| is_type_compatible(arg_type, m, class_loader, strict_types))
    {
        return true;
    }

    // ── Bare Closure/callable ↔ callable specification: MAYBE ───
    // When the param is a callable specification like
    // `Closure(Builder<X>): mixed` and the arg is a bare `Closure`
    // or `callable`, we can't verify the signature — stay silent.
    // (The reverse direction — callable spec <: bare Closure — is a
    // strict YES handled by the `is_subtype_of_typed` fallback.)
    if matches!(param_type, PhpType::Callable { .. })
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

    // ── Nullable arg → non-nullable param: MAYBE ────────────────
    // The developer may have guarded against null before this call
    // (instanceof, assert, if-check).  We can't prove the null
    // path actually reaches here, so stay silent.
    if let PhpType::Nullable(inner) = arg_type
        && is_type_compatible(inner, param_type, class_loader, strict_types)
    {
        return true;
    }

    // ── Non-nullable arg → nullable param: YES ──────────────────
    // Passing `X` where `?X` is expected is always valid.
    if let PhpType::Nullable(inner) = param_type
        && is_type_compatible(arg_type, inner, class_loader, strict_types)
    {
        return true;
    }

    // ── Stringable objects accepted as string ────────────────────
    // PHP calls __toString() on Stringable objects when a string is
    // expected.  We only accept objects whose class implements
    // \Stringable or declares __toString().  For bare `object` types
    // (no class name) we stay permissive since we can't check.
    if param_type.is_string_type() && arg_type.is_object_like() {
        if let Some(class_name) = arg_type.base_name() {
            if let Some(cls) = class_loader(class_name) {
                let merged = crate::inheritance::resolve_class_with_inheritance(&cls, class_loader);
                let implements_stringable =
                    crate::util::is_subtype_of(&cls, "Stringable", class_loader);
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
        && let PhpType::Named(sup) = param_type
        && sup.eq_ignore_ascii_case("string")
    {
        let is_numeric_like = match arg_type {
            PhpType::Named(sub) => {
                matches!(
                    sub.to_ascii_lowercase().as_str(),
                    "int" | "integer" | "float" | "double"
                )
            }
            PhpType::Literal(LiteralValue::Int(_) | LiteralValue::Float(_)) => true,
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
        let arg_is_numeric_string = match arg_type {
            PhpType::Named(sub) => sub.eq_ignore_ascii_case("numeric-string"),
            PhpType::Literal(LiteralValue::String(s)) => {
                LiteralValue::string_raw(s.clone()).is_numeric_string()
            }
            _ => false,
        };
        if arg_is_numeric_string {
            match param_type {
                PhpType::Named(sup)
                    if matches!(
                        sup.to_ascii_lowercase().as_str(),
                        "float" | "double" | "int" | "integer" | "numeric"
                    ) =>
                {
                    return true;
                }
                PhpType::IntRange(min, max)
                    if let PhpType::Literal(LiteralValue::String(s)) = arg_type
                        && LiteralValue::string_raw(s.clone())
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
    if let PhpType::Named(sub) = arg_type
        && sub.eq_ignore_ascii_case("numeric")
        && let PhpType::Named(sup) = param_type
        && matches!(
            sup.to_ascii_lowercase().as_str(),
            "float" | "double" | "int" | "integer"
        )
    {
        return true;
    }

    // ── Array-like / traversable-like → generic iterable/Traversable ─
    // When the param is a generic `iterable<K,V>`, `Traversable<K,V>`,
    // or any interface that extends Traversable (e.g. `Arrayable<K,V>`),
    // we can't verify the generic type arguments covariantly at this
    // phase.  Stay silent (MAYBE) when the base types are compatible.
    if let PhpType::Generic(name, _) = param_type
        && (name.eq_ignore_ascii_case("iterable")
            || class_loader(name)
                .is_some_and(|cls| crate::util::is_subtype_of(&cls, "Traversable", class_loader)))
    {
        // Array-like args always satisfy iterable/Traversable generics.
        if arg_type.is_array_like()
            || matches!(arg_type, PhpType::Generic(n, _) if is_array_like_name(n))
            || matches!(arg_type, PhpType::Array(_) | PhpType::ArrayShape(_))
        {
            return true;
        }
        // Object-like args: check hierarchy for Traversable.
        // Can't load → stay permissive; bare `object` → stay permissive.
        if let Some(class_name) = arg_type.base_name() {
            if let Some(cls) = class_loader(class_name) {
                if crate::util::is_subtype_of(&cls, "Traversable", class_loader) {
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
    if let (PhpType::Generic(name_arg, args_arg), PhpType::Generic(name_param, args_param)) =
        (arg_type, param_type)
    {
        let base_arg = name_arg.to_ascii_lowercase();
        let base_param = name_param.to_ascii_lowercase();
        let bases_match = base_arg == base_param
            || (is_array_like_name(name_arg) && is_array_like_name(name_param));
        if bases_match && args_arg.len() == args_param.len() && !args_arg.is_empty() {
            let all_args_compatible = args_arg
                .iter()
                .zip(args_param.iter())
                .all(|(a, p)| is_type_compatible(a, p, class_loader, strict_types));
            if all_args_compatible {
                return true;
            }
        }
    }

    // ── list<X> ↔ array<int, X>: MAYBE ─────────────────────────
    // A list is an array with sequential int keys starting at 0.
    // In practice, PHP codebases use these interchangeably and we
    // can't verify the structural guarantee statically.
    if let (PhpType::Generic(name_arg, args_arg), PhpType::Generic(name_param, args_param)) =
        (arg_type, param_type)
    {
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

        // ── array<K, V> → array<V> (2-param to 1-param): YES ───
        // `array<V>` means `array<mixed, V>`.  A 2-param array is
        // more specific — its value type always fits in the 1-param
        // form as long as the value types are compatible.
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

    // ── Generic array → X[] : YES ───────────────────────────────
    // `array<int, string>` → `string[]` — the value type of the
    // generic form is more specific than (or equal to) the slice.
    if let PhpType::Generic(name, args) = arg_type
        && is_array_like_name(name)
        && let PhpType::Array(inner) = param_type
    {
        let mixed = PhpType::mixed();
        let val = args.last().unwrap_or(&mixed);
        if is_type_compatible(val, inner, class_loader, strict_types) {
            return true;
        }
    }

    // ── X[] → Generic array : MAYBE ─────────────────────────────
    // `string[]` → `array<int, string>` — the key type is unknown,
    // it *might* be int at runtime.
    if let PhpType::Array(inner) = arg_type
        && let PhpType::Generic(name, args) = param_type
        && is_array_like_name(name)
    {
        let mixed = PhpType::mixed();
        let val = args.last().unwrap_or(&mixed);
        if is_type_compatible(inner, val, class_loader, strict_types) {
            return true;
        }
    }

    // ── list<X> → X[] : YES ────────────────────────────────────
    // A list is a stricter form of array (sequential int keys).
    // It always satisfies the weaker `X[]` constraint.
    if let PhpType::Generic(name, args) = arg_type
        && name.eq_ignore_ascii_case("list")
        && args.len() == 1
        && let PhpType::Array(inner) = param_type
        && is_type_compatible(&args[0], inner, class_loader, strict_types)
    {
        return true;
    }

    // ── X[] → list<X> : MAYBE ──────────────────────────────────
    // `X[]` is `array<mixed, X>` — it might have non-sequential
    // keys, so it might not be a valid list.  Stay silent.
    if let PhpType::Array(inner) = arg_type
        && let PhpType::Generic(name, args) = param_type
        && name.eq_ignore_ascii_case("list")
        && args.len() == 1
        && is_type_compatible(inner, &args[0], class_loader, strict_types)
    {
        return true;
    }

    // ── Class hierarchy: reverse direction MAYBE ────────────────
    //
    // Direction 1 (arg <: param, e.g. `Cat` passed to `Animal`)
    // is a strict YES handled by the `is_subtype_of_typed` fallback
    // at the end of this function.  No need to duplicate it here.
    //
    // Direction 2 (param <: arg, e.g. `Carbon\Carbon` passed where a
    // `Illuminate\Support\Carbon` subclass is expected) is a MAYBE:
    // the argument is a *broader* type but the value *might* be the
    // narrower concrete at runtime (the developer may have checked
    // with instanceof, or the API always returns the concrete type
    // despite being typed as the parent).  This also covers cases the
    // resolver still under-narrows, such as an Eloquent relation typed
    // as the base `Collection` where a custom collection subclass is
    // declared — dropping this rule turns those into false positives
    // that PHPStan does not report.
    //
    // However, if the arg's class is **final**, the value cannot be
    // any subtype — it is exactly that class.  So `final class Jack`
    // passed to `JackSparrow` (where Jack does not extend
    // JackSparrow) is a definite NO.
    //
    // We intentionally do NOT treat `object` or `stdClass` as
    // universal supertypes here.  `base_name()` returns `None` for
    // `object` (it is in the scalar-name list), so it never enters
    // this block.  `stdClass` has a base_name but is not a parent
    // of arbitrary classes in PHP — the hierarchy walk will simply
    // not find a relationship, which is correct.

    if let (Some(arg_base), Some(param_base)) = (arg_type.base_name(), param_type.base_name()) {
        let arg_cls = class_loader(arg_base);

        let arg_is_final = arg_cls.as_ref().is_some_and(|cls| cls.is_final);
        if !arg_is_final {
            if is_subtype_of_typed(param_type, arg_type, class_loader) {
                return true;
            }
            // Also try loading param and walking up to arg.
            if let Some(param_cls) = class_loader(param_base)
                && crate::util::is_subtype_of(&param_cls, arg_base, class_loader)
            {
                return true;
            }
        }
    }

    // ── Arrayable/ArrayAccess arg → bare array: MAYBE ───────────
    // Many collection-like types implement ArrayAccess or Arrayable
    // and are used interchangeably with arrays.  When the arg type
    // implements one of these interfaces and the param is bare
    // `array`, stay silent.  Note: Traversable alone does NOT
    // qualify — a Traversable is not substitutable for `array`
    // (e.g. you cannot pass a Collection to array_map()).
    if is_bare_array(param_type) {
        if let Some(class_name) = arg_type.base_name()
            && let Some(cls) = class_loader(class_name)
        {
            let is_array_like = crate::util::is_subtype_of(&cls, "ArrayAccess", class_loader)
                || crate::util::is_subtype_of(&cls, "Arrayable", class_loader);
            if is_array_like {
                return true;
            }
        }
        // Union arg: accept if every member is array-like or
        // implements ArrayAccess/Arrayable.  When a
        // member's class can't be loaded (unresolved short name
        // from a docblock), treat it as potentially array-like
        // to avoid false positives on large collection union
        // types (DataCollection|PaginatedDataCollection|...).
        if let PhpType::Union(members) = arg_type {
            let all_array_like = members.iter().all(|m| {
                if m.is_array_like() || matches!(m, PhpType::Array(_) | PhpType::ArrayShape(_)) {
                    return true;
                }
                if let Some(name) = m.base_name() {
                    if let Some(cls) = class_loader(name) {
                        return crate::util::is_subtype_of(&cls, "ArrayAccess", class_loader)
                            || crate::util::is_subtype_of(&cls, "Arrayable", class_loader);
                    }
                    // Can't load class — stay permissive (the short
                    // name likely comes from a docblock whose imports
                    // we can't resolve in this context).
                    return true;
                }
                false
            });
            if all_array_like {
                return true;
            }
        }
    }

    // ── Array shape superset → subset: MAYBE ────────────────────
    // When an array shape has extra keys beyond what the parameter
    // shape requires, the extra keys are harmless.  PHP array shapes
    // are open by convention in most codebases.
    // Optional keys in the param shape (marked with `?`) do not need
    // to be present in the arg shape — they are, by definition,
    // not required.
    if let (PhpType::ArrayShape(arg_entries), PhpType::ArrayShape(param_entries)) =
        (arg_type, param_type)
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

    // ── ArrayShape → typed array: MAYBE ─────────────────────────
    // `array{id: string, index: string, body: array}` should be
    // accepted where `array<string, mixed>` or similar is expected.
    // The shape is a more specific form of the typed array.
    if matches!(arg_type, PhpType::ArrayShape(_))
        && matches!(param_type, PhpType::Generic(name, _) if is_array_like_name(name))
    {
        return true;
    }
    if matches!(arg_type, PhpType::ArrayShape(_)) && matches!(param_type, PhpType::Array(_)) {
        return true;
    }

    // ── Typed array → ArrayShape: MAYBE ─────────────────────────
    // `array<string, mixed>` or `array{index: string}` (fewer keys)
    // passed where a specific shape is expected.  The actual array
    // might have the right keys at runtime.  We can't prove
    // otherwise from the type alone.
    if matches!(param_type, PhpType::ArrayShape(_)) && is_any_array_type(arg_type) {
        return true;
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
    if let PhpType::Union(members) = param {
        return members.iter().any(|m| is_refined_scalar_pair(arg, m));
    }

    let (arg_name, param_name) = match (arg, param) {
        (PhpType::Named(a), PhpType::Named(p)) => (a.to_ascii_lowercase(), p.to_ascii_lowercase()),
        _ => return false,
    };

    matches!(
        (arg_name.as_str(), param_name.as_str()),
        (
            "string",
            "non-empty-string"
                | "numeric-string"
                | "class-string"
                | "literal-string"
                | "callable-string"
                | "truthy-string"
                | "non-falsy-string"
                | "lowercase-string"
                | "non-empty-lowercase-string"
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

/// Returns `true` when the file declares `strict_types=1`.
///
/// Scans the top-level statements of the parsed program for a
/// `declare(strict_types=1)` directive.  In PHP this must appear as
/// the very first statement (after `<?php`), but we check all
/// top-level statements for robustness.
pub(super) fn has_strict_types(program: &Program<'_>) -> bool {
    for stmt in program.statements.iter() {
        if let Statement::Declare(declare) = stmt {
            for item in declare.items.iter() {
                if bytes_to_str(item.name.value).eq_ignore_ascii_case("strict_types")
                    && let Expression::Literal(Literal::Integer(i)) = item.value
                    && bytes_to_str(i.raw) == "1"
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Returns `true` when the type is any form of array (bare, generic,
/// slice, or shape).  Used by the bare-array MAYBE rules to check
/// inside unions as well as at the top level.
fn is_any_array_type(ty: &PhpType) -> bool {
    matches!(ty, PhpType::Array(_) | PhpType::ArrayShape(_))
        || matches!(ty, PhpType::Generic(name, _) if is_array_like_name(name))
        || is_bare_array(ty)
}

/// Deep check: returns `true` when the type contains `self`, `static`,
/// `$this`, or `parent` anywhere in its structure — including inside
/// union/intersection members, nullable wrappers, and generic arguments.
fn contains_self_or_parent(ty: &PhpType) -> bool {
    match ty {
        PhpType::Named(s) => {
            let low = s.to_ascii_lowercase();
            matches!(low.as_str(), "self" | "static" | "$this" | "parent")
        }
        PhpType::Nullable(inner) => contains_self_or_parent(inner),
        PhpType::Union(members) | PhpType::Intersection(members) => {
            members.iter().any(contains_self_or_parent)
        }
        PhpType::Generic(name, args) => {
            let low = name.to_ascii_lowercase();
            matches!(low.as_str(), "self" | "static" | "$this" | "parent")
                || args.iter().any(contains_self_or_parent)
        }
        _ => false,
    }
}

// ── AST walking: collect argument expressions keyed by args_start ───────────

/// Try to register the argument expressions of an [`ArgumentList`] if its
/// `args_start` offset matches one of the call sites we are interested in.
fn try_collect_argument_list<'a>(
    arg_list: &'a mago_syntax::cst::argument::ArgumentList<'a>,
    call_site_starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    let args_start = arg_list.left_parenthesis.end.offset;
    if !call_site_starts.contains(&args_start) {
        return;
    }
    let expressions: Vec<(&'a Expression<'a>, usize, usize)> = arg_list
        .arguments
        .iter()
        .map(|arg| {
            let value = match arg {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            };
            let start = value.span().start.offset as usize;
            let end = value.span().end.offset as usize;
            (value, start, end)
        })
        .collect();
    result.insert(args_start, expressions);
}

/// Try to register the partial argument expressions of an [`PartialArgumentList`] if its
/// `args_start` offset matches one of the call sites we are interested in.
fn try_collect_partial_argument_list<'a>(
    arg_list: &'a mago_syntax::cst::argument::PartialArgumentList<'a>,
    call_site_starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    let args_start = arg_list.left_parenthesis.end.offset;
    if !call_site_starts.contains(&args_start) {
        return;
    }
    // Placeholders have no expression and therefore no type to validate.
    // Keep only supplied positional/named arguments.
    let expressions: Vec<(&'a Expression<'a>, usize, usize)> = arg_list
        .arguments
        .iter()
        .filter_map(|arg| {
            let value = match arg {
                PartialArgument::Positional(pos) => pos.value,
                PartialArgument::Named(named) => named.value,
                PartialArgument::NamedPlaceholder(_)
                | PartialArgument::Placeholder(_)
                | PartialArgument::VariadicPlaceholder(_) => return None,
            };
            let start = value.span().start.offset as usize;
            let end = value.span().end.offset as usize;
            Some((value, start, end))
        })
        .collect();
    result.insert(args_start, expressions);
}

// ── Recursive AST walkers ───────────────────────────────────────────────────

fn collect_from_statement<'a>(
    stmt: &'a Statement<'a>,
    starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    match stmt {
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                collect_from_statement(inner, starts, result);
            }
        }
        Statement::Class(class) => {
            for member in class.members.iter() {
                collect_from_class_member(member, starts, result);
            }
        }
        Statement::Interface(iface) => {
            for member in iface.members.iter() {
                collect_from_class_member(member, starts, result);
            }
        }
        Statement::Trait(trait_def) => {
            for member in trait_def.members.iter() {
                collect_from_class_member(member, starts, result);
            }
        }
        Statement::Enum(enum_def) => {
            for member in enum_def.members.iter() {
                collect_from_class_member(member, starts, result);
            }
        }
        Statement::Function(func) => {
            for s in func.body.statements.iter() {
                collect_from_statement(s, starts, result);
            }
        }
        Statement::Expression(expr_stmt) => {
            collect_from_expression(expr_stmt.expression, starts, result);
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                collect_from_expression(val, starts, result);
            }
        }
        Statement::Echo(echo) => {
            for expr in echo.values.iter() {
                collect_from_expression(expr, starts, result);
            }
        }
        Statement::If(if_stmt) => {
            collect_from_expression(if_stmt.condition, starts, result);
            collect_from_if_body(&if_stmt.body, starts, result);
        }
        Statement::While(while_stmt) => {
            collect_from_expression(while_stmt.condition, starts, result);
            for s in while_stmt.body.statements() {
                collect_from_statement(s, starts, result);
            }
        }
        Statement::DoWhile(do_while) => {
            collect_from_statement(do_while.statement, starts, result);
            collect_from_expression(do_while.condition, starts, result);
        }
        Statement::For(for_stmt) => {
            for expr in for_stmt.initializations.iter() {
                collect_from_expression(expr, starts, result);
            }
            for expr in for_stmt.conditions.iter() {
                collect_from_expression(expr, starts, result);
            }
            for expr in for_stmt.increments.iter() {
                collect_from_expression(expr, starts, result);
            }
            for s in for_stmt.body.statements() {
                collect_from_statement(s, starts, result);
            }
        }
        Statement::Foreach(foreach_stmt) => {
            collect_from_expression(foreach_stmt.expression, starts, result);
            for s in foreach_stmt.body.statements() {
                collect_from_statement(s, starts, result);
            }
        }
        Statement::Switch(switch_stmt) => {
            collect_from_expression(switch_stmt.expression, starts, result);
            collect_from_switch_body(&switch_stmt.body, starts, result);
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_from_statement(s, starts, result);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_from_statement(s, starts, result);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_from_statement(s, starts, result);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_from_statement(s, starts, result);
            }
        }
        Statement::Unset(unset_stmt) => {
            for val in unset_stmt.values.iter() {
                collect_from_expression(val, starts, result);
            }
        }
        Statement::Constant(constant) => {
            for item in constant.items.iter() {
                collect_from_expression(item.value, starts, result);
            }
        }
        Statement::Declare(declare) => {
            use mago_syntax::cst::declare::DeclareBody;
            match &declare.body {
                DeclareBody::Statement(inner) => {
                    collect_from_statement(inner, starts, result);
                }
                DeclareBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_from_statement(s, starts, result);
                    }
                }
            }
        }
        Statement::EchoTag(echo_tag) => {
            for expr in echo_tag.values.iter() {
                collect_from_expression(expr, starts, result);
            }
        }
        Statement::Global(_)
        | Statement::Static(_)
        | Statement::OpeningTag(_)
        | Statement::ClosingTag(_)
        | Statement::Inline(_)
        | Statement::Use(_)
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Continue(_)
        | Statement::Break(_)
        | Statement::HaltCompiler(_)
        | Statement::Noop(_) => {}
        // Non-exhaustive catch-all for future AST variants.
        _ => {}
    }
}

fn collect_from_class_member<'a>(
    member: &'a mago_syntax::cst::class_like::member::ClassLikeMember<'a>,
    starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;
    use mago_syntax::cst::class_like::property::{
        Property, PropertyHookBody, PropertyHookConcreteBody, PropertyItem,
    };
    match member {
        ClassLikeMember::Method(method) => match &method.body {
            MethodBody::Concrete(block) => {
                for s in block.statements.iter() {
                    collect_from_statement(s, starts, result);
                }
            }
            MethodBody::Abstract(_) => {}
        },
        ClassLikeMember::Property(prop) => match prop {
            Property::Plain(plain) => {
                for item in plain.items.iter() {
                    if let PropertyItem::Concrete(concrete) = item {
                        collect_from_expression(concrete.value, starts, result);
                    }
                }
            }
            Property::Hooked(hooked) => {
                if let PropertyItem::Concrete(concrete) = &hooked.item {
                    collect_from_expression(concrete.value, starts, result);
                }
                for hook in hooked.hook_list.hooks.iter() {
                    match &hook.body {
                        PropertyHookBody::Concrete(PropertyHookConcreteBody::Block(block)) => {
                            for s in block.statements.iter() {
                                collect_from_statement(s, starts, result);
                            }
                        }
                        PropertyHookBody::Concrete(PropertyHookConcreteBody::Expression(
                            expr_body,
                        )) => {
                            collect_from_expression(expr_body.expression, starts, result);
                        }
                        PropertyHookBody::Abstract(_) => {}
                    }
                }
            }
        },
        ClassLikeMember::Constant(c) => {
            for item in c.items.iter() {
                collect_from_expression(item.value, starts, result);
            }
        }
        ClassLikeMember::EnumCase(ec) => {
            use mago_syntax::cst::class_like::enum_case::EnumCaseItem;
            match &ec.item {
                EnumCaseItem::Backed(b) => {
                    collect_from_expression(b.value, starts, result);
                }
                EnumCaseItem::Unit(_) => {}
            }
        }
        ClassLikeMember::TraitUse(_) => {}
    }
}

fn collect_from_if_body<'a>(
    body: &'a mago_syntax::cst::control_flow::r#if::IfBody<'a>,
    starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    use mago_syntax::cst::control_flow::r#if::IfBody;
    match body {
        IfBody::Statement(if_stmt_body) => {
            collect_from_statement(if_stmt_body.statement, starts, result);
            for elseif in if_stmt_body.else_if_clauses.iter() {
                collect_from_expression(elseif.condition, starts, result);
                collect_from_statement(elseif.statement, starts, result);
            }
            if let Some(ref else_clause) = if_stmt_body.else_clause {
                collect_from_statement(else_clause.statement, starts, result);
            }
        }
        IfBody::ColonDelimited(colon_body) => {
            for s in colon_body.statements.iter() {
                collect_from_statement(s, starts, result);
            }
            for elseif in colon_body.else_if_clauses.iter() {
                collect_from_expression(elseif.condition, starts, result);
                for s in elseif.statements.iter() {
                    collect_from_statement(s, starts, result);
                }
            }
            if let Some(ref else_clause) = colon_body.else_clause {
                for s in else_clause.statements.iter() {
                    collect_from_statement(s, starts, result);
                }
            }
        }
    }
}

fn collect_from_switch_body<'a>(
    body: &'a mago_syntax::cst::control_flow::switch::SwitchBody<'a>,
    starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    use mago_syntax::cst::control_flow::switch::SwitchBody;
    match body {
        SwitchBody::BraceDelimited(b) => {
            for case in b.cases.iter() {
                for s in case.statements() {
                    collect_from_statement(s, starts, result);
                }
            }
        }
        SwitchBody::ColonDelimited(b) => {
            for case in b.cases.iter() {
                for s in case.statements() {
                    collect_from_statement(s, starts, result);
                }
            }
        }
    }
}

fn collect_from_expression<'a>(
    expr: &'a Expression<'a>,
    starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    match expr {
        // ── Calls — these are what we are looking for ───────────
        Expression::Call(call) => match call {
            Call::Function(func_call) => {
                collect_from_expression(func_call.function, starts, result);
                try_collect_argument_list(&func_call.argument_list, starts, result);
                collect_from_argument_list(&func_call.argument_list, starts, result);
            }
            Call::Method(method_call) => {
                collect_from_expression(method_call.object, starts, result);
                try_collect_argument_list(&method_call.argument_list, starts, result);
                collect_from_argument_list(&method_call.argument_list, starts, result);
            }
            Call::NullSafeMethod(method_call) => {
                collect_from_expression(method_call.object, starts, result);
                try_collect_argument_list(&method_call.argument_list, starts, result);
                collect_from_argument_list(&method_call.argument_list, starts, result);
            }
            Call::StaticMethod(static_call) => {
                collect_from_expression(static_call.class, starts, result);
                try_collect_argument_list(&static_call.argument_list, starts, result);
                collect_from_argument_list(&static_call.argument_list, starts, result);
            }
        },

        // ── Instantiation: `new Foo(...)` ──
        Expression::Instantiation(inst) => {
            collect_from_expression(inst.class, starts, result);
            if let Some(ref args) = inst.argument_list {
                try_collect_argument_list(args, starts, result);
                collect_from_argument_list(args, starts, result);
            }
        }

        // ── Recurse into sub-expressions ────────────────────────
        Expression::Binary(bin) => {
            collect_from_expression(bin.lhs, starts, result);
            collect_from_expression(bin.rhs, starts, result);
        }
        Expression::UnaryPrefix(un) => {
            collect_from_expression(un.operand, starts, result);
        }
        Expression::UnaryPostfix(un) => {
            collect_from_expression(un.operand, starts, result);
        }
        Expression::Parenthesized(paren) => {
            collect_from_expression(paren.expression, starts, result);
        }
        Expression::Assignment(assign) => {
            collect_from_expression(assign.lhs, starts, result);
            collect_from_expression(assign.rhs, starts, result);
        }
        Expression::Conditional(cond) => {
            collect_from_expression(cond.condition, starts, result);
            if let Some(then_expr) = cond.then {
                collect_from_expression(then_expr, starts, result);
            }
            collect_from_expression(cond.r#else, starts, result);
        }
        Expression::Array(arr) => {
            collect_from_array_elements(&arr.elements, starts, result);
        }
        Expression::LegacyArray(arr) => {
            collect_from_array_elements(&arr.elements, starts, result);
        }
        Expression::List(list) => {
            collect_from_array_elements(&list.elements, starts, result);
        }
        Expression::ArrayAccess(access) => {
            collect_from_expression(access.array, starts, result);
            collect_from_expression(access.index, starts, result);
        }
        Expression::ArrayAppend(append) => {
            collect_from_expression(append.array, starts, result);
        }
        Expression::Closure(closure) => {
            for s in closure.body.statements.iter() {
                collect_from_statement(s, starts, result);
            }
        }
        Expression::ArrowFunction(arrow) => {
            collect_from_expression(arrow.expression, starts, result);
        }
        Expression::Match(match_expr) => {
            collect_from_expression(match_expr.expression, starts, result);
            use mago_syntax::cst::control_flow::r#match::MatchArm;
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(expr_arm) => {
                        for cond in expr_arm.conditions.iter() {
                            collect_from_expression(cond, starts, result);
                        }
                        collect_from_expression(expr_arm.expression, starts, result);
                    }
                    MatchArm::Default(def_arm) => {
                        collect_from_expression(def_arm.expression, starts, result);
                    }
                }
            }
        }
        Expression::Yield(yield_expr) => {
            use mago_syntax::cst::r#yield::Yield;
            match yield_expr {
                Yield::Value(v) => {
                    if let Some(val) = v.value {
                        collect_from_expression(val, starts, result);
                    }
                }
                Yield::Pair(p) => {
                    collect_from_expression(p.key, starts, result);
                    collect_from_expression(p.value, starts, result);
                }
                Yield::From(f) => {
                    collect_from_expression(f.iterator, starts, result);
                }
            }
        }
        Expression::Throw(throw) => {
            collect_from_expression(throw.exception, starts, result);
        }
        Expression::Clone(clone) => {
            collect_from_expression(clone.object, starts, result);
        }
        Expression::Access(access) => {
            use mago_syntax::cst::access::Access;
            match access {
                Access::Property(pa) => {
                    collect_from_expression(pa.object, starts, result);
                }
                Access::NullSafeProperty(pa) => {
                    collect_from_expression(pa.object, starts, result);
                }
                Access::StaticProperty(spa) => {
                    collect_from_expression(spa.class, starts, result);
                }
                Access::ClassConstant(cca) => {
                    collect_from_expression(cca.class, starts, result);
                }
            }
        }
        Expression::Construct(construct) => {
            use mago_syntax::cst::construct::Construct;
            match construct {
                Construct::Isset(c) => {
                    for val in c.values.iter() {
                        collect_from_expression(val, starts, result);
                    }
                }
                Construct::Empty(c) => {
                    collect_from_expression(c.value, starts, result);
                }
                Construct::Eval(c) => {
                    collect_from_expression(c.value, starts, result);
                }
                Construct::Include(c) => {
                    collect_from_expression(c.value, starts, result);
                }
                Construct::IncludeOnce(c) => {
                    collect_from_expression(c.value, starts, result);
                }
                Construct::Require(c) => {
                    collect_from_expression(c.value, starts, result);
                }
                Construct::RequireOnce(c) => {
                    collect_from_expression(c.value, starts, result);
                }
                Construct::Print(c) => {
                    collect_from_expression(c.value, starts, result);
                }
                Construct::Exit(c) => {
                    if let Some(ref args) = c.arguments {
                        for arg in args.arguments.iter() {
                            collect_from_expression(arg.value(), starts, result);
                        }
                    }
                }
                Construct::Die(c) => {
                    if let Some(ref args) = c.arguments {
                        for arg in args.arguments.iter() {
                            collect_from_expression(arg.value(), starts, result);
                        }
                    }
                }
            }
        }
        Expression::Pipe(pipe) => {
            collect_from_expression(pipe.input, starts, result);
            collect_from_expression(pipe.callable, starts, result);
        }
        Expression::CompositeString(cs) => {
            use mago_syntax::cst::string::{CompositeString, StringPart};
            let walk_parts = |parts: &'a mago_syntax::cst::sequence::Sequence<
                'a,
                StringPart<'a>,
            >,
                              starts: &HashSet<u32>,
                              result: &mut HashMap<
                u32,
                Vec<(&'a Expression<'a>, usize, usize)>,
            >| {
                for part in parts.iter() {
                    match part {
                        StringPart::Expression(inner_expr) => {
                            collect_from_expression(inner_expr, starts, result);
                        }
                        StringPart::BracedExpression(braced) => {
                            collect_from_expression(braced.expression, starts, result);
                        }
                        StringPart::Literal(_) => {}
                    }
                }
            };
            match cs {
                CompositeString::Interpolated(interp) => {
                    walk_parts(&interp.parts, starts, result);
                }
                CompositeString::Document(doc) => {
                    walk_parts(&doc.parts, starts, result);
                }
                CompositeString::ShellExecute(shell) => {
                    walk_parts(&shell.parts, starts, result);
                }
            }
        }
        Expression::AnonymousClass(anon) => {
            if let Some(ref args) = anon.argument_list {
                try_collect_partial_argument_list(args, starts, result);
                collect_from_partial_argument_list(args, starts, result);
            }
            for member in anon.members.iter() {
                collect_from_class_member(member, starts, result);
            }
        }

        // ── Leaf expressions — no sub-expressions to recurse into ──
        Expression::Literal(_)
        | Expression::Variable(_)
        | Expression::ConstantAccess(_)
        | Expression::Identifier(_)
        | Expression::Parent(_)
        | Expression::Static(_)
        | Expression::Self_(_)
        | Expression::MagicConstant(_)
        | Expression::Error(_) => {}

        // ── Partial application — skip for now ──
        Expression::PartialApplication(_) => {}

        // Non-exhaustive catch-all for future AST variants.
        _ => {}
    }
}

fn collect_from_argument_list<'a>(
    arg_list: &'a mago_syntax::cst::argument::ArgumentList<'a>,
    starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    for arg in arg_list.arguments.iter() {
        collect_from_expression(arg.value(), starts, result);
    }
}

fn collect_from_partial_argument_list<'a>(
    arg_list: &'a mago_syntax::cst::argument::PartialArgumentList<'a>,
    starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    for arg in arg_list.arguments.iter() {
        if let Some(value) = arg.value() {
            collect_from_expression(value, starts, result);
        }
    }
}

fn collect_from_array_elements<'a>(
    elements: &'a mago_syntax::cst::sequence::TokenSeparatedSequence<
        'a,
        mago_syntax::cst::array::ArrayElement<'a>,
    >,
    starts: &HashSet<u32>,
    result: &mut HashMap<u32, Vec<(&'a Expression<'a>, usize, usize)>>,
) {
    use mago_syntax::cst::array::ArrayElement;
    for elem in elements.iter() {
        match elem {
            ArrayElement::KeyValue(kv) => {
                collect_from_expression(kv.key, starts, result);
                collect_from_expression(kv.value, starts, result);
            }
            ArrayElement::Value(v) => {
                collect_from_expression(v.value, starts, result);
            }
            ArrayElement::Variadic(v) => {
                collect_from_expression(v.value, starts, result);
            }
            ArrayElement::Missing(_) => {}
        }
    }
}

// ── Main diagnostic collection ──────────────────────────────────────────────

impl Backend {
    /// Collect argument type mismatch diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_argument_type_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        // ── Gather context under locks ──────────────────────────────
        let symbol_map = {
            let maps = self.symbol_maps.read();
            match maps.get(uri) {
                Some(sm) => sm.clone(),
                None => return,
            }
        };

        let file_ctx = self.file_context(uri);

        // Activate the thread-local parse cache so that every call to
        // `with_parsed_program(content, …)` in the resolution pipeline
        // reuses the same parsed AST instead of re-parsing the file.
        let _parse_guard = with_parse_cache(content);

        // Build the set of call site args_start offsets for the AST walk.
        // Only include sites without argument unpacking.
        let call_site_starts: HashSet<u32> = symbol_map
            .call_sites
            .iter()
            .filter(|cs| !cs.has_unpacking)
            .map(|cs| cs.args_start)
            .collect();

        if call_site_starts.is_empty() {
            return;
        }

        let class_loader = self.class_loader(&file_ctx);
        let function_loader_cl = self.function_loader(&file_ctx);
        let constant_loader_cl = self.constant_loader();
        let default_class = ClassInfo::default();

        // Walk the AST once, collect argument expressions, and resolve
        // their types — all inside the `with_parsed_program` closure so
        // AST references never escape the arena lifetime.
        let (resolved_map, strict_types): (HashMap<u32, ResolvedCallArgs>, bool) =
            with_parsed_program(content, "type_error_diagnostics", |program, _content| {
                let strict_types = has_strict_types(program);

                // Phase 1: walk the AST and collect raw argument expressions
                // keyed by args_start offset.
                let mut expr_map: HashMap<u32, Vec<(&Expression<'_>, usize, usize)>> =
                    HashMap::new();
                for stmt in program.statements.iter() {
                    collect_from_statement(stmt, &call_site_starts, &mut expr_map);
                }

                // Phase 2: resolve types for each collected expression.
                let mut result: HashMap<u32, ResolvedCallArgs> = HashMap::new();
                for (args_start, exprs) in &expr_map {
                    let enclosing = find_innermost_enclosing_class(&file_ctx.classes, *args_start);
                    let current_class_info = enclosing.unwrap_or(&default_class);

                    let loaders = Loaders {
                        function_loader: Some(&function_loader_cl),
                        constant_loader: Some(&constant_loader_cl),
                    };

                    let var_ctx = VarResolutionCtx {
                        var_name: "",
                        top_level_scope: None,
                        current_class: current_class_info,
                        all_classes: &file_ctx.classes,
                        content,
                        cursor_offset: *args_start,
                        class_loader: &class_loader,
                        loaders,
                        resolved_class_cache: Some(&self.resolved_class_cache),
                        enclosing_return_type: None,
                        branch_aware: true,
                        match_arm_narrowing: HashMap::new(),
                        scope_var_resolver: None,
                    };

                    let mut resolved_args = Vec::with_capacity(exprs.len());
                    for &(arg_expr, start, end) in exprs {
                        // Use the argument expression's own start offset
                        // as the cursor position so that variable
                        // resolution only sees assignments *before* this
                        // expression.  Without this, `$type = Enum::from($type)`
                        // would resolve the RHS `$type` to `Enum` (the
                        // result of the assignment on the same line)
                        // instead of the prior `string` value from the
                        // foreach key.
                        let arg_ctx = var_ctx.with_cursor_offset(start as u32);
                        let ty = resolve_expression_type(arg_expr, &arg_ctx)
                            .unwrap_or_else(PhpType::untyped);
                        // Narrow scalar literals to their precise literal
                        // type so that e.g. `'desc'` matches `'asc'|'desc'`
                        // and `2` matches `1|2|3`.  The general expression
                        // resolver returns the widened base type (`string`,
                        // `int`, `float`) which is correct for variable
                        // tracking, but for argument diagnostics we need
                        // the exact value to compare against literal types.
                        //
                        // Numeric literals use the parsed value rather than
                        // the raw source text so that non-decimal forms
                        // (`0xFF`, `0b1010`, `0o17`, `1_000`) still match
                        // `int`/`float`/`numeric` parameters and decimal
                        // literal unions (`1|2|3`).  The raw text would not
                        // parse back into a number and would be flagged as
                        // an incompatible argument.
                        let ty = match arg_expr {
                            Expression::Literal(Literal::String(s)) => {
                                PhpType::literal_string_raw(bytes_to_str(s.raw).to_string())
                            }
                            Expression::Literal(Literal::Integer(i)) => match i.value {
                                Some(value) => PhpType::literal_int(value.to_string()),
                                None => ty,
                            },
                            Expression::Literal(Literal::Float(f)) => {
                                PhpType::literal_float(bytes_to_str(f.raw).to_string())
                            }
                            // Negative numeric literals (`-1`, `-1.5`) parse
                            // as a unary negation wrapping the literal, not
                            // as a single `Literal` node, so they need their
                            // own case to be narrowed the same way.
                            Expression::UnaryPrefix(unary)
                                if matches!(
                                    unary.operator,
                                    mago_syntax::cst::unary::UnaryPrefixOperator::Negation(_)
                                ) =>
                            {
                                match unary.operand {
                                    Expression::Literal(Literal::Integer(i)) => match i.value {
                                        Some(value) => PhpType::literal_int(format!("-{value}")),
                                        None => ty,
                                    },
                                    Expression::Literal(Literal::Float(f)) => {
                                        PhpType::literal_float(format!("-{}", bytes_to_str(f.raw)))
                                    }
                                    _ => ty,
                                }
                            }
                            _ => ty,
                        };
                        // Resolve any short class names in the arg type
                        // to FQN via the class loader.  Variable
                        // resolution may return raw docblock names
                        // (e.g. `SubscriptionProduct` instead of its
                        // FQN) — normalise them so that comparisons
                        // against parameter types (which are already
                        // FQN from resolve_parent_class_names) succeed.
                        let ty = ty.resolve_names(&|name: &str| {
                            // Don't FQN-ify anonymous class names — they
                            // use synthetic names (`__anonymous@<offset>`)
                            // that are only resolvable via the local-class
                            // shortcut in the class loader.  Prepending
                            // the namespace would break that lookup.
                            if name.contains("__anonymous@") {
                                return name.to_string();
                            }
                            if let Some(cls) = class_loader(name) {
                                cls.fqn().to_string()
                            } else {
                                name.to_string()
                            }
                        });
                        // Expand @phpstan-type / @psalm-type aliases so
                        // that e.g. `Payload` becomes `array{name: string,
                        // phone: string}` before the compatibility check.
                        let ty = crate::completion::types::resolution::resolve_type_alias_typed(
                            &ty,
                            &current_class_info.fqn(),
                            &file_ctx.classes,
                            &class_loader,
                        )
                        .unwrap_or(ty);
                        resolved_args.push(ResolvedArg { ty, start, end });
                    }
                    result.insert(
                        *args_start,
                        ResolvedCallArgs {
                            args: resolved_args,
                        },
                    );
                }
                (result, strict_types)
            });

        // Call-expression resolution cache: avoids re-resolving the
        // same call expression (e.g. `ClassName::method`) at every
        // call site that uses it.
        //
        // Only expressions that are guaranteed to resolve to the same
        // target everywhere in the file are cached.  Variable-based
        // calls (`$listener->handle`, `$repo->save`) are NOT cached
        // because the same variable name can hold different types in
        // different methods or after reassignment.  Static calls
        // (`Foo::bar`) and plain function calls (`array_map`) are
        // safe to cache.
        let mut call_cache: HashMap<String, Option<ResolvedCallableTarget>> = HashMap::new();

        // ── Walk every call site ────────────────────────────────────
        for call_site in &symbol_map.call_sites {
            // Skip calls with argument unpacking — actual types of
            // individual arguments are unknown.
            if call_site.has_unpacking {
                continue;
            }

            let expr = &call_site.call_expression;

            // Look up or populate the call expression cache.
            // Variable-based calls are resolved fresh every time
            // because the receiver variable may hold different types
            // at different call sites (e.g. `$listener->handle` in
            // two different test methods with different assignments).
            let is_variable_call = expr.starts_with('$');

            // Extract the raw argument text from the source so that
            // method-level @template parameters can be resolved from
            // the call-site argument types.
            let call_args_text: Option<&str> = {
                let start = call_site.args_start as usize;
                let end = call_site.args_end as usize;
                if let Some(slice) = content.get(start..end) {
                    let trimmed = slice.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                } else {
                    None
                }
            };

            // Resolve the callable target.  Class-level template
            // substitution happens inside resolve_callable_target
            // (driven by the variable's generic type string).
            // Method-level template substitution uses the per-site
            // argument text extracted above.
            //
            // Variable-based calls are always resolved per-site
            // because the receiver variable may hold different types
            // at different call sites.  Static/function calls are
            // also resolved per-site when argument text is available
            // (method-level template subs depend on per-site args);
            // only zero-arg calls can be cached.
            let resolved = if is_variable_call || call_args_text.is_some() {
                let position =
                    crate::util::offset_to_position(content, call_site.args_start as usize);
                self.resolve_callable_target_with_args(
                    expr,
                    content,
                    position,
                    &file_ctx,
                    call_args_text,
                )
            } else {
                call_cache
                    .entry(expr.clone())
                    .or_insert_with(|| {
                        let position =
                            crate::util::offset_to_position(content, call_site.args_start as usize);
                        self.resolve_callable_target(expr, content, position, &file_ctx)
                    })
                    .clone()
            };

            // Resolve the call expression to a callable target.
            let resolved = match resolved {
                Some(r) => r,
                None => continue,
            };

            // Get resolved argument types for this call site.
            let resolved_args = match resolved_map.get(&call_site.args_start) {
                Some(c) => c,
                None => continue,
            };

            let params = &resolved.parameters;

            // Track how many positional args we've seen so far for
            // mapping positional args to parameter indices.
            let mut positional_idx: usize = 0;

            // Check each argument against its parameter.
            for (arg_idx, resolved_arg) in resolved_args.args.iter().enumerate() {
                // Skip spread arguments.
                if call_site.spread_arg_indices.contains(&(arg_idx as u32)) {
                    continue;
                }

                // Find the corresponding parameter.
                let param = if call_site.named_arg_indices.contains(&(arg_idx as u32)) {
                    // Named argument: look up parameter by name.
                    let name_pos = call_site
                        .named_arg_indices
                        .iter()
                        .position(|&i| i == arg_idx as u32);
                    match name_pos {
                        Some(idx) => {
                            let param_name = &call_site.named_arg_names[idx];
                            params
                                .iter()
                                .find(|p| p.name.trim_start_matches('$') == param_name.as_str())
                        }
                        None => continue,
                    }
                } else {
                    // Positional argument.
                    let p = params.get(positional_idx);
                    positional_idx += 1;
                    p
                };

                let param = match param {
                    Some(p) => p,
                    None => continue, // Extra argument beyond declared params
                };

                // Skip if parameter has no type hint.
                let param_type = match &param.type_hint {
                    Some(t) if !t.is_untyped() && !t.is_mixed() => t,
                    _ => continue,
                };

                // Skip variadic parameters — hard to match individual
                // arg types to the variadic param's inner type.
                if param.is_variadic {
                    continue;
                }

                let arg_type = &resolved_arg.ty;

                // Skip unresolved / empty / Raw("") sentinel types.
                if arg_type.is_untyped()
                    || arg_type.is_empty()
                    || matches!(arg_type, PhpType::Raw(s) if s.is_empty())
                {
                    continue;
                }

                // Check compatibility.
                if is_type_compatible(arg_type, param_type, &class_loader, strict_types) {
                    continue;
                }

                // When the function has overloaded signatures, check
                // whether the argument is compatible with the same
                // positional parameter in any overload.  Only emit the
                // diagnostic when ALL signatures reject it.
                if !resolved.overloads.is_empty() {
                    let compatible_with_overload = resolved.overloads.iter().any(|alt_params| {
                        if let Some(alt_param) = alt_params.get(positional_idx.saturating_sub(1)) {
                            if let Some(ref alt_type) = alt_param.type_hint
                                && !alt_type.is_untyped()
                                && !alt_type.is_mixed()
                            {
                                return is_type_compatible(
                                    arg_type,
                                    alt_type,
                                    &class_loader,
                                    strict_types,
                                );
                            }
                            true // no type hint on alt param = compatible
                        } else {
                            // This overload has fewer params — the arg
                            // doesn't correspond to any parameter, so
                            // it's not relevant for this check.
                            false
                        }
                    });
                    if compatible_with_overload {
                        continue;
                    }
                }

                // Emit diagnostic.
                let range = match self.offset_range_to_lsp_range(
                    uri,
                    content,
                    resolved_arg.start,
                    resolved_arg.end,
                ) {
                    Some(r) => r,
                    None => continue,
                };

                let param_name = &param.name;
                // Always show full type names (FQN) so the developer
                // can actually find and fix the types.  Short names
                // strip the namespace which is the very information
                // needed to resolve the mismatch.
                let message = format!(
                    "Argument {} ({}) expects {}, got {}",
                    arg_idx + 1,
                    param_name,
                    param_type,
                    arg_type,
                );

                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::ERROR,
                    TYPE_MISMATCH_ARGUMENT_CODE,
                    message,
                ));
            }
        }
    }
}
