use super::*;

use mago_syntax::cst::Literal;

/// Narrow by a check compared against `true` or `false`.
///
/// `in_array($x, $list, true) === true` proves what the bare call proves,
/// and `is_string($x) === false` proves what `!is_string($x)` does, but
/// every extractor matches the check itself, so the comparison wrapped
/// around it hid it from all of them.  This hands the operand to the
/// pass for whichever polarity the comparison implies.
///
/// `holds` is whether the condition is known to hold (the truthy branch)
/// or known to have failed (the inverse).
///
/// A loose comparison against a boolean casts the operand to `bool`, so it
/// decides the operand's truthiness in both directions.  A strict one does
/// too when it holds, but failing `=== true` only proves the operand is
/// falsy when `true` is the one truthy value it can have, so that
/// direction needs the operand to be a boolean.
pub(super) fn apply_bool_comparison_narrowing<'b>(
    condition: &'b Expression<'b>,
    holds: bool,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let Expression::Binary(bin) = unwrap_parens(condition) else {
        return;
    };
    let (equals, strict) = match bin.operator {
        BinaryOperator::Identical(_) => (true, true),
        BinaryOperator::Equal(_) => (true, false),
        BinaryOperator::NotIdentical(_) => (false, true),
        BinaryOperator::NotEqual(_) => (false, false),
        _ => return,
    };
    let (operand, literal) = match (bool_literal(bin.lhs), bool_literal(bin.rhs)) {
        (None, Some(literal)) => (bin.lhs, literal),
        (Some(literal), None) => (bin.rhs, literal),
        _ => return,
    };

    // Whether the operand compared equal to the literal.
    let matched = equals == holds;
    let truthy = if matched || !strict {
        literal == matched
    } else if operand_is_boolean(operand, literal, scope, ctx) {
        !literal
    } else {
        return;
    };

    if truthy {
        apply_condition_narrowing(operand, scope, ctx);
    } else {
        apply_condition_narrowing_inverse(operand, scope, ctx);
    }
}

/// The value of a `true` / `false` literal.
fn bool_literal(expr: &Expression<'_>) -> Option<bool> {
    match unwrap_parens(expr) {
        Expression::Literal(Literal::True(_)) => Some(true),
        Expression::Literal(Literal::False(_)) => Some(false),
        _ => None,
    }
}

/// Whether every value `expr` can hold is a boolean, so that it failing to
/// be one boolean makes it the other.
///
/// `null` counts when `allow_null` is set: failing `=== true` leaves a
/// `?bool` falsy either way, while failing `=== false` leaves it `true` or
/// `null`, which is no proof of truthiness.
fn operand_is_boolean(
    expr: &Expression<'_>,
    allow_null: bool,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> bool {
    let resolved = resolve_rhs_with_scope(expr, scope, ctx);
    if resolved.is_empty() {
        return false;
    }
    let ty = ResolvedType::types_joined(&resolved);
    ty.union_members().iter().all(|member| match member.kind() {
        TypeKind::Nullable(_) => allow_null && member.is_bool(),
        _ => {
            member.is_bool()
                || member.is_true()
                || member.is_false()
                || (allow_null && member.is_null())
        }
    })
}
