//! The interval of integers a type can hold.
//!
//! Arithmetic on `int<a, b>` operands works on these bounds, the way
//! PHPStan's `InitializerExprTypeResolver::integerRangeMath()` does, and a
//! loop widens a variable whose interval keeps growing between passes to
//! the range covering every pass.

use crate::php_type::{LiteralValue, PhpType, TypeKind, refined_int_to_range};
use crate::types::ResolvedType;

/// An inclusive interval of integers; a `None` bound is unbounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IntInterval {
    pub(crate) min: Option<i64>,
    pub(crate) max: Option<i64>,
}

impl IntInterval {
    pub(crate) const UNBOUNDED: Self = Self {
        min: None,
        max: None,
    };

    fn exact(value: i64) -> Self {
        Self {
            min: Some(value),
            max: Some(value),
        }
    }

    /// The interval covering every value an arithmetic operand of type `ty`
    /// can hold, reading `null` and `bool` the way PHP's arithmetic does
    /// (`0`, `0`/`1`). `None` when some alternative is not an integer.
    pub(crate) fn of_operand(ty: &PhpType) -> Option<Self> {
        match ty.kind() {
            TypeKind::Union(members) => {
                let mut members = members.iter();
                let first = Self::of_operand(members.next()?)?;
                members.try_fold(first, |hull, member| {
                    Some(hull.hull(Self::of_operand(member)?))
                })
            }
            TypeKind::Nullable(inner) => Some(Self::of_operand(inner)?.hull(Self::exact(0))),
            TypeKind::Named(name) if name.eq_ignore_ascii_case("null") => Some(Self::exact(0)),
            TypeKind::Named(name) if name.eq_ignore_ascii_case("false") => Some(Self::exact(0)),
            TypeKind::Named(name) if name.eq_ignore_ascii_case("true") => Some(Self::exact(1)),
            TypeKind::Named(name)
                if name.eq_ignore_ascii_case("bool") || name.eq_ignore_ascii_case("boolean") =>
            {
                Some(Self {
                    min: Some(0),
                    max: Some(1),
                })
            }
            _ => Self::of_int(ty),
        }
    }

    /// The interval of a single integer type (a literal, a range, or one of
    /// the named `int` refinements), or `None` for anything else.
    pub(crate) fn of_int(ty: &PhpType) -> Option<Self> {
        match ty.kind() {
            TypeKind::Literal(literal) => match **literal {
                LiteralValue::Int(_) => literal.parse_i64().map(Self::exact),
                _ => None,
            },
            TypeKind::IntRange(min, max) => Some(Self {
                min: parse_bound(min, "min"),
                max: parse_bound(max, "max"),
            }),
            TypeKind::Named(name) => {
                let lower = name.to_ascii_lowercase();
                if let Some((min, max)) = refined_int_to_range(&lower) {
                    Some(Self {
                        min: parse_bound(min, "min"),
                        max: parse_bound(max, "max"),
                    })
                } else {
                    ty.is_int_subtype().then_some(Self::UNBOUNDED)
                }
            }
            _ => None,
        }
    }

    /// The smallest interval covering both.
    fn hull(self, other: Self) -> Self {
        Self {
            min: self.min.zip(other.min).map(|(a, b)| a.min(b)),
            max: self.max.zip(other.max).map(|(a, b)| a.max(b)),
        }
    }

    /// The integers both hold, or `None` when they share none.
    pub(crate) fn intersect(self, other: Self) -> Option<Self> {
        let pick = |a: Option<i64>, b: Option<i64>, tighter: fn(i64, i64) -> i64| match (a, b) {
            (Some(a), Some(b)) => Some(tighter(a, b)),
            (bound, None) | (None, bound) => bound,
        };
        let min = pick(self.min, other.min, i64::max);
        let max = pick(self.max, other.max, i64::min);
        match (min, max) {
            (Some(min), Some(max)) if min > max => None,
            _ => Some(Self { min, max }),
        }
    }

    /// The integers `ordering` admits against `bound`: `Less` is
    /// `int<min, bound - 1>`, `GreaterOrEqual` is `int<bound, max>`, and so
    /// on. `None` for a bound past what an `int` can reach.
    pub(crate) fn ordered_against(bound: i64, less: bool, or_equal: bool) -> Option<Self> {
        Some(match (less, or_equal) {
            (true, true) => Self {
                min: None,
                max: Some(bound),
            },
            (true, false) => Self {
                min: None,
                max: Some(bound.checked_sub(1)?),
            },
            (false, true) => Self {
                min: Some(bound),
                max: None,
            },
            (false, false) => Self {
                min: Some(bound.checked_add(1)?),
                max: None,
            },
        })
    }

    /// The smallest interval covering all of `intervals`.
    pub(crate) fn hull_of(intervals: &[Self]) -> Self {
        intervals
            .iter()
            .copied()
            .reduce(Self::hull)
            .unwrap_or(Self::UNBOUNDED)
    }

    /// Whether `other` reaches below or above this interval.
    fn is_exceeded_by(self, other: Self) -> (bool, bool) {
        let below = match (self.min, other.min) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(mine), Some(theirs)) => theirs < mine,
        };
        let above = match (self.max, other.max) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(mine), Some(theirs)) => theirs > mine,
        };
        (below, above)
    }

    pub(crate) fn add(self, other: Self) -> Self {
        Self {
            min: self.min.zip(other.min).and_then(|(a, b)| a.checked_add(b)),
            max: self.max.zip(other.max).and_then(|(a, b)| a.checked_add(b)),
        }
    }

    pub(crate) fn sub(self, other: Self) -> Self {
        Self {
            min: self.min.zip(other.max).and_then(|(a, b)| a.checked_sub(b)),
            max: self.max.zip(other.min).and_then(|(a, b)| a.checked_sub(b)),
        }
    }

    /// The product's interval: the extremes of the four corner products,
    /// where an unbounded side times zero is zero.
    pub(crate) fn mul(self, other: Self) -> Self {
        let corners = [
            Bound::low(self.min).mul(Bound::low(other.min)),
            Bound::low(self.min).mul(Bound::high(other.max)),
            Bound::high(self.max).mul(Bound::low(other.min)),
            Bound::high(self.max).mul(Bound::high(other.max)),
        ];
        let min = corners.iter().copied().min().unwrap_or(Bound::NegInf);
        let max = corners.iter().copied().max().unwrap_or(Bound::PosInf);
        Self {
            min: min.finite(),
            max: max.finite(),
        }
    }

    /// The type holding exactly the integers of this interval.
    pub(crate) fn into_type(self) -> PhpType {
        match (self.min, self.max) {
            (None, None) => PhpType::int(),
            (Some(min), Some(max)) if min == max => PhpType::literal_int(min.to_string()),
            (min, max) => PhpType::int_range(
                min.map_or_else(|| "min".to_string(), |v| v.to_string()),
                max.map_or_else(|| "max".to_string(), |v| v.to_string()),
            ),
        }
    }
}

/// A range bound, with `"min"`/`"max"` (or anything unparsable) unbounded.
fn parse_bound(bound: &str, unbounded: &str) -> Option<i64> {
    if bound.eq_ignore_ascii_case(unbounded) {
        return None;
    }
    bound.trim().parse().ok()
}

/// An interval end on the extended number line, wide enough that a product
/// of two `i64`s cannot overflow it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Bound {
    NegInf,
    Finite(i128),
    PosInf,
}

impl Bound {
    fn low(bound: Option<i64>) -> Self {
        bound.map_or(Self::NegInf, |v| Self::Finite(v.into()))
    }

    fn high(bound: Option<i64>) -> Self {
        bound.map_or(Self::PosInf, |v| Self::Finite(v.into()))
    }

    fn signum(self) -> i128 {
        match self {
            Self::NegInf => -1,
            Self::Finite(v) => v.signum(),
            Self::PosInf => 1,
        }
    }

    fn mul(self, other: Self) -> Self {
        match (self, other) {
            (Self::Finite(a), Self::Finite(b)) => Self::Finite(a * b),
            _ => match self.signum() * other.signum() {
                0 => Self::Finite(0),
                s if s < 0 => Self::NegInf,
                _ => Self::PosInf,
            },
        }
    }

    /// The bound as an `i64`, or `None` past what an `int` can hold (where
    /// PHP itself overflows into a `float`, which the range ignores the
    /// same way PHPStan's does).
    fn finite(self) -> Option<i64> {
        match self {
            Self::Finite(v) => i64::try_from(v).ok(),
            _ => None,
        }
    }
}

/// The interval covering a local's integer alternatives, ignoring every
/// other alternative, or `None` when it has no integer alternative.
fn int_alternatives_hull(types: &[ResolvedType]) -> Option<IntInterval> {
    let mut hull: Option<IntInterval> = None;
    for rt in types {
        for member in rt.type_string.union_members() {
            if let Some(interval) = IntInterval::of_int(member) {
                hull = Some(hull.map_or(interval, |h| h.hull(interval)));
            }
        }
    }
    hull
}

/// Widen a loop-carried local whose integers grew from one entry of the
/// loop body to the next, the way PHPStan's `generalizeWith()` does.
///
/// A bound that moved is taken as one that keeps moving on every further
/// pass: `$i = $i + 1` enters the body as `0`, then as `0|1`, and the loop
/// may run any number of times, so the range is opened on that side
/// (`int<0, max>`). Every other alternative the local holds is kept.
///
/// Returns `None` when nothing grew, or the widened types otherwise.
pub(crate) fn widen_grown_ints(
    previous: &[ResolvedType],
    current: &[ResolvedType],
) -> Option<Vec<ResolvedType>> {
    let before = int_alternatives_hull(previous)?;
    let after = int_alternatives_hull(current)?;
    let (below, above) = before.is_exceeded_by(after);
    let widened = IntInterval {
        min: if below { None } else { after.min },
        max: if above { None } else { after.max },
    };
    if widened == after {
        return None;
    }

    let mut types = Vec::with_capacity(current.len());
    for rt in current {
        let members = rt.type_string.union_members();
        if !members.iter().any(|m| IntInterval::of_int(m).is_some()) {
            types.push(rt.clone());
            continue;
        }
        let rest: Vec<PhpType> = members
            .into_iter()
            .filter(|m| IntInterval::of_int(m).is_none())
            .cloned()
            .collect();
        if !rest.is_empty() {
            types.push(ResolvedType::from_type_string(PhpType::union(rest)));
        }
    }
    types.insert(0, ResolvedType::from_type_string(widened.into_type()));
    Some(types)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interval(min: Option<i64>, max: Option<i64>) -> IntInterval {
        IntInterval { min, max }
    }

    #[test]
    fn addition_and_subtraction_move_the_bounds() {
        let non_negative = interval(Some(0), None);
        assert_eq!(
            non_negative.add(IntInterval::exact(1)),
            interval(Some(1), None)
        );
        assert_eq!(
            interval(Some(0), Some(10)).sub(interval(Some(1), Some(3))),
            interval(Some(-3), Some(9))
        );
        assert_eq!(
            non_negative.sub(IntInterval::exact(1)),
            interval(Some(-1), None)
        );
    }

    #[test]
    fn multiplication_takes_the_extreme_corners() {
        assert_eq!(
            interval(Some(-2), Some(3)).mul(interval(Some(4), Some(5))),
            interval(Some(-10), Some(15))
        );
        assert_eq!(
            interval(Some(0), None).mul(IntInterval::exact(2)),
            interval(Some(0), None)
        );
        assert_eq!(
            interval(Some(0), None).mul(IntInterval::exact(-1)),
            interval(None, Some(0))
        );
        assert_eq!(
            IntInterval::UNBOUNDED.mul(IntInterval::exact(0)),
            IntInterval::exact(0)
        );
    }

    #[test]
    fn overflowing_bounds_become_unbounded() {
        assert_eq!(
            interval(Some(i64::MAX - 1), Some(i64::MAX)).add(IntInterval::exact(1)),
            interval(Some(i64::MAX), None)
        );
        assert_eq!(
            interval(Some(i64::MAX / 2), Some(i64::MAX)).mul(IntInterval::exact(4)),
            interval(None, None)
        );
    }

    #[test]
    fn operands_read_null_and_bool_as_numbers() {
        let ty = PhpType::parse("int<1, 5>|null");
        assert_eq!(
            IntInterval::of_operand(&ty),
            Some(interval(Some(0), Some(5)))
        );
        assert_eq!(
            IntInterval::of_operand(&PhpType::parse("bool")),
            Some(interval(Some(0), Some(1)))
        );
        assert_eq!(
            IntInterval::of_operand(&PhpType::parse("positive-int")),
            Some(interval(Some(1), None))
        );
        assert_eq!(IntInterval::of_operand(&PhpType::parse("int|string")), None);
    }

    #[test]
    fn a_grown_bound_is_opened() {
        let previous = [ResolvedType::from_type_string(PhpType::parse("1"))];
        let current = [ResolvedType::from_type_string(PhpType::parse("1|2"))];
        let widened = widen_grown_ints(&previous, &current).unwrap();
        assert_eq!(
            ResolvedType::types_joined(&widened).to_string(),
            "int<1, max>"
        );

        let current = [ResolvedType::from_type_string(PhpType::parse("1|2|null"))];
        let widened = widen_grown_ints(&previous, &current).unwrap();
        assert_eq!(
            ResolvedType::types_joined(&widened).to_string(),
            "int<1, max>|null"
        );

        let same = [ResolvedType::from_type_string(PhpType::parse("0|5"))];
        assert!(widen_grown_ints(&same, &same).is_none());
    }
}
