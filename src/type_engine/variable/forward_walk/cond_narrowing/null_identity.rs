use super::*;

/// Strip `null` from a subject that an identity comparison has matched
/// against a value that cannot be `null`.
///
/// `$a === $b` holding means both sides carried the same value, so a
/// nullable `$a` compared identical to a definitely-non-null `$b` holds
/// no null in that branch:
///
/// ```php
/// $name = $context->getName(); // ?string
/// if ($name === $node->name) {  // $node->name is string
///     takesString($name);       // string
/// }
/// ```
///
/// Only identity qualifies: `null == 0` and `null == false` are both
/// true, so a loose comparison proves nothing.
pub(super) fn apply_identity_comparison_null_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    let mut compared: Vec<(&Expression<'_>, &Expression<'_>)> = Vec::new();
    collect_identity_comparisons(condition, truthy, &mut compared);

    for (subject, comparand) in compared {
        let Some(key) =
            expr_to_var_name(subject).or_else(|| narrowing::expr_to_subject_key(subject))
        else {
            continue;
        };
        // The cheap half first: with no null to rule out there is
        // nothing to narrow, and resolving the comparand costs a full
        // type resolution.
        if !scope_value_is_nullable(&key, scope) || expr_accepts_null(comparand, scope, ctx) {
            continue;
        }
        strip_null_from_scope(&key, scope);
    }
}

/// Whether the value `expr` evaluates to could be `null`.
///
/// An expression that resolves to nothing counts as nullable: an unknown
/// value is not a proof.
pub(super) fn expr_accepts_null(
    expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    let var_ctx = build_var_ctx("", ctx, &scope_resolver);
    crate::type_engine::variable::resolution::resolve_arg_raw_type(expr, &var_ctx)
        .is_none_or(|ty| ty.accepts_null())
}

/// Collect the `(subject, comparand)` pairs of every identity comparison
/// the condition proves held, in both operand orders.
///
/// Whichever side a caller cares about, the identity holding means both
/// sides carried the same value, so a proof about one is a proof about
/// the other.  `A === B` holding and `A !== B` failing are the same
/// proof, which is why the `truthy` flag flips for `!==`.
pub(super) fn collect_identity_comparisons<'b>(
    condition: &'b Expression<'b>,
    truthy: bool,
    out: &mut Vec<(&'b Expression<'b>, &'b Expression<'b>)>,
) {
    match condition {
        Expression::Parenthesized(inner) => {
            collect_identity_comparisons(inner.expression, truthy, out);
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            collect_identity_comparisons(prefix.operand, !truthy, out);
        }
        Expression::Binary(bin) => {
            // `A && B` proves both when true; `A || B` proves neither
            // operand held when false.
            let decomposes = match bin.operator {
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_) => truthy,
                BinaryOperator::Or(_) | BinaryOperator::LowOr(_) => !truthy,
                _ => false,
            };
            if decomposes {
                collect_identity_comparisons(bin.lhs, truthy, out);
                collect_identity_comparisons(bin.rhs, truthy, out);
                return;
            }
            // Only identity qualifies: `null == false` and `null == 0` are
            // both true, so a loose comparison proves nothing.
            let holds = match bin.operator {
                BinaryOperator::Identical(_) => truthy,
                BinaryOperator::NotIdentical(_) => !truthy,
                _ => false,
            };
            if holds {
                out.push((bin.lhs, bin.rhs));
                out.push((bin.rhs, bin.lhs));
            }
        }
        _ => {}
    }
}

/// Carry a proof about one value's null back to every value whose null it
/// stands for: the receivers a `?->` chain would have short-circuited on,
/// and the variables a branch wrote alongside it.
///
/// Runs after the condition's own narrowing has landed, so what it reads
/// is the guarded state: a holder that is no longer nullable is one the
/// condition ruled the null out of, whichever shape the guard was written
/// in (`instanceof`, `!== null`, a bare truthy test, an assertion helper).
pub(super) fn apply_non_null_implication_narrowing(
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if !scope.non_null_implications.is_empty() {
        let proven: Vec<Atom> = scope
            .non_null_implications
            .iter()
            .filter(|(holder, _)| !scope_value_is_nullable(holder, scope))
            .flat_map(|(_, implieds)| implieds.iter().copied())
            .collect();
        for implied in proven {
            seed_synthetic_key_if_needed(&implied, scope, ctx);
            strip_null_from_scope(&implied, scope);
        }
    }

    if scope.implied_narrowings.is_empty() {
        return;
    }
    let proven: Vec<(Atom, Vec<ResolvedType>)> = scope
        .implied_narrowings
        .iter()
        .flat_map(|(holder, proofs)| {
            proofs
                .iter()
                .filter(|proof| trigger_holds(proof, holder, scope))
                .map(|proof| (proof.key, proof.types.clone()))
        })
        .collect();
    for (key, types) in proven {
        seed_synthetic_key_if_needed(&key, scope, ctx);
        if recorded_narrows_current(&types, scope.get(&key), ctx) {
            scope.set(&key, types);
        }
    }
}

/// Whether the scope now shows the holder to have taken the branch that
/// recorded `proof`.
///
/// A [`ProofTrigger::NonNull`] asks only that the holder's `null` is gone,
/// which is what a `!== null` test below the join establishes. A
/// [`ProofTrigger::Within`] asks that every type the holder can still be
/// is one the branch's own value could have been — the join only recorded
/// the proof because a holder inside that value cannot have come down the
/// other path. A [`ProofTrigger::Outside`] is the complement: nothing the
/// holder can still be is a value the *other* path left, so that path is
/// the one that did not run.
fn trigger_holds(proof: &ImpliedNarrowing, holder: &Atom, scope: &ScopeState) -> bool {
    let held = scope.get(holder);
    match &proof.trigger {
        ProofTrigger::NonNull => !scope_value_is_nullable(holder, scope),
        ProofTrigger::Within(trigger) => {
            !held.is_empty()
                && held.iter().all(|current| {
                    trigger
                        .iter()
                        .any(|want| current.type_string.is_subtype_of(&want.type_string))
                })
        }
        // Conservatively: two classes only count as contradicting each
        // other when the loader has been consulted, which
        // `types_are_disjoint` declines to guess at.  An `A` that is not
        // *spelled* `B` may still be one.
        ProofTrigger::Outside(trigger) => types_are_disjoint(held, trigger),
    }
}

/// Whether a recorded proof still refines what the scope knows.
///
/// The proof describes the value the key held on the branch that ran, so
/// it can only ever add to what is known — never replace it.  A guard
/// closer to the read has already narrowed further (the `&&` chain that
/// re-proves `$node->dim instanceof FuncCall` right where it is used),
/// and overwriting that with the coarser branch type would lose the more
/// specific answer.  An entry with no type is the one exception: unknown
/// is the top of the lattice, so anything recorded is narrower.
///
/// Which is also why a recorded `mixed` refines nothing.  It sits at the
/// same top of the lattice a missing entry does, so a branch that left the
/// key `mixed` beside a class the scope also holds says nothing the scope
/// does not already say — and applying it would drop that class, which is
/// the half every member lookup needs.
fn recorded_narrows_current(
    recorded: &[ResolvedType],
    current: &[ResolvedType],
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    if recorded.iter().any(|r| r.type_string.is_mixed()) {
        return false;
    }
    if current.is_empty() {
        return true;
    }
    let within = |narrow: &ResolvedType, wide: &ResolvedType| {
        if narrow.type_string == wide.type_string
            || narrow.type_string.is_subtype_of(&wide.type_string)
        {
            return true;
        }
        match (
            narrow.type_string.unwrap_nullable().class_name(),
            wide.type_string.unwrap_nullable().class_name(),
        ) {
            (Some(child), Some(parent)) => is_subclass_of(child, parent, ctx.class_loader),
            _ => false,
        }
    };
    let identical = recorded.len() == current.len()
        && recorded
            .iter()
            .all(|r| current.iter().any(|c| c.type_string == r.type_string));
    !identical
        && recorded
            .iter()
            .all(|r| current.iter().any(|c| within(r, c)))
}

/// Whether the scope's entry for `key` still admits `null`.
///
/// An entry with no types admits everything, so it counts as nullable:
/// an unknown value is not a proof.
pub(super) fn scope_value_is_nullable(key: &str, scope: &ScopeState) -> bool {
    let types = scope.get(key);
    types.is_empty()
        || types
            .iter()
            .any(|rt| rt.type_string.non_null_type().is_some() || rt.type_string == PhpType::null())
}

/// Collect the expressions `condition` proves are not `null` under the
/// given polarity.
///
/// Callers filter the result for the shapes they can act on, so a bare
/// truthy test contributes its whole subject rather than nothing.
pub(super) fn collect_proven_non_null_exprs<'b>(
    condition: &'b Expression<'b>,
    truthy: bool,
    out: &mut Vec<&'b Expression<'b>>,
) {
    match condition {
        Expression::Parenthesized(inner) => {
            collect_proven_non_null_exprs(inner.expression, truthy, out);
        }
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            collect_proven_non_null_exprs(prefix.operand, !truthy, out);
        }
        Expression::Binary(bin) => {
            // `A && B` proves both when true; `A || B` proves neither
            // operand held when false.  Either way each operand carries
            // the parent's polarity.
            let decomposes = match bin.operator {
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_) => truthy,
                BinaryOperator::Or(_) | BinaryOperator::LowOr(_) => !truthy,
                _ => false,
            };
            if decomposes {
                collect_proven_non_null_exprs(bin.lhs, truthy, out);
                collect_proven_non_null_exprs(bin.rhs, truthy, out);
                return;
            }

            let inequality = matches!(
                bin.operator,
                BinaryOperator::NotIdentical(_) | BinaryOperator::NotEqual(_)
            );
            let equality = matches!(
                bin.operator,
                BinaryOperator::Identical(_) | BinaryOperator::Equal(_)
            );
            if !inequality && !equality {
                return;
            }

            // `$x !== null` proves non-null when true, `$x === null` when
            // false.
            let proves_non_null = if inequality { truthy } else { !truthy };
            if is_null_expr(bin.rhs) {
                if proves_non_null {
                    out.push(bin.lhs);
                }
                return;
            }
            if is_null_expr(bin.lhs) {
                if proves_non_null {
                    out.push(bin.rhs);
                }
                return;
            }

            // A match against a value that is not null proves the other
            // side is not null either.  Only identity qualifies: `null ==
            // false` and `null == 0` are both true, so a loose comparison
            // against a falsy value proves nothing.
            if matches!(bin.operator, BinaryOperator::Identical(_)) && truthy {
                if exact_value_of_expr(bin.rhs).is_some_and(|(v, _)| v != ExactValue::Null) {
                    out.push(bin.lhs);
                }
                if exact_value_of_expr(bin.lhs).is_some_and(|(v, _)| v != ExactValue::Null) {
                    out.push(bin.rhs);
                }
            }
        }
        // A bare truthy test: anything truthy is non-null.
        _ if truthy => out.push(condition),
        _ => {}
    }
}

/// Strip `null` from `var_name` when the constant it was proven identical
/// to cannot be null.
///
/// Identity is only ever true between two values of the same type, so a
/// constant that holds no null leaves none in the subject. A constant
/// that does — `const NONE = null;`, or one whose type we cannot read —
/// proves nothing and is left alone.
pub(super) fn strip_null_by_constant_identity(
    var_name: &str,
    constant: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if scope.get(var_name).is_empty() {
        seed_synthetic_key_if_needed(var_name, scope, ctx);
        if scope.get(var_name).is_empty() {
            return;
        }
    }
    let constant_types = super::assignment::resolve_rhs_with_scope(constant, scope, ctx);
    if constant_types.is_empty() {
        return;
    }
    if constant_types
        .iter()
        .any(|rt| type_admits_null(&rt.type_string))
    {
        return;
    }
    strip_null_from_scope(var_name, scope);
}

/// Whether a type has a `null` among the values it describes.
fn type_admits_null(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Nullable(_) => true,
        TypeKind::Union(members) => members.iter().any(type_admits_null),
        _ => ty.is_null() || ty.is_mixed(),
    }
}
