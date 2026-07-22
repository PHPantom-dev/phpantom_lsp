use super::*;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use crate::atom::Atom;
use crate::completion::types::narrowing;

// ─── `&&` chain narrowing for diagnostic scope snapshots ────────────────────

/// Collect operands of a `&&` chain into a left-to-right list.
///
/// `a && b && c` is parsed as `(a && b) && c`.  This function flattens
/// it into `[a, b, c]`.  Non-`&&` expressions return a single-element
/// list.
pub(crate) fn collect_and_chain_operands<'b>(expr: &'b Expression<'b>) -> Vec<&'b Expression<'b>> {
    let mut operands = Vec::new();
    collect_and_chain_operands_inner(expr, &mut operands);
    operands
}

pub(crate) fn collect_and_chain_operands_inner<'b>(
    expr: &'b Expression<'b>,
    out: &mut Vec<&'b Expression<'b>>,
) {
    if let Expression::Binary(bin) = expr
        && matches!(
            bin.operator,
            BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
        )
    {
        collect_and_chain_operands_inner(bin.lhs, out);
        collect_and_chain_operands_inner(bin.rhs, out);
        return;
    }
    // Also unwrap parenthesised `&&` chains.
    if let Expression::Parenthesized(inner) = expr {
        let inner_ops = collect_and_chain_operands(inner.expression);
        if inner_ops.len() > 1 {
            out.extend(inner_ops);
            return;
        }
    }
    out.push(expr);
}

pub(crate) fn collect_or_chain_operands<'b>(expr: &'b Expression<'b>) -> Vec<&'b Expression<'b>> {
    let mut operands = Vec::new();
    collect_or_chain_operands_inner(expr, &mut operands);
    operands
}

pub(crate) fn collect_or_chain_operands_inner<'b>(
    expr: &'b Expression<'b>,
    out: &mut Vec<&'b Expression<'b>>,
) {
    if let Expression::Binary(bin) = expr
        && matches!(
            bin.operator,
            BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
        )
    {
        collect_or_chain_operands_inner(bin.lhs, out);
        collect_or_chain_operands_inner(bin.rhs, out);
        return;
    }
    // Also unwrap parenthesised `||` chains.
    if let Expression::Parenthesized(inner) = expr {
        let inner_ops = collect_or_chain_operands(inner.expression);
        if inner_ops.len() > 1 {
            out.extend(inner_ops);
            return;
        }
    }
    out.push(expr);
}

/// Walk an expression tree looking for `match(true)` arms and ternary
/// `instanceof` patterns.  When found, clone the scope, apply per-arm
/// or per-branch narrowing, and record scope snapshots so that member
/// accesses inside the narrowed context see the correct variable types.
///
/// Unlike [`record_scope_snapshot_recursive`], this function does NOT
/// record snapshots at every sub-expression offset.  It only writes
/// snapshots at offsets inside match arms and ternary branches where
/// narrowing applies.  This avoids polluting the scope cache with
/// redundant entries that could conflict with `&&`-chain snapshots.
pub(crate) fn record_match_ternary_snapshots<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match expr {
        Expression::Match(match_expr) if match_expr.expression.is_true() => {
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(expr_arm) => {
                        let mut arm_scope = scope.clone();
                        for condition in expr_arm.conditions.iter() {
                            apply_condition_narrowing(condition, &mut arm_scope, ctx);
                        }
                        record_scope_snapshot(expr_arm.expression.span().start.offset, &arm_scope);
                        record_scope_snapshot_recursive(expr_arm.expression, &arm_scope);
                        // Recurse into the arm body for nested patterns.
                        record_match_ternary_snapshots(expr_arm.expression, &arm_scope, ctx);
                    }
                    MatchArm::Default(def_arm) => {
                        record_scope_snapshot(def_arm.expression.span().start.offset, scope);
                        record_scope_snapshot_recursive(def_arm.expression, scope);
                        record_match_ternary_snapshots(def_arm.expression, scope, ctx);
                    }
                }
            }
        }
        Expression::Conditional(conditional) => {
            // Only apply narrowing when the condition adds information the
            // guarded branch relies on: an instanceof check (simple or
            // compound OR) or a member-existence proof
            // (`property_exists`/`method_exists`/`isset($x->prop)`).
            // General truthiness/null narrowing is too broad and can
            // produce incorrect scope snapshots for arbitrary ternaries.
            let has_narrowing = {
                let var_names: Vec<Atom> = scope.locals.keys().copied().collect();
                var_names.iter().any(|vn| {
                    narrowing::try_extract_instanceof(conditional.condition, vn).is_some()
                        || narrowing::try_extract_compound_or_instanceof(conditional.condition, vn)
                            .is_some()
                })
            } || condition_proves_member(conditional.condition, scope);
            if has_narrowing {
                let mut then_scope = scope.clone();
                apply_condition_narrowing(conditional.condition, &mut then_scope, ctx);
                if let Some(then_expr) = conditional.then {
                    record_scope_snapshot(then_expr.span().start.offset, &then_scope);
                    record_scope_snapshot_recursive(then_expr, &then_scope);
                    record_match_ternary_snapshots(then_expr, &then_scope, ctx);
                }
                let mut else_scope = scope.clone();
                apply_condition_narrowing_inverse(conditional.condition, &mut else_scope, ctx);
                record_scope_snapshot(conditional.r#else.span().start.offset, &else_scope);
                record_scope_snapshot_recursive(conditional.r#else, &else_scope);
                record_match_ternary_snapshots(conditional.r#else, &else_scope, ctx);
            } else {
                // No instanceof — just recurse for nested patterns.
                if let Some(then_expr) = conditional.then {
                    record_match_ternary_snapshots(then_expr, scope, ctx);
                }
                record_match_ternary_snapshots(conditional.r#else, scope, ctx);
            }
        }
        Expression::Assignment(assignment) => {
            record_match_ternary_snapshots(assignment.rhs, scope, ctx);
        }
        Expression::Parenthesized(inner) => {
            record_match_ternary_snapshots(inner.expression, scope, ctx);
        }
        Expression::Call(call) => {
            let args = match call {
                Call::Function(fc) => {
                    record_match_ternary_snapshots(fc.function, scope, ctx);
                    &fc.argument_list
                }
                Call::Method(mc) => {
                    record_match_ternary_snapshots(mc.object, scope, ctx);
                    &mc.argument_list
                }
                Call::NullSafeMethod(mc) => {
                    record_match_ternary_snapshots(mc.object, scope, ctx);
                    &mc.argument_list
                }
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for arg in args.arguments.iter() {
                let arg_expr = match arg {
                    Argument::Positional(a) => a.value,
                    Argument::Named(a) => a.value,
                };
                record_match_ternary_snapshots(arg_expr, scope, ctx);
            }
        }
        Expression::Binary(bin) => {
            record_match_ternary_snapshots(bin.lhs, scope, ctx);
            record_match_ternary_snapshots(bin.rhs, scope, ctx);
        }
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                let elem_expr = match elem {
                    ArrayElement::KeyValue(kv) => {
                        record_match_ternary_snapshots(kv.key, scope, ctx);
                        kv.value
                    }
                    ArrayElement::Value(val) => val.value,
                    ArrayElement::Variadic(v) => v.value,
                    ArrayElement::Missing(_) => continue,
                };
                record_match_ternary_snapshots(elem_expr, scope, ctx);
            }
        }
        // Match expressions where the subject is NOT `true` — just
        // recurse into arm expressions.
        Expression::Match(match_expr) => {
            for arm in match_expr.arms.iter() {
                let arm_expr = match arm {
                    MatchArm::Expression(e) => e.expression,
                    MatchArm::Default(d) => d.expression,
                };
                record_match_ternary_snapshots(arm_expr, scope, ctx);
            }
        }
        _ => {}
    }
}

/// Record intermediate scope snapshots within `&&` chains.
///
/// When the diagnostic scope cache is active and an expression contains
/// a `&&` chain, this function:
///
/// 1. Collects the operands left-to-right.
/// 2. For each operand after the first, applies instanceof and null
///    narrowing from all previous operands to a temporary scope.
/// 3. Records a scope snapshot at the operand's byte offset so that
///    diagnostic member-access lookups within the operand see the
///    narrowed types.
///
/// This fixes patterns like:
/// - `return $x instanceof Foo && $x->bar()` — `$x` narrowed to `Foo`
///   for the `$x->bar()` span.
/// - `$x !== null && $x->method()` — `$x` narrowed to non-null for
///   the `$x->method()` span.
///
/// The narrowing is applied only to snapshots — it does NOT mutate the
/// caller's scope, so subsequent statements see the original types.
pub(crate) fn record_and_chain_snapshots<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if !is_diagnostic_scope_active() {
        return;
    }

    let operands = collect_and_chain_operands(expr);
    if operands.len() < 2 {
        // Not a `&&` chain — nothing to do.  The scope snapshot at
        // the statement boundary already covers single expressions.
        // Match(true) and ternary narrowing are handled separately
        // by `record_match_ternary_snapshots`.
        return;
    }

    // Apply narrowing cumulatively: each operand sees the narrowing
    // from all previous operands.
    let mut narrowed_scope = scope.clone();
    for (i, operand) in operands.iter().enumerate() {
        if i == 0 {
            // First operand: apply its narrowing for subsequent operands.
            apply_condition_narrowing(operand, &mut narrowed_scope, ctx);
            continue;
        }

        // Record a snapshot at this operand's start offset so that
        // member accesses within it see the narrowed types.
        record_scope_snapshot(operand.span().start.offset, &narrowed_scope);

        // Also recurse into sub-expressions of this operand that might
        // contain member accesses at deeper byte offsets.  For example,
        // `is_array($x->errorInfo)` — the access `$x->errorInfo` is
        // inside a function call argument.
        record_scope_snapshot_recursive(operand, &narrowed_scope);

        // Refine nested `&&` / `||` chains inside this operand on top of
        // the accumulated narrowing.  E.g. `$a && ($b instanceof Foo ||
        // $c) && $a->m()` — the inner `||` operands narrow independently.
        // These overwrite the coarser snapshots recorded above at the
        // offsets that carry intra-chain narrowing.
        record_and_chain_snapshots(operand, &narrowed_scope, ctx);
        record_or_chain_snapshots(operand, &narrowed_scope, ctx);

        // Apply this operand's narrowing for the next operand.
        apply_condition_narrowing(operand, &mut narrowed_scope, ctx);
    }
}

/// Record intermediate scope snapshots within `||` chains.
///
/// The right operand of `||` executes only when every preceding
/// operand evaluated to false, so each operand after the first sees
/// the *inverse* narrowing of all operands before it. This is the
/// mirror of [`record_and_chain_snapshots`]:
///
/// - `!$x instanceof Foo || $x->bar()` — `$x` narrowed to `Foo` for
///   the `$x->bar()` span (the negation of `!$x instanceof Foo`).
/// - `$x === null || $x->method()` — `$x` narrowed to non-null for
///   the `$x->method()` span.
///
/// As with the `&&` variant, the narrowing is applied only to
/// snapshots — it does NOT mutate the caller's scope, so subsequent
/// statements see the original types.
pub(crate) fn record_or_chain_snapshots<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    if !is_diagnostic_scope_active() {
        return;
    }

    let operands = collect_or_chain_operands(expr);
    if operands.len() < 2 {
        // Not a `||` chain — the `&&` and match/ternary snapshot
        // recorders handle the other shapes.
        return;
    }

    // Apply the inverse narrowing cumulatively: each operand sees the
    // negation of all previous operands.
    let mut narrowed_scope = scope.clone();
    for (i, operand) in operands.iter().enumerate() {
        if i == 0 {
            // First operand: apply its inverse for subsequent operands.
            apply_condition_narrowing_inverse(operand, &mut narrowed_scope, ctx);
            continue;
        }

        // Record a snapshot at this operand's start offset, and recurse
        // into sub-expressions so member accesses nested inside calls,
        // negations, etc. see the narrowed types.
        record_scope_snapshot(operand.span().start.offset, &narrowed_scope);
        record_scope_snapshot_recursive(operand, &narrowed_scope);

        // Refine nested `&&` / `||` chains inside this operand on top of
        // the accumulated inverse narrowing.  E.g. the common idiom
        // `$parent->child() !== $node || ($x instanceof Foo && !$x->m())`
        // — the inner `&&` narrows `$x` to `Foo` for `$x->m()`.  These
        // overwrite the coarser snapshots recorded above at the offsets
        // that carry intra-chain narrowing.
        record_and_chain_snapshots(operand, &narrowed_scope, ctx);
        record_or_chain_snapshots(operand, &narrowed_scope, ctx);

        // Apply this operand's inverse for the next operand.
        apply_condition_narrowing_inverse(operand, &mut narrowed_scope, ctx);
    }
}

/// Recursively record scope snapshots at every sub-expression offset
/// within an expression.  This ensures that member accesses nested
/// inside function calls, array accesses, ternaries, etc. within a
/// `&&` chain operand see the narrowed scope.
pub(crate) fn record_scope_snapshot_recursive(expr: &Expression<'_>, scope: &ScopeState) {
    match expr {
        Expression::Call(call) => {
            let args = match call {
                Call::Function(fc) => {
                    // Record at the function call's argument list.
                    for arg in fc.argument_list.arguments.iter() {
                        let arg_expr = match arg {
                            Argument::Positional(a) => a.value,
                            Argument::Named(a) => a.value,
                        };
                        record_scope_snapshot(arg_expr.span().start.offset, scope);
                        record_scope_snapshot_recursive(arg_expr, scope);
                    }
                    return;
                }
                Call::Method(mc) => {
                    record_scope_snapshot(mc.object.span().start.offset, scope);
                    record_scope_snapshot_recursive(mc.object, scope);
                    &mc.argument_list
                }
                Call::NullSafeMethod(mc) => {
                    record_scope_snapshot(mc.object.span().start.offset, scope);
                    record_scope_snapshot_recursive(mc.object, scope);
                    &mc.argument_list
                }
                Call::StaticMethod(sc) => &sc.argument_list,
            };
            for arg in args.arguments.iter() {
                let arg_expr = match arg {
                    Argument::Positional(a) => a.value,
                    Argument::Named(a) => a.value,
                };
                record_scope_snapshot(arg_expr.span().start.offset, scope);
                record_scope_snapshot_recursive(arg_expr, scope);
            }
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => {
                record_scope_snapshot(pa.object.span().start.offset, scope);
                record_scope_snapshot_recursive(pa.object, scope);
            }
            Access::NullSafeProperty(pa) => {
                record_scope_snapshot(pa.object.span().start.offset, scope);
                record_scope_snapshot_recursive(pa.object, scope);
            }
            Access::StaticProperty(sp) => {
                record_scope_snapshot(sp.span().start.offset, scope);
            }
            Access::ClassConstant(cc) => {
                record_scope_snapshot(cc.span().start.offset, scope);
            }
        },
        Expression::Parenthesized(inner) => {
            record_scope_snapshot(inner.expression.span().start.offset, scope);
            record_scope_snapshot_recursive(inner.expression, scope);
        }
        Expression::Binary(bin) => {
            record_scope_snapshot(bin.lhs.span().start.offset, scope);
            record_scope_snapshot_recursive(bin.lhs, scope);
            record_scope_snapshot(bin.rhs.span().start.offset, scope);
            record_scope_snapshot_recursive(bin.rhs, scope);
        }
        Expression::UnaryPrefix(prefix) => {
            record_scope_snapshot(prefix.operand.span().start.offset, scope);
            record_scope_snapshot_recursive(prefix.operand, scope);
        }
        Expression::Conditional(conditional) => {
            if let Some(then_expr) = conditional.then {
                record_scope_snapshot(then_expr.span().start.offset, scope);
                record_scope_snapshot_recursive(then_expr, scope);
            }
            record_scope_snapshot(conditional.r#else.span().start.offset, scope);
            record_scope_snapshot_recursive(conditional.r#else, scope);
        }
        Expression::ArrayAccess(aa) => {
            record_scope_snapshot(aa.array.span().start.offset, scope);
            record_scope_snapshot_recursive(aa.array, scope);
        }
        _ => {}
    }
}
