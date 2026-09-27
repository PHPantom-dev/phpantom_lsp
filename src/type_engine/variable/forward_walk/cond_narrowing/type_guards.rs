use super::*;

/// Apply type-guard narrowing in the truthy branch.
///
/// When `is_object($var)` (or `is_array`, `is_string`, etc.) appears
/// in a condition, narrow the variable's type.  For `mixed` variables,
/// this replaces `mixed` with the guard's canonical type (e.g. `object`).
/// For union types, it filters to only the members that match the guard.
///
/// Handles compound `&&` conditions by decomposing them into individual
/// operands and applying each type guard found.  For example,
/// `is_object($data) && property_exists($data, 'error_link')` applies
/// the `is_object` guard to `$data`.
pub(crate) fn apply_type_guard_narrowing_truthy(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    apply_type_guard_on_operands(condition, scope, true, ctx);
}

/// Apply type-guard narrowing in the inverse (else) branch.
///
/// When `is_object($var)` appears in a condition, the else branch
/// knows the variable is NOT an object — filter out object-like
/// members from the union type.
pub(crate) fn apply_type_guard_narrowing_inverse(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    apply_type_guard_on_operands(condition, scope, false, ctx);
}

/// Shared implementation for truthy and inverse type-guard narrowing.
///
/// Decomposes `&&` chains into individual operands and applies each
/// type guard found.  When `truthy` is `true`, applies inclusion
/// narrowing (then-body); when `false`, applies exclusion (else-body).
pub(crate) fn apply_type_guard_on_operands(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    truthy: bool,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Decompose `&&` chains so that `is_object($x) && is_string($y)`
    // applies both guards.
    let operands = collect_and_chain_operands(condition);
    let mut var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    // Include property access keys from conditions (e.g. `$a->foo`
    // from `is_string($a->foo)`) so they can be narrowed.
    for key in collect_condition_property_keys(condition) {
        if !var_names.contains(&key) {
            var_names.push(key);
        }
    }
    // Include plain variables the condition names but the scope has no
    // type for — a guard on a value read from an unknown source (a
    // `stdClass` property, an untyped array offset) is the only thing
    // that says what it is, so it must not be skipped for want of a
    // prior type.
    for name in collect_condition_var_names(condition) {
        if !var_names.contains(&name) {
            var_names.push(name);
        }
    }
    for operand in &operands {
        for var_name in &var_names {
            if let Some((kind, negated)) = narrowing::try_extract_type_guard(operand, var_name) {
                // When the guard is negated (e.g. `!is_object($x)`),
                // flip the inclusion/exclusion logic: the truthy branch
                // of a negated guard means the variable is NOT the
                // guarded type, and vice versa.
                let effective_truthy = if negated { !truthy } else { truthy };
                let splits_iterable = matches!(
                    kind,
                    narrowing::TypeGuardKind::Array | narrowing::TypeGuardKind::Object
                );
                let mut results = splits_iterable
                    .then(|| {
                        split_iterable_alternatives(scope.get(var_name), |hint| {
                            ctx.resolved_types_for(hint)
                        })
                    })
                    .flatten()
                    .unwrap_or_else(|| scope.get(var_name).to_vec());
                if results.is_empty() {
                    // Nothing known about the subject.  A guard that
                    // holds still proves its type outright; one that
                    // fails only rules a type out, which says nothing
                    // on its own.
                    if effective_truthy {
                        scope.set(
                            var_name,
                            vec![ResolvedType::from_type_string(
                                narrowing::guard_kind_to_narrowed_type(kind),
                            )],
                        );
                    }
                    continue;
                }
                if effective_truthy {
                    narrowing::apply_type_guard_inclusion(
                        kind,
                        &mut results,
                        Some(ctx.class_loader),
                    );
                } else {
                    narrowing::apply_type_guard_exclusion(
                        kind,
                        &mut results,
                        Some(ctx.class_loader),
                    );
                }
                if results.is_empty() {
                    mark_exhausted(var_name, scope);
                } else {
                    scope.set(var_name, results);
                }
            }
        }
    }
}

/// Narrow a union of object types by a check on a property that only some
/// of its members could have passed.
///
/// `is_string($b->v)` on a `StrBox|IntBox` subject proves the value is a
/// `StrBox` when `IntBox::$v` is declared `int`: no `IntBox` reaches the
/// then-body.  An identity check against a literal (`$b->v === 'x'`)
/// discriminates the same way.  A member is only ever dropped when its
/// own declaration rules the check out, so a property whose type is
/// unknown, wide, or shared across the union leaves the subject alone.
pub(crate) fn apply_property_discriminant_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    // `&&` proves each of its operands where the body runs.  Its inverse
    // proves none of them on its own (`!(A && B)` leaves both open), so
    // the else branch only reads a condition that stands alone.
    let operands = if truthy {
        collect_and_chain_operands(condition)
    } else {
        vec![condition]
    };
    for operand in operands {
        if let Some(check) = extract_property_check(operand, truthy) {
            narrow_union_by_property_check(&check, scope, ctx);
        }
    }
}

/// Apply `is_a($x, Class::class, true)` / `class_exists($x)` (and the
/// other `*_exists()` forms) class-string narrowing.
///
/// When the guard's effective truth value is `true`, narrows string-like
/// (and `mixed`) entries in `$x`'s type to `class-string<Class>` (or
/// bare `class-string` for the generic `*_exists()` forms, which don't
/// name a specific class).  Negation is resolved by
/// `try_extract_class_string_guard`, so passing `truthy = false` here
/// from a guard-clause inverse correctly re-derives the truthy narrowing
/// for a negated condition (`if (!is_a(...)) { throw; }`).
///
/// Object-typed entries (with `class_info` set) are left untouched —
/// `is_a()`'s object side is already narrowed by the existing
/// instanceof-style handling, which operates independently on the
/// class-bearing entries.
pub(crate) fn apply_class_string_guard_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    let operands = collect_and_chain_operands(condition);
    let mut var_names: Vec<String> = scope.locals.keys().map(|k| k.to_string()).collect();
    for key in collect_condition_property_keys(condition) {
        if !var_names.contains(&key) {
            var_names.push(key);
        }
    }
    for operand in &operands {
        for var_name in &var_names {
            if let Some((target, negated)) =
                narrowing::try_extract_class_string_guard(operand, var_name)
            {
                let effective_truthy = if negated { !truthy } else { truthy };
                if !effective_truthy {
                    continue;
                }
                // Seed compound subject keys (`$arr['class']`, `$obj->prop`)
                // so a class-string guard on an array-index or property
                // subject narrows just like one on a plain variable.  An
                // untyped array index seeds as `mixed`, which the loop below
                // narrows to `class-string<Class>`.
                seed_synthetic_key_if_needed(var_name, scope, ctx);
                let mut results = scope.get(var_name).to_vec();
                if results.is_empty() {
                    continue;
                }
                let resolved_fqn = target
                    .as_deref()
                    .map(|name| crate::util::resolve_name_via_loader(name, ctx.class_loader));
                let class_string_type = match &resolved_fqn {
                    Some(fqn) => PhpType::parse(&format!("class-string<{}>", fqn)),
                    None => PhpType::parse("class-string"),
                };
                let mut changed = false;
                for rt in results.iter_mut() {
                    if rt.class_info.is_some() {
                        continue;
                    }
                    // Never widen a type that is already at least as
                    // specific as the guard's result. The generic
                    // `*_exists()` forms narrow to bare `class-string`; a
                    // variable already typed `class-string<Foo>` must keep
                    // its type argument rather than be downgraded (a bare
                    // `class-string` is a supertype, so `new $var` could no
                    // longer recover the concrete class).
                    if rt.type_string.is_subtype_of(&class_string_type)
                        || resolved_fqn
                            .as_deref()
                            .is_some_and(|fqn| names_only_subclasses_of(&rt.type_string, fqn, ctx))
                    {
                        continue;
                    }
                    if rt.type_string.is_subtype_of(&PhpType::string()) || rt.type_string.is_mixed()
                    {
                        rt.type_string = class_string_type.clone();
                        changed = true;
                    }
                }
                if changed {
                    scope.set(var_name, results);
                }
            }
        }
    }
}

/// Whether every alternative of `ty` is a class name already known to be
/// `fqn` or below it: a `class-string<Bar>`, or a literal naming `Bar`,
/// when `Bar extends Foo` and the check is against `Foo`.
///
/// The structural subtype test cannot see a class hierarchy, so without
/// this the guard replaced `class-string<Bar>` with the wider
/// `class-string<Foo>` it had just proved.
fn names_only_subclasses_of(ty: &PhpType, fqn: &str, ctx: &ForwardWalkCtx<'_>) -> bool {
    ty.union_members().iter().all(|member| {
        let named = match member.kind() {
            TypeKind::ClassString(Some(inner)) => inner.class_name().map(str::to_string),
            _ => member
                .as_literal()
                .and_then(LiteralValue::string_content)
                .map(|name| name.into_owned()),
        };
        named.is_some_and(|name| {
            crate::class_lookup::is_subtype_of_names(&name, fqn, ctx.class_loader)
        })
    })
}
