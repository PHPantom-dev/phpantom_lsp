use super::*;

/// Narrow the `$matches` out-parameter of a `preg_match`/`preg_match_all`
/// call that a condition tests the outcome of.
///
/// The seeding at the call site writes what the call leaves either way
/// (`array{0: string, 1: string}|array{}`), because that is all it can know
/// there. A branch that runs only on a successful match knows the array has
/// the pattern's keys, and one that runs only on a failed match knows it is
/// empty. That is what keeps a group read inside the guard a plain `string`,
/// while the same read on a line either outcome reaches carries the `null` a
/// missing key yields.
pub(crate) fn apply_preg_match_narrowing(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    // `preg_match(…, $m) && $m[1] === 'x'` proves the match for the whole
    // conjunction; an `||` operand proves nothing, and is left whole by the
    // decomposition below.
    for operand in collect_and_chain_operands(condition) {
        // The condition either makes the call itself, or names a variable
        // holding a result the walk recorded at the assignment.  Both say
        // the same thing about the out-parameter.
        let Some((matches_var, matched, matches_all, matched_when_true)) =
            crate::type_engine::regex_shape::preg_condition(operand)
                .and_then(|(call, matched_when_true)| {
                    let matched = preg_matched_type(&call, scope, ctx)?;
                    Some((
                        atom(call.matches_var),
                        matched,
                        call.matches_all,
                        matched_when_true,
                    ))
                })
                .or_else(|| {
                    let (outcome, matched_when_true) = preg_outcome_alias(operand, scope)?;
                    Some((
                        outcome.matches_var,
                        outcome.matched,
                        outcome.matches_all,
                        matched_when_true,
                    ))
                })
        else {
            continue;
        };
        let narrowed = if matched_when_true == truthy {
            matched
        } else {
            crate::type_engine::regex_shape::no_match_type(&matched, matches_all).unwrap_or(matched)
        };
        scope.set(&matches_var, vec![ResolvedType::from_type_string(narrowed)]);
    }
}

/// Record the `preg_match` outcome a variable's value is.
///
/// `$ok = preg_match('/(\d+)/', $s, $m);` proves nothing on its own, but
/// `$ok` now stands for the match: wherever it is tested, `$m` narrows the
/// same way testing the call would narrow it.  The comparison spellings
/// (`$ok = preg_match(…) === 1`) come along for free, since the condition
/// reader handles them and this reuses it.
pub(crate) fn record_preg_outcome<'b>(
    lhs_name: &str,
    rhs: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let Some((call, matched_when_true)) = crate::type_engine::regex_shape::preg_condition(rhs)
    else {
        return;
    };
    // `$m = preg_match(…, $m)` overwrites the very array the outcome would
    // describe, so there is nothing left for it to narrow.
    if call.matches_var == lhs_name {
        return;
    }
    let Some(matched) = preg_matched_type(&call, scope, ctx) else {
        return;
    };
    // `$failed = !preg_match(…, $m)` holds the negation, which the reader
    // folds into `matched_when_true`.  Storing the positive shape and
    // flipping the sense here keeps the lookup side free of polarity.
    let matched = if matched_when_true {
        matched
    } else {
        match crate::type_engine::regex_shape::no_match_type(&matched, call.matches_all) {
            Some(no_match) => no_match,
            None => return,
        }
    };
    scope.preg_outcomes.insert(
        atom(lhs_name),
        PregOutcome {
            matches_var: atom(call.matches_var),
            matched,
            matches_all: call.matches_all,
        },
    );
}

/// Expand a bare boolean operand into the `preg_match` outcome it stands
/// for.
///
/// `$ok` and `!$ok` both resolve through the recorded outcome, with the
/// operand's own negation folded into the result, so the caller treats
/// them exactly like the call the outcome came from.
fn preg_outcome_alias(expr: &Expression<'_>, scope: &ScopeState) -> Option<(PregOutcome, bool)> {
    if scope.preg_outcomes.is_empty() {
        return None;
    }

    let mut negated = false;
    let mut inner = expr;
    loop {
        match inner {
            Expression::Parenthesized(p) => inner = p.expression,
            Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
                negated = !negated;
                inner = prefix.operand;
            }
            _ => break,
        }
    }

    let Expression::Variable(Variable::Direct(dv)) = inner else {
        return None;
    };
    let outcome = scope.preg_outcomes.get(&atom(bytes_to_str(dv.name)))?;
    Some((outcome.clone(), !negated))
}
