//! The result of `**` (and `pow()`) beyond two known numbers.
//!
//! Each pair of alternatives of the base and the exponent is raised on its
//! own, the way PHPStan's `ExponentiateHelper` does: an exponent that is
//! loosely `0` makes the result `1`, a loose `1` leaves a numeric base as it
//! was, a `null` base is `0` or `1`, and integer ranges raise to the range
//! of every power they can make.

use crate::php_type::PhpType;
use crate::type_engine::type_ops::int_interval::IntInterval;
use crate::types::ResolvedType;

use super::scalar_fold::Scalar;

/// How many pairs of base and exponent alternatives are raised one by one
/// before the result is left to the operator's base type.
const PAIR_LIMIT: usize = 128;

/// The result of `lhs ** rhs` worked out pair by pair, or `None` when some
/// pair says no more than the operator's base type (`int ** int` may be a
/// float), so the caller's fallback answers instead.
pub(super) fn exponentiation_result(
    lhs_types: &[ResolvedType],
    rhs_types: &[ResolvedType],
) -> Option<PhpType> {
    let bases = members(lhs_types)?;
    let exponents = members(rhs_types)?;
    if bases.len() * exponents.len() > PAIR_LIMIT {
        return None;
    }
    let mut ints: Vec<IntInterval> = Vec::new();
    let mut others: Vec<PhpType> = Vec::new();
    for base in &bases {
        for exponent in &exponents {
            match raise(base, exponent)? {
                Raised::Int(interval) => ints.push(interval),
                Raised::Other(ty) => {
                    if !others.contains(&ty) {
                        others.push(ty);
                    }
                }
            }
        }
    }
    let mut results: Vec<PhpType> = IntInterval::merged(ints)
        .into_iter()
        .map(IntInterval::into_type)
        .collect();
    results.extend(others);
    Some(PhpType::union(results))
}

/// Every alternative of an operand, or `None` for an operand nothing typed.
fn members(types: &[ResolvedType]) -> Option<Vec<PhpType>> {
    let members: Vec<PhpType> = types
        .iter()
        .flat_map(|rt| rt.type_string.union_members())
        .cloned()
        .collect();
    (!members.is_empty()).then_some(members)
}

/// One pair's result: integers are kept as intervals so the pairs merge
/// into the fewest ranges.
enum Raised {
    Int(IntInterval),
    Other(PhpType),
}

/// `base ** exponent` for one alternative of each, or `None` when the pair
/// gives no more than `int|float`.
fn raise(base: &PhpType, exponent: &PhpType) -> Option<Raised> {
    if base.is_null() {
        return Some(Raised::Other(PhpType::union(vec![
            PhpType::literal_int("0"),
            PhpType::literal_int("1"),
        ])));
    }
    let base_number = Scalar::from_type(base).and_then(|s| s.to_number());
    let exponent_number = Scalar::from_type(exponent).and_then(|s| s.to_number());
    if let (Some(base), Some(exponent)) = (&base_number, &exponent_number) {
        return fold(base, exponent);
    }
    let float_base = base.is_float_subtype();
    match exponent_number {
        Some(Scalar::Int(0)) => {
            return Some(Raised::Other(if float_base {
                PhpType::literal_float("1.0")
            } else {
                PhpType::literal_int("1")
            }));
        }
        Some(Scalar::Int(1)) if float_base || base.is_int_subtype() => {
            return Some(match IntInterval::of_int(base) {
                Some(interval) => Raised::Int(interval),
                None => Raised::Other(base.clone()),
            });
        }
        _ => {}
    }
    let base_interval = match base_number {
        Some(Scalar::Int(value)) => Some(IntInterval {
            min: Some(value),
            max: Some(value),
        }),
        Some(_) => None,
        None => IntInterval::of_int(base),
    };
    if let (Some(base), Some(exponent)) = (base_interval, IntInterval::of_int(exponent))
        && let Some(interval) = base.pow(exponent)
    {
        return Some(Raised::Int(interval));
    }
    float_base.then(|| Raised::Other(PhpType::float()))
}

/// Two known numbers raised the way PHP does, or `None` when the result is
/// not one finite number.
fn fold(base: &Scalar, exponent: &Scalar) -> Option<Raised> {
    if let (Scalar::Int(base), Scalar::Int(exponent)) = (base, exponent)
        && let Ok(exponent) = u32::try_from(*exponent)
        && let Some(value) = i128::from(*base).checked_pow(exponent)
        && let Ok(value) = i64::try_from(value)
    {
        return Some(Raised::Int(IntInterval {
            min: Some(value),
            max: Some(value),
        }));
    }
    let as_f64 = |s: &Scalar| match s {
        Scalar::Int(v) => Some(*v as f64),
        Scalar::Float(v) => Some(*v),
        _ => None,
    };
    let value = as_f64(base)?.powf(as_f64(exponent)?);
    value
        .is_finite()
        .then(|| Raised::Other(PhpType::literal_float(format!("{value:?}"))))
}
