use super::*;

/// The chain inside a `!` that negates a whole `&&` / `||` expression.
///
/// A negation over a chain says the opposite of everything the chain says,
/// so the pass that reads it is the *other* polarity's: the truthy branch
/// of `if (!(A || B))` is the inverse of `A || B`, and the fall-through of
/// `if (!(A || B)) { return; }` is its truthy narrowing.  Handing the
/// chain over is what lets each operand be examined at all — the negated
/// disjunction as a whole matches no extractor, so without this the
/// widest idiom for "one of these two types" narrows nothing.
///
/// Only chains delegate.  A `!` over a single check is a shape the
/// extractors recognise in place, and routing it through the opposite
/// pass would change which commit path it takes.
pub(crate) fn negated_logical_chain<'b>(expr: &'b Expression<'b>) -> Option<&'b Expression<'b>> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    let is_chain = matches!(
        inner,
        Expression::Binary(bin)
            if matches!(
                bin.operator,
                BinaryOperator::And(_)
                    | BinaryOperator::LowAnd(_)
                    | BinaryOperator::Or(_)
                    | BinaryOperator::LowOr(_)
            )
    );
    (negated && is_chain).then_some(inner)
}

/// Narrow `scope` to what holds once any one of `legs` was truthy: each leg
/// narrows its own copy of the scope, and the copies are joined.
///
/// This is what the body of a `match (true)` arm with the conditions `a, b`
/// sees.  Unlike the legs of `a || b`, each condition is tested against the
/// arm's own value with `===`, so a condition that did not match proves
/// nothing the next one can build on, and every leg starts from `scope`.
pub(super) fn apply_any_leg_narrowing<'b>(
    legs: &[&'b Expression<'b>],
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let leg_scopes: Vec<ScopeState> = legs
        .iter()
        .map(|leg| {
            let mut leg_scope = scope.clone();
            apply_condition_narrowing(leg, &mut leg_scope, ctx);
            drop_leg_answers_the_base_rules_out(&mut leg_scope, scope);
            leg_scope
        })
        .collect();
    let Some((first, rest)) = leg_scopes.split_first() else {
        return;
    };

    let mut joined = first.clone();
    for leg_scope in rest {
        joined.merge_branch(leg_scope);
    }
    // A path that could not read a member path at all did not narrow
    // it, and adopting the one leg that could would claim its answer
    // for the whole disjunction.
    let refs: Vec<&ScopeState> = leg_scopes.iter().collect();
    retain_synthetic_keys_common_to_all(&mut joined, &refs);
    *scope = joined;
}

/// Undo what a leg concluded about a subject the branch already knew
/// something else about.
///
/// A leg starts from what everything evaluated before it proved, and can
/// only refine that. When a leg lands on
/// a type the base rules out, the leg describes a run that cannot happen:
/// the `is_null($price)` half of `is_null($price) || $price->isZero()` on
/// a `$price` a guard already proved non-null. Joining the `null` it
/// wrote back in would hand the branch body the very type the guard
/// removed, so the base's answer stands for that subject instead.
///
/// Only the entries the leg rewrote can disagree with the base, so only
/// those are compared.
pub(crate) fn drop_leg_answers_the_base_rules_out(leg: &mut ScopeState, base: &ScopeState) {
    let mut ruled_out: Vec<(Atom, Vec<ResolvedType>)> = Vec::new();
    let _ = leg
        .locals
        .diff::<()>(&base.locals, |name, leg_types, base_types| {
            if let (Some(leg_types), Some(base_types)) = (leg_types, base_types)
                && types_are_disjoint(leg_types, base_types)
            {
                ruled_out.push((*name, base_types.clone()));
            }
            std::ops::ControlFlow::Continue(())
        });
    for (name, base_types) in ruled_out {
        leg.locals.insert(name, base_types);
    }
}

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

fn collect_and_chain_operands_inner<'b>(
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

/// Collect operands of a `||` chain into a left-to-right list, the way
/// [`collect_and_chain_operands`] does for `&&`.
pub(crate) fn collect_or_chain_operands<'b>(expr: &'b Expression<'b>) -> Vec<&'b Expression<'b>> {
    let mut operands = Vec::new();
    collect_or_chain_operands_inner(expr, &mut operands);
    operands
}

fn collect_or_chain_operands_inner<'b>(
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
