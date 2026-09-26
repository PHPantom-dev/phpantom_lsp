use super::*;

use mago_syntax::cst::Literal;

/// What a boolean assignment's value proves, so that testing the
/// variable later narrows the way testing the expression would have.
///
/// ```php
/// $show = $limit !== null && $count > $limit;
/// if ($show) { $limit; } // int
/// ```
///
/// The expression is narrowed once for each outcome, over a scope that
/// holds only the subjects it names, and every key that narrowed becomes a
/// proof held by the variable: [`ProofTrigger::Within`] `true` for what
/// the truthy outcome proved, `false` for the falsy one.  Writing a subject
/// afterwards drops the proofs about it, the same as for every other
/// proof.
///
/// Only a value typed `bool` qualifies, which keeps this off the ordinary
/// assignments that dominate a function body.  A ternary whose other arm
/// is a falsy literal (`$c instanceof S ? $c->ok() : false`) is truthy
/// only when its condition held and the arm it took was truthy, so that
/// outcome narrows by both.  Its falsy outcome is a disjunction of the two
/// ways the value can be falsy, so it is not recorded.
pub(crate) fn condition_implications<'b>(
    lhs_name: &str,
    rhs: &'b Expression<'b>,
    rhs_types: &[ResolvedType],
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ImpliedNarrowing> {
    let is_boolean = !rhs_types.is_empty()
        && rhs_types.iter().all(|rt| {
            let ty = &rt.type_string;
            !matches!(ty.kind(), TypeKind::Nullable(_))
                && (ty.is_bool() || ty.is_true() || ty.is_false())
        });
    if !is_boolean || !is_condition_shaped(rhs) {
        return Vec::new();
    }

    let lhs = atom(lhs_name);
    let mut proofs: Vec<ImpliedNarrowing> = Vec::new();
    for truthy in [true, false] {
        let Some(parts) = truthiness_parts(rhs, truthy) else {
            continue;
        };
        let mut subjects: Vec<String> = Vec::new();
        for (part, _) in &parts {
            collect_condition_subject_vars(part, &mut subjects);
            for key in collect_condition_property_keys(part) {
                if !subjects.contains(&key) {
                    subjects.push(key);
                }
            }
        }
        let mut local = ScopeState::new();
        for subject in &subjects {
            if subject == lhs_name {
                continue;
            }
            let types = scope.get(subject);
            if !types.is_empty() {
                local.set(subject, types.to_vec());
            }
        }
        if local.locals.is_empty() {
            continue;
        }
        let seeded = local.locals.clone();
        for (part, holds) in &parts {
            if *holds {
                apply_condition_narrowing(part, &mut local, ctx);
            } else {
                apply_condition_narrowing_inverse(part, &mut local, ctx);
            }
        }
        if local.unreachable {
            continue;
        }
        let trigger = ProofTrigger::Within(vec![ResolvedType::from_type_string(if truthy {
            PhpType::true_()
        } else {
            PhpType::false_()
        })]);
        for (key, types) in local.locals {
            let changed = seeded.get(&key).is_some_and(|before| {
                narrowing_changed_types(before, &types) && rules_out_something(before, &types)
            });
            if !changed || types.is_empty() || key == lhs {
                continue;
            }
            proofs.push(ImpliedNarrowing {
                trigger: trigger.clone(),
                key,
                types,
            });
        }
    }
    proofs
}

/// Whether narrowing `before` to `after` ruled out an alternative.
///
/// Joining the legs of an `||` hands back every leg's answer, and a leg
/// that proved nothing about a subject hands back its type unchanged, so
/// `$raw instanceof Html || $other instanceof Html` leaves `$raw` as
/// `Html|Renderable`: a different list, but no narrower than `Renderable`.
fn rules_out_something(before: &[ResolvedType], after: &[ResolvedType]) -> bool {
    !before.iter().all(|b| {
        after
            .iter()
            .any(|a| a.type_string == b.type_string || b.type_string.is_subtype_of(&a.type_string))
    })
}

/// Whether `expr` is the kind of expression a condition is written as,
/// rather than a value that merely happens to be a boolean.
fn is_condition_shaped(expr: &Expression<'_>) -> bool {
    match unwrap_parens(expr) {
        Expression::Binary(bin) => matches!(
            bin.operator,
            BinaryOperator::And(_)
                | BinaryOperator::LowAnd(_)
                | BinaryOperator::Or(_)
                | BinaryOperator::LowOr(_)
                | BinaryOperator::Identical(_)
                | BinaryOperator::NotIdentical(_)
                | BinaryOperator::Equal(_)
                | BinaryOperator::NotEqual(_)
                | BinaryOperator::LessThan(_)
                | BinaryOperator::LessThanOrEqual(_)
                | BinaryOperator::GreaterThan(_)
                | BinaryOperator::GreaterThanOrEqual(_)
                | BinaryOperator::Instanceof(_)
        ),
        Expression::UnaryPrefix(prefix) => prefix.operator.is_not(),
        Expression::Construct(Construct::Isset(_) | Construct::Empty(_)) => true,
        Expression::Call(Call::Function(_) | Call::Method(_) | Call::StaticMethod(_)) => true,
        Expression::Conditional(conditional) => {
            conditional.then.is_some()
                && (is_falsy_literal(conditional.r#else)
                    || conditional.then.is_some_and(is_falsy_literal))
        }
        _ => false,
    }
}

/// The conditions whose outcomes make `expr` evaluate to a value of the
/// given truthiness, each paired with the outcome it must have had, or
/// `None` when that is not a single conjunction.
fn truthiness_parts<'b>(
    expr: &'b Expression<'b>,
    truthy: bool,
) -> Option<Vec<(&'b Expression<'b>, bool)>> {
    let Expression::Conditional(conditional) = unwrap_parens(expr) else {
        return Some(vec![(expr, truthy)]);
    };
    if !truthy {
        return None;
    }
    let then = conditional.then?;
    if is_falsy_literal(conditional.r#else) {
        Some(vec![(conditional.condition, true), (then, true)])
    } else if is_falsy_literal(then) {
        Some(vec![
            (conditional.condition, false),
            (conditional.r#else, true),
        ])
    } else {
        None
    }
}

/// Whether `expr` is a literal that is always falsy.
fn is_falsy_literal(expr: &Expression<'_>) -> bool {
    matches!(
        unwrap_parens(expr),
        Expression::Literal(Literal::False(_) | Literal::Null(_))
    )
}
