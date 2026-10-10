use super::*;

use mago_span::HasSpan;
use mago_syntax::cst::argument::Argument;

use std::sync::Arc;

use crate::type_engine::types::narrowing;

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
    out.push(narrowing::fold_negation_pairs(expr));
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
    out.push(narrowing::fold_negation_pairs(expr));
}

/// Walk an expression tree looking for `match(true)` arms and ternary
/// `instanceof` patterns.  When found, clone the scope, apply per-arm
/// or per-branch narrowing, and record scope snapshots so that member
/// accesses inside the narrowed context see the correct variable types.
///
/// Unlike [`record_scope_snapshot_recursive`], this function does NOT
/// record snapshots at every sub-expression offset.  It writes them only
/// inside match arms and ternary branches, where the branch scope can
/// legitimately differ from the enclosing one.  This avoids polluting the
/// scope cache with entries that could conflict with the `&&`-chain
/// snapshots [`process_expr`] records.
pub(crate) fn record_match_ternary_snapshots<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    match expr {
        Expression::Match(match_expr) if match_expr.expression.is_true() => {
            // Reaching an arm means every arm above it was tested and
            // failed, so the inverse of each of their conditions holds in
            // its body, exactly as in an `elseif` chain.  `default` runs
            // only once every other arm failed, wherever it is written.
            let mut failed_scope = scope.clone();
            let mut default_expr = None;
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(expr_arm) => {
                        let mut arm_scope = failed_scope.clone();
                        apply_match_arm_narrowing(expr_arm, &mut arm_scope, ctx);
                        // A condition is itself evaluated only once every
                        // condition before it failed, so a chain or a
                        // ternary inside it starts from that.
                        for condition in expr_arm.conditions.iter() {
                            record_branch_snapshots(condition, &failed_scope, ctx);
                            apply_failed_match_condition_narrowing(
                                condition,
                                &mut failed_scope,
                                ctx,
                            );
                        }
                        record_branch_snapshots(expr_arm.expression, &arm_scope, ctx);
                    }
                    MatchArm::Default(def_arm) => default_expr = Some(def_arm.expression),
                }
            }
            if let Some(default_expr) = default_expr {
                record_branch_snapshots(default_expr, &failed_scope, ctx);
            }
            restore_after_branches(expr, scope);
        }
        Expression::Conditional(conditional) => {
            // Each arm is evaluated under its own polarity of the condition,
            // exactly like an `if`/`else` body, and does so unconditionally:
            // gating on a list of recognised condition shapes silently
            // missed every form added to the narrowing pipeline afterwards,
            // and left the arm offsets with no snapshot at all, so the
            // diagnostic fell back to whatever scope it could find nearby.
            if let Some(then_expr) = conditional.then {
                let mut then_scope = scope.clone();
                apply_condition_narrowing(conditional.condition, &mut then_scope, ctx);
                record_branch_snapshots(then_expr, &then_scope, ctx);
            }

            let mut else_scope = scope.clone();
            apply_condition_narrowing_inverse(conditional.condition, &mut else_scope, ctx);
            record_branch_snapshots(conditional.r#else, &else_scope, ctx);
            restore_after_branches(expr, scope);
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
        // `new Foo($x ? $x->name() : '')` holds a ternary in the same
        // position a call does, and its branches narrow the same way.
        Expression::Instantiation(inst) => {
            if let Some(args) = &inst.argument_list {
                for arg in args.arguments.iter() {
                    let arg_expr = match arg {
                        Argument::Positional(a) => a.value,
                        Argument::Named(a) => a.value,
                    };
                    record_match_ternary_snapshots(arg_expr, scope, ctx);
                }
            }
        }
        // The right operand of `&&` runs only once the left one was truthy,
        // and that of `||` only once it was falsy, so a ternary or `match`
        // inside it starts from what that proved, the way `process_expr`
        // reads the operand itself.
        Expression::Binary(bin) => {
            record_match_ternary_snapshots(bin.lhs, scope, ctx);
            match short_circuit_kind(expr) {
                Some(kind) if holds_branches(bin.rhs) => {
                    let mut rhs_scope = scope.clone();
                    match kind {
                        ChainKind::And => apply_condition_narrowing(bin.lhs, &mut rhs_scope, ctx),
                        ChainKind::Or => {
                            apply_condition_narrowing_inverse(bin.lhs, &mut rhs_scope, ctx)
                        }
                    }
                    record_match_ternary_snapshots(bin.rhs, &rhs_scope, ctx);
                }
                _ => record_match_ternary_snapshots(bin.rhs, scope, ctx),
            }
        }
        // A `throw` is an expression since PHP 8, and the value it throws
        // is built the same way any other is: `throw new Exception($x ? …
        // : …)` narrows its branches exactly as the identical `return`
        // would.
        Expression::Throw(throw_expr) => {
            record_match_ternary_snapshots(throw_expr.exception, scope, ctx);
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
        // Match expressions where the subject is NOT `true`.  A
        // `match ($x::class)` subject is still narrowed per arm; any other
        // subject just gets recursed into.
        Expression::Match(match_expr) => {
            let subject_var = narrowing::match_class_subject_var(match_expr.expression);
            for arm in match_expr.arms.iter() {
                let arm_expr = match arm {
                    MatchArm::Expression(e) => e.expression,
                    MatchArm::Default(d) => d.expression,
                };
                match (subject_var, arm) {
                    (Some(var), MatchArm::Expression(expr_arm)) => {
                        let mut arm_scope = scope.clone();
                        apply_class_match_arm_narrowing(var, expr_arm, &mut arm_scope, ctx);
                        record_branch_snapshots(arm_expr, &arm_scope, ctx);
                    }
                    _ => record_branch_snapshots(arm_expr, scope, ctx),
                }
            }
            restore_after_branches(expr, scope);
        }
        _ => {}
    }
}

/// Whether `expr` holds a ternary or `match` that
/// [`record_match_ternary_snapshots`] records branch snapshots for, found
/// through the same expression shapes it descends through.
///
/// Narrowing a scope for a short-circuit operand only pays off when there is
/// a branch inside it to hand the scope to, so this is asked first.
pub(crate) fn holds_branches(expr: &Expression<'_>) -> bool {
    let any_argument = |args: &ArgumentList<'_>| {
        args.arguments
            .iter()
            .any(|arg| holds_branches(argument_value(arg)))
    };
    match expr {
        Expression::Match(_) | Expression::Conditional(_) => true,
        Expression::Assignment(assignment) => holds_branches(assignment.rhs),
        Expression::Parenthesized(inner) => holds_branches(inner.expression),
        Expression::Call(call) => match call {
            Call::Function(fc) => holds_branches(fc.function) || any_argument(&fc.argument_list),
            Call::Method(mc) => holds_branches(mc.object) || any_argument(&mc.argument_list),
            Call::NullSafeMethod(mc) => {
                holds_branches(mc.object) || any_argument(&mc.argument_list)
            }
            Call::StaticMethod(sc) => any_argument(&sc.argument_list),
        },
        Expression::Instantiation(inst) => inst.argument_list.as_ref().is_some_and(any_argument),
        Expression::Binary(bin) => holds_branches(bin.lhs) || holds_branches(bin.rhs),
        Expression::Throw(throw_expr) => holds_branches(throw_expr.exception),
        Expression::Array(arr) => arr.elements.iter().any(|elem| match elem {
            ArrayElement::KeyValue(kv) => holds_branches(kv.key) || holds_branches(kv.value),
            ArrayElement::Value(val) => holds_branches(val.value),
            ArrayElement::Variadic(v) => holds_branches(v.value),
            ArrayElement::Missing(_) => false,
        }),
        _ => false,
    }
}

/// Put the enclosing scope back where a ternary or `match` ends.
///
/// A lookup reads the nearest snapshot at or before its offset, so without
/// this the code after the expression (the next argument of
/// `f($x instanceof A ? 1 : 2, $x->a())`) would read the last branch's
/// narrowing as its own.
fn restore_after_branches(expr: &Expression<'_>, scope: &ScopeState) {
    record_scope_snapshot(expr.span().end.offset, scope);
}

/// Record every snapshot a branch body needs, given the scope that body
/// runs under.
///
/// A match arm and a ternary branch are expression positions with their
/// own scope, and everything an expression statement gets has to reach
/// them: the scope at the branch offset, the same scope at each nested
/// offset inside it, the intra-`&&`/`||` refinements the branch's own
/// chain makes about its operands, and the nested branches below it.
/// The chain recorders come last so their finer snapshots overwrite the
/// flat ones at the offsets both cover.
fn record_branch_snapshots<'b>(
    expr: &'b Expression<'b>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    record_scope_snapshot(expr.span().start.offset, scope);
    record_scope_snapshot_recursive(expr, scope);
    // The branch runs on its own copy: what it writes is the enclosing
    // walk's to apply, once it has joined the branches.
    process_expr(expr, &mut scope.clone(), ctx);
    record_match_ternary_snapshots(expr, scope, ctx);
}

/// Which short-circuit operator joins a chain's operands.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ChainKind {
    /// `&&` / `and`: every operand after the first runs only when all
    /// the ones before it were truthy.
    And,
    /// `||` / `or`: every operand after the first runs only when all
    /// the ones before it were falsy.
    Or,
}

/// The short-circuit operator at the root of an expression, if any.
///
/// Cheaper than calling the operand collectors just to learn the shape:
/// this answers the question without allocating, so the descent below
/// can walk a whole expression tree and only pay for the nodes that
/// actually are chains.
fn short_circuit_kind(expr: &Expression<'_>) -> Option<ChainKind> {
    match expr {
        Expression::Binary(bin) => match bin.operator {
            BinaryOperator::And(_) | BinaryOperator::LowAnd(_) => Some(ChainKind::And),
            BinaryOperator::Or(_) | BinaryOperator::LowOr(_) => Some(ChainKind::Or),
            _ => None,
        },
        Expression::Parenthesized(inner) => short_circuit_kind(inner.expression),
        _ => None,
    }
}

/// The expression an argument carries, whether it was passed
/// positionally or by name.
fn argument_value<'b>(arg: &'b Argument<'b>) -> &'b Expression<'b> {
    match arg {
        Argument::Positional(a) => a.value,
        Argument::Named(a) => a.value,
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
        Expression::Instantiation(inst) => {
            if let Some(args) = &inst.argument_list {
                for arg in args.arguments.iter() {
                    let arg_expr = match arg {
                        Argument::Positional(a) => a.value,
                        Argument::Named(a) => a.value,
                    };
                    record_scope_snapshot(arg_expr.span().start.offset, scope);
                    record_scope_snapshot_recursive(arg_expr, scope);
                }
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

/// Whether a narrowing pass changed a variable's resolved type.
///
/// Both halves of a `ResolvedType` matter. The type string carries most
/// narrowings, but a member-existence guard (`property_exists($x, 'p')`)
/// leaves it untouched and only swaps in a `ClassInfo` carrying the proven
/// member, so the class side is compared by `Arc` identity too — every
/// narrowing that rebuilds a `ClassInfo` yields a fresh allocation.
///
/// This is deliberately stricter than
/// [`resolved_types_differ`](super::resolved_types_differ), which compares
/// classes by FQN: that one drives loop fix-point iteration, where treating
/// a re-allocated but equal `ClassInfo` as a change would stop it converging.
pub(crate) fn narrowing_changed_types(before: &[ResolvedType], after: &[ResolvedType]) -> bool {
    before.len() != after.len()
        || before.iter().zip(after).any(|(a, b)| {
            a.type_string != b.type_string
                || match (&a.class_info, &b.class_info) {
                    (Some(x), Some(y)) => !Arc::ptr_eq(x, y),
                    (None, None) => false,
                    _ => true,
                }
        })
}
