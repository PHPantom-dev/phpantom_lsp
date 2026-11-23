//! Narrowing by the comparison a `switch` case or a `match` arm makes
//! between its subject and a label.
//!
//! `case Foo::class:` under `switch (get_class($x))` runs where
//! `get_class($x) == Foo::class` held, and a `match` arm where `===` did.
//! Building that comparison and handing it to the condition pipeline gives
//! the labels every rule an `if` knows, instead of a subset of their own.

use super::*;

/// The comparison a case label makes against its subject: `==` for a
/// `switch`, `===` for a `match`.
pub(crate) fn subject_comparison<'a>(
    subject: &'a Expression<'a>,
    label: &'a Expression<'a>,
    strict: bool,
) -> Expression<'a> {
    let span = label.span();
    Expression::Binary(Binary {
        lhs: subject,
        operator: if strict {
            BinaryOperator::Identical(span)
        } else {
            BinaryOperator::Equal(span)
        },
        rhs: label,
    })
}

/// Narrow `scope` to where `subject` compared equal to one of `labels`.
///
/// The arm runs when any one label matched, so each label narrows its own
/// copy of the scope and the copies are joined.
pub(crate) fn apply_subject_match_narrowing(
    subject: &Expression<'_>,
    labels: &[&Expression<'_>],
    strict: bool,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let conditions: Vec<Expression<'_>> = labels
        .iter()
        .map(|label| subject_comparison(subject, label, strict))
        .collect();
    match conditions.as_slice() {
        [] => {}
        [condition] => apply_condition_narrowing(condition, scope, ctx),
        _ => {
            let legs: Vec<&Expression<'_>> = conditions.iter().collect();
            for leg in &legs {
                seed_property_keys_into_scope(leg, scope, ctx);
            }
            apply_any_leg_narrowing(&legs, scope, ctx);
        }
    }
}

/// Narrow `scope` to where `subject` compared equal to none of `labels`.
pub(crate) fn apply_subject_mismatch_narrowing(
    subject: &Expression<'_>,
    labels: &[&Expression<'_>],
    strict: bool,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    for label in labels {
        let comparison = subject_comparison(subject, label, strict);
        apply_condition_narrowing_inverse(&comparison, scope, ctx);
        if scope.unreachable {
            return;
        }
    }
}
