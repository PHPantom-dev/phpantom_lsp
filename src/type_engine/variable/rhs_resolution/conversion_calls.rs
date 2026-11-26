//! Builtins that do what an operator does: `intval()` and friends convert
//! like a cast, `pow()` is `**`, and `abs()` is a sign flip on the negative
//! values.  Each reads its arguments through the same typing as the
//! operator, so `intval('1')` and `(int) '1'` cannot disagree.

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::php_type::PhpType;
use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::type_ops::int_interval::IntInterval;
use crate::type_engine::variable::string_func_rules::calls_global_function;
use crate::types::ResolvedType;

use super::arithmetic::{ArithmeticOpKind, infer_arithmetic_result_type};
use super::resolve_rhs_expression;
use super::scalar_fold::Scalar;
use super::unary_prefix::{CastKind, cast_expression_type};

/// What `name(...)` returns when it is one of the operator-like builtins,
/// or `None` for any other call, or one whose arguments are spread, named,
/// or not the ones the rule reads.
pub(super) fn conversion_call(
    name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let bare = name.trim_start_matches('\\');
    let kind = match bare.to_ascii_lowercase().as_str() {
        "strval" => Rule::Cast(CastKind::String),
        "intval" => Rule::Cast(CastKind::Int),
        "floatval" | "doubleval" => Rule::Cast(CastKind::Float),
        "boolval" => Rule::Cast(CastKind::Bool),
        "pow" => Rule::Pow,
        "abs" => Rule::Abs,
        _ => return None,
    };
    let args = positional_args(argument_list)?;
    if !calls_global_function(
        name,
        ctx.function_loader(),
        argument_list.span().start.offset,
    ) {
        return None;
    }
    let ty = match (kind, args.as_slice()) {
        (Rule::Cast(kind), [operand]) => cast_expression_type(
            kind,
            operand,
            || resolve_rhs_expression(operand, ctx),
            ctx.file.class_loader,
            ctx.resolved_class_cache,
        ),
        (Rule::Pow, [base, exponent]) => infer_arithmetic_result_type(
            &resolve_rhs_expression(base, ctx),
            &resolve_rhs_expression(exponent, ctx),
            ArithmeticOpKind::Exponentiation,
        ),
        (Rule::Abs, [operand]) => abs_type(&resolve_rhs_expression(operand, ctx))?,
        _ => return None,
    };
    Some(vec![ResolvedType::from_type_string(ty)])
}

#[derive(Clone, Copy)]
enum Rule {
    Cast(CastKind),
    Pow,
    Abs,
}

/// The values of a call's arguments, when every one is a plain positional
/// argument.
fn positional_args<'b>(argument_list: &'b ArgumentList<'b>) -> Option<Vec<&'b Expression<'b>>> {
    argument_list
        .arguments
        .iter()
        .map(|arg| match arg {
            Argument::Positional(arg) if arg.ellipsis.is_none() => Some(arg.value),
            _ => None,
        })
        .collect()
}

/// `abs()` of each alternative of its operand: an integer's interval
/// folded onto the non-negative side, a known number's absolute value,
/// and a `float` kept as one.  An operand that is not a number throughout
/// gives the function's own `float|int<0, max>`.
fn abs_type(operand: &[ResolvedType]) -> Option<PhpType> {
    if operand.is_empty() {
        return None;
    }
    let declared = || PhpType::union(vec![PhpType::float(), PhpType::int_range("0", "max")]);
    let mut ints: Vec<IntInterval> = Vec::new();
    let mut floats: Vec<PhpType> = Vec::new();
    let push_float = |ty: PhpType, floats: &mut Vec<PhpType>| {
        if !floats.contains(&ty) {
            floats.push(ty);
        }
    };
    for member in operand
        .iter()
        .flat_map(|resolved| resolved.type_string.union_members())
    {
        if member.is_named_ci("non-zero-int") {
            ints.push(IntInterval {
                min: Some(1),
                max: None,
            });
        } else if let Some(interval) = IntInterval::of_int(member) {
            ints.push(interval.abs());
        } else if member.is_null() {
            ints.push(IntInterval {
                min: Some(0),
                max: Some(0),
            });
        } else if let Some(number) = Scalar::from_type(member).and_then(|s| s.to_number()) {
            match number {
                Scalar::Int(value) => {
                    // `abs(PHP_INT_MIN)` overflows to a float.
                    let Some(value) = value.checked_abs() else {
                        return Some(declared());
                    };
                    ints.push(IntInterval {
                        min: Some(value),
                        max: Some(value),
                    });
                }
                Scalar::Float(value) => {
                    push_float(Scalar::Float(value.abs()).into_type(), &mut floats)
                }
                _ => return Some(declared()),
            }
        } else if member.is_float_subtype() {
            push_float(PhpType::float(), &mut floats);
        } else {
            return Some(declared());
        }
    }
    let mut members: Vec<PhpType> = IntInterval::merged(ints)
        .into_iter()
        .map(IntInterval::into_type)
        .collect();
    members.extend(floats);
    Some(match members.len() {
        1 => members.remove(0),
        _ => PhpType::union(members),
    })
}
