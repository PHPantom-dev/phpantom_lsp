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
pub(super) fn negated_logical_chain<'b>(expr: &'b Expression<'b>) -> Option<&'b Expression<'b>> {
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

/// Narrow through an `&&` operand that is itself an `||` chain.
///
/// Entering the branch proves the disjunction as a whole, not any one leg,
/// so the scope inside it is the join of the scopes the legs would each
/// produce — exactly the shape [`ScopeState::merge_branch`] already
/// handles for an `if`/`else`.  Going through the join is what records
/// which leg proved what, so a check further down that rules the other
/// legs out recovers the surviving leg's conclusions:
///
/// ```php
/// if ($n->keyVar === null || ($n->keyVar instanceof Variable && is_string($n->keyVar->name))) {
///     $name = $n->keyVar instanceof Variable ? $n->keyVar->name : null;   // string|null
/// }
/// ```
///
/// The join is the *only* treatment a disjunction gets: the passes that
/// run before it are handed the chain's plain conjuncts alone, so what
/// they leave behind is the base every leg starts from.  That ordering is
/// what keeps a leg's own conclusion out of the branch body — reading
/// `$v instanceof Variable || $flag` as though the `instanceof` had held
/// dropped the `null` the guard never ruled out.
///
/// `pinned` names the subjects a conjunct already narrowed to a definite
/// class.  Those the join leaves alone: `$b instanceof Generic && ($cls
/// === Generic::class || $b instanceof Template)` proves `$b` is a
/// `Generic`, and a leg naming an unrelated class replaces rather than
/// intersects, so joining the legs would answer `Generic|Template` and
/// lose what the conjunct established.
pub(super) fn apply_disjunct_operand_narrowing<'b>(
    disjunctions: &[&'b Expression<'b>],
    pinned: &[String],
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    for operand in disjunctions {
        let legs = collect_or_chain_operands(unwrap_parens(operand));
        if legs.len() < 2 {
            continue;
        }

        let leg_scopes: Vec<ScopeState> = legs
            .into_iter()
            .map(|leg| {
                let mut leg_scope = scope.clone();
                apply_condition_narrowing(leg, &mut leg_scope, ctx);
                drop_leg_answers_the_base_rules_out(&mut leg_scope, scope);
                leg_scope
            })
            .collect();

        let mut joined = leg_scopes[0].clone();
        for leg_scope in &leg_scopes[1..] {
            joined.merge_branch(leg_scope);
        }
        // A path that could not read a member path at all did not narrow
        // it, and adopting the one leg that could would claim its answer
        // for the whole disjunction.
        let refs: Vec<&ScopeState> = leg_scopes.iter().collect();
        retain_synthetic_keys_common_to_all(&mut joined, &refs);
        for subject in pinned {
            let key = atom(subject);
            if let Some(types) = scope.locals.get(&key) {
                joined.locals.insert(key, types.clone());
            }
        }
        *scope = joined;
    }
}

/// Undo what a leg concluded about a subject the branch already knew
/// something else about.
///
/// The legs start from what the chain's conjuncts — and everything above
/// the `if` — proved, and a leg can only refine that. When a leg lands on
/// a type the base rules out, the leg describes a run that cannot happen:
/// the `is_null($price)` half of `is_null($price) || $price->isZero()` on
/// a `$price` a guard already proved non-null. Joining the `null` it
/// wrote back in would hand the branch body the very type the guard
/// removed, so the base's answer stands for that subject instead.
fn drop_leg_answers_the_base_rules_out(leg: &mut ScopeState, base: &ScopeState) {
    for (name, base_types) in &base.locals {
        let Some(leg_types) = leg.locals.get(name) else {
            continue;
        };
        if types_are_disjoint(leg_types, base_types) {
            leg.locals.insert(*name, base_types.clone());
        }
    }
}
