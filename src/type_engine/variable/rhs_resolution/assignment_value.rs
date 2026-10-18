//! The value an assignment expression evaluates to: what plain `=`, `??=`,
//! and each compound operator leave in their target.

use mago_syntax::cst::Assignment;
use mago_syntax::cst::assignment::AssignmentOperator;

use crate::php_type::PhpType;
use crate::type_engine::resolver::VarResolutionCtx;
use crate::types::ResolvedType;

use super::arithmetic::{
    ArithmeticOpKind, infer_addition_result_type, infer_arithmetic_result_type,
    infer_modulo_result_type,
};
use super::resolve_rhs_expression;

/// Resolve an assignment expression used as a value, not just a
/// statement: `$x = ($y = 1)`, `$x = $a ?? ($y = 1) ?? 1`.
///
/// Recording the target itself, so a later read of `$y` sees this
/// write, is the forward walker's job (`process_expr` /
/// `process_assignment_expr`), not this read-only pipeline's.
pub(super) fn resolve_assignment_as_value<'b>(
    assignment: &'b Assignment<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    assignment_value(
        &assignment.operator,
        || resolve_rhs_expression(assignment.lhs, ctx),
        || resolve_rhs_expression(assignment.rhs, ctx),
    )
}

/// The value an assignment with `operator` evaluates to, given the types
/// its target held and the types its right-hand side produces.
///
/// PHP assignment is itself an expression whose value is whatever ends
/// up in the target. Plain `=` yields the RHS's own value; the other
/// operators combine it with the target's current value the same way a
/// statement-level compound assignment does, so an operand position
/// (`??` chain, ternary, match arm) gets the same answer a top-level
/// `$y = expr;` statement would.
///
/// The operand closures are only called for the operators that need them:
/// plain `=` never reads its target, and `.=` never resolves its
/// right-hand side.
pub(crate) fn assignment_value(
    operator: &AssignmentOperator,
    lhs_types: impl FnOnce() -> Vec<ResolvedType>,
    rhs_types: impl FnOnce() -> Vec<ResolvedType>,
) -> Vec<ResolvedType> {
    match operator {
        AssignmentOperator::Assign(_) => rhs_types(),
        // Whichever side survives: the target's non-null half, or the
        // fallback that replaced it.
        AssignmentOperator::Coalesce(_) => {
            let combined = coalesce_assign_value(lhs_types(), rhs_types());
            if combined.is_empty() {
                vec![ResolvedType::from_type_string(PhpType::mixed())]
            } else {
                combined
            }
        }
        _ => compound_assignment_result(operator, lhs_types, rhs_types)
            .map(|ty| vec![ResolvedType::from_type_string(ty)])
            .unwrap_or_default(),
    }
}

/// What a `??=` leaves behind, given what its target and its fallback
/// resolve to.
///
/// `??=` keeps the target when it is not null and assigns the fallback
/// otherwise, so the value is the target's non-null half unioned with the
/// fallback. The resolved types are combined as they are, rather than
/// joined into one union *type string*, so the `class_info` already
/// attached to each operand survives: a rebuilt string carries none, and
/// a member access on the result would have nothing to resolve against.
///
/// Where both sides name the same type (commonly the target's declared
/// element type, which resolved no class, alongside an argument that did)
/// the class-backed entry speaks for the pair.
///
/// Shared by the `??=` used as a value here and the forward walker's
/// statement-level `??=`.
pub(crate) fn coalesce_assign_value(
    lhs_types: Vec<ResolvedType>,
    rhs_types: Vec<ResolvedType>,
) -> Vec<ResolvedType> {
    let mut combined: Vec<ResolvedType> = lhs_types
        .into_iter()
        .filter(|rt| !rt.type_string.is_null())
        .map(|mut rt| {
            if let Some(non_null) = rt.type_string.non_null_type() {
                rt.type_string = non_null;
            }
            rt
        })
        .collect();
    ResolvedType::extend_unique(&mut combined, rhs_types);
    let class_backed: Vec<PhpType> = combined
        .iter()
        .filter(|rt| rt.class_info.is_some())
        .map(|rt| rt.type_string.clone())
        .collect();
    combined.retain(|rt| rt.class_info.is_some() || !class_backed.contains(&rt.type_string));
    combined
}

/// The type a compound assignment (`.=`, `+=`, `<<=`, …) leaves in its
/// target, given the types the target held and the types the right-hand
/// side produces.
///
/// The operand closures are only called for the operators that need them,
/// so a `.=` never pays for resolving its right-hand side.
///
/// `??=` and plain `=` return `None`: neither is decided by the operator
/// alone, so each caller settles them for itself.
pub(crate) fn compound_assignment_result(
    operator: &AssignmentOperator,
    lhs_types: impl FnOnce() -> Vec<ResolvedType>,
    rhs_types: impl FnOnce() -> Vec<ResolvedType>,
) -> Option<PhpType> {
    match operator {
        AssignmentOperator::Concat(_) => Some(PhpType::string()),
        AssignmentOperator::Modulo(_) => Some(infer_modulo_result_type(&lhs_types(), &rhs_types())),
        AssignmentOperator::LeftShift(_)
        | AssignmentOperator::RightShift(_)
        | AssignmentOperator::BitwiseAnd(_)
        | AssignmentOperator::BitwiseOr(_)
        | AssignmentOperator::BitwiseXor(_) => Some(PhpType::int()),
        AssignmentOperator::Addition(_) => {
            Some(infer_addition_result_type(&lhs_types(), &rhs_types()))
        }
        AssignmentOperator::Subtraction(_)
        | AssignmentOperator::Multiplication(_)
        | AssignmentOperator::Division(_)
        | AssignmentOperator::Exponentiation(_) => {
            let op_kind = match operator {
                AssignmentOperator::Subtraction(_) => ArithmeticOpKind::Subtraction,
                AssignmentOperator::Multiplication(_) => ArithmeticOpKind::Multiplication,
                AssignmentOperator::Division(_) => ArithmeticOpKind::Division,
                AssignmentOperator::Exponentiation(_) => ArithmeticOpKind::Exponentiation,
                _ => unreachable!("outer match already narrowed to arithmetic operators"),
            };
            Some(infer_arithmetic_result_type(
                &lhs_types(),
                &rhs_types(),
                op_kind,
            ))
        }
        AssignmentOperator::Coalesce(_) | AssignmentOperator::Assign(_) => None,
    }
}
