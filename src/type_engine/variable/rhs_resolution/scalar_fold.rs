//! Folding operators over operands that each hold one known scalar value.
//!
//! `'1' . 'a'`, `(int) '1'`, `"$a"` and `'1' <=> 'a'` all have exactly one
//! value when their operands do, and PHP's own conversion rules decide what
//! it is. An operand that is not pinned to one scalar, or a conversion PHP
//! answers with a warning, an error, or a value that depends on runtime
//! configuration beyond the defaults, leaves the operation unfolded so the
//! caller falls back to the operator's base type.
//!
//! Only operands that are cheap to look up are ever resolved for folding
//! (see [`is_cheap_scalar_operand`]): a cast of a method call does not pay
//! for resolving the call just to learn it cannot be folded.

use std::cmp::Ordering;

use mago_syntax::cst::string::{CompositeString, StringPart};
use mago_syntax::cst::unary::UnaryPrefixOperator;
use mago_syntax::cst::{Access, Expression, Variable};

use crate::atom::bytes_to_str;
use crate::php_type::{LiteralValue, PhpType, TypeKind, with_keyword_lowercase};
use crate::type_engine::type_ops::int_interval::IntInterval;
use crate::types::ResolvedType;

/// One known scalar value.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Scalar {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Null,
}

impl Scalar {
    /// The scalar a type pins its value to, or `None` for a type that
    /// admits more than one value.
    pub(super) fn from_type(ty: &PhpType) -> Option<Self> {
        if let Some(literal) = ty.as_literal() {
            return match literal {
                LiteralValue::Int(_) => literal.parse_i64().map(Scalar::Int),
                LiteralValue::Float(_) => literal
                    .parse_f64()
                    .filter(|value| value.is_finite())
                    .map(Scalar::Float),
                LiteralValue::String(_) => literal
                    .string_content()
                    .map(|content| Scalar::String(content.into_owned())),
            };
        }
        match ty.kind() {
            TypeKind::Named(name) if name.eq_ignore_ascii_case("true") => Some(Scalar::Bool(true)),
            TypeKind::Named(name) if name.eq_ignore_ascii_case("false") => {
                Some(Scalar::Bool(false))
            }
            TypeKind::Named(name) if name.eq_ignore_ascii_case("null") => Some(Scalar::Null),
            _ => None,
        }
    }

    pub(super) fn into_type(self) -> PhpType {
        match self {
            Scalar::Int(value) => PhpType::literal_int(value.to_string()),
            Scalar::Float(value) if value.is_finite() => {
                PhpType::literal_float(format!("{value:?}"))
            }
            Scalar::Float(_) => PhpType::float(),
            Scalar::String(value) => PhpType::literal_string_value(value),
            Scalar::Bool(true) => PhpType::true_(),
            Scalar::Bool(false) => PhpType::false_(),
            Scalar::Null => PhpType::null(),
        }
    }

    /// The string PHP converts the value to (`(string)`, `.`, interpolation).
    pub(super) fn to_php_string(&self) -> Option<String> {
        Some(match self {
            Scalar::Int(value) => value.to_string(),
            Scalar::Float(value) => php_float_to_string(*value)?,
            Scalar::String(value) => value.clone(),
            Scalar::Bool(true) => "1".to_string(),
            Scalar::Bool(false) | Scalar::Null => String::new(),
        })
    }

    /// The number PHP reads the value as for `+`, `-` and friends: a
    /// numeric string counts, anything else that would warn does not.
    fn to_number(&self) -> Option<Scalar> {
        match self {
            Scalar::Int(_) | Scalar::Float(_) => Some(self.clone()),
            Scalar::Bool(value) => Some(Scalar::Int(i64::from(*value))),
            Scalar::Null => Some(Scalar::Int(0)),
            Scalar::String(content) => {
                match LiteralValue::string_value(content).numeric_string_value()? {
                    number @ LiteralValue::Int(_) => number.parse_i64().map(Scalar::Int),
                    number => number.parse_f64().map(Scalar::Float),
                }
            }
        }
    }

    /// The value of `(int)`.
    fn to_int(&self) -> Option<i64> {
        match self.to_number()? {
            Scalar::Int(value) => Some(value),
            // Out of range (or non-finite) is platform-dependent in PHP, so
            // it is not folded.
            Scalar::Float(value) if value.is_finite() && value.abs() < 9.2e18 => {
                Some(value.trunc() as i64)
            }
            _ => None,
        }
    }

    /// The value of `(float)`.
    fn to_float(&self) -> Option<f64> {
        match self.to_number()? {
            Scalar::Int(value) => Some(value as f64),
            Scalar::Float(value) => Some(value),
            _ => None,
        }
    }

    fn truthiness(&self) -> bool {
        match self {
            Scalar::Int(value) => *value != 0,
            Scalar::Float(value) => *value != 0.0,
            Scalar::String(value) => !value.is_empty() && value != "0",
            Scalar::Bool(value) => *value,
            Scalar::Null => false,
        }
    }

    fn is_numeric_string(&self) -> bool {
        matches!(self, Scalar::String(content) if LiteralValue::string_value(content).is_numeric_string())
    }
}

/// The single scalar a resolved operand holds.
pub(super) fn single_scalar(types: &[ResolvedType]) -> Option<Scalar> {
    let [only] = types else {
        return None;
    };
    Scalar::from_type(&only.type_string)
}

/// Whether an operand is cheap enough to resolve just to see whether it
/// folds: a literal, a variable, a constant, or a sign/cast/concatenation/
/// interpolation built from them. A concatenation or interpolated string
/// counts as cheap without looking inside: resolving it applies this same
/// check to its own operands, so a long concatenation chain is still only
/// walked once.
pub(super) fn is_cheap_scalar_operand(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Parenthesized(inner) => is_cheap_scalar_operand(inner.expression),
        Expression::Literal(_)
        | Expression::Variable(Variable::Direct(_))
        | Expression::ConstantAccess(_)
        | Expression::Access(Access::ClassConstant(_))
        | Expression::CompositeString(_) => true,
        Expression::Binary(binary) => binary.operator.is_concatenation(),
        Expression::UnaryPrefix(unary) => {
            let folds = match unary.operator {
                UnaryPrefixOperator::ObjectCast(..) | UnaryPrefixOperator::UnsetCast(..) => false,
                UnaryPrefixOperator::Negation(_)
                | UnaryPrefixOperator::Plus(_)
                | UnaryPrefixOperator::BitwiseNot(_) => true,
                ref operator => operator.is_cast(),
            };
            folds && is_cheap_scalar_operand(unary.operand)
        }
        _ => false,
    }
}

/// `lhs . rhs` for two known scalars.
pub(super) fn fold_concat(lhs: &Scalar, rhs: &Scalar) -> Option<PhpType> {
    let mut text = lhs.to_php_string()?;
    text.push_str(&rhs.to_php_string()?);
    Some(PhpType::literal_string_value(text))
}

/// `lhs . rhs` for two types that each pin one scalar value, or `None` when
/// either admits more than one.
pub(crate) fn fold_concat_types(lhs: &PhpType, rhs: &PhpType) -> Option<PhpType> {
    fold_concat(&Scalar::from_type(lhs)?, &Scalar::from_type(rhs)?)
}

/// How much the string an operand converts to is known not to be empty.
/// Ordered so the weakest member of a union is its minimum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum StringEmptiness {
    MaybeEmpty,
    /// Never `''`, but possibly `'0'`.
    NonEmpty,
    /// Never `''` or `'0'`.
    NonFalsy,
}

impl StringEmptiness {
    /// What converting any value of the resolved operand to a string is
    /// guaranteed to give. An unresolved operand promises nothing.
    pub(super) fn of_operand(types: &[ResolvedType]) -> Self {
        types
            .iter()
            .map(|resolved| Self::of_type(&resolved.type_string))
            .min()
            .unwrap_or(Self::MaybeEmpty)
    }

    fn of_type(ty: &PhpType) -> Self {
        let from_falsy = |falsy: bool| {
            if falsy {
                Self::NonEmpty
            } else {
                Self::NonFalsy
            }
        };
        match ty.kind() {
            TypeKind::Union(members) => members
                .iter()
                .map(Self::of_type)
                .min()
                .unwrap_or(Self::MaybeEmpty),
            TypeKind::Intersection(members) => members
                .iter()
                .map(Self::of_type)
                .max()
                .unwrap_or(Self::MaybeEmpty),
            TypeKind::Nullable(_) => Self::MaybeEmpty,
            _ if ty.is_true() => Self::NonFalsy,
            TypeKind::Literal(_) => match Scalar::from_type(ty).and_then(|s| s.to_php_string()) {
                Some(text) if text.is_empty() => Self::MaybeEmpty,
                Some(text) => from_falsy(text == "0"),
                None => Self::MaybeEmpty,
            },
            TypeKind::IntRange(min, max) => {
                let bound = |b: &crate::atom::Atom| b.parse::<i64>().ok();
                from_falsy(
                    bound(min).is_none_or(|min| min <= 0) && bound(max).is_none_or(|max| max >= 0),
                )
            }
            TypeKind::ClassString(_) | TypeKind::InterfaceString(_) => Self::NonFalsy,
            TypeKind::Named(name) => with_keyword_lowercase(name, |lower| match lower {
                "positive-int" | "negative-int" | "non-zero-int" | "non-falsy-string"
                | "truthy-string" | "class-string" | "interface-string" | "trait-string"
                | "enum-string" => Self::NonFalsy,
                // Every int and float prints at least one digit.
                "int"
                | "integer"
                | "non-negative-int"
                | "non-positive-int"
                | "float"
                | "double"
                | "numeric"
                | "numeric-string"
                | "non-empty-string"
                | "non-empty-lowercase-string"
                | "non-empty-uppercase-string"
                | "non-empty-literal-string" => Self::NonEmpty,
                _ => Self::MaybeEmpty,
            }),
            _ => Self::MaybeEmpty,
        }
    }

    /// The string type `lhs . rhs` gives: two non-empty halves join into at
    /// least two characters, which cannot be `'0'`. `literal` says both
    /// halves are written in the source, so the result is a
    /// `literal-string` too, and `numeric` that the halves join into a
    /// number (see [`NumericConcat`]).
    pub(super) fn concat_type(lhs: Self, rhs: Self, literal: bool, numeric: bool) -> PhpType {
        let joined = if lhs >= Self::NonEmpty && rhs >= Self::NonEmpty {
            Self::NonFalsy
        } else {
            lhs.max(rhs)
        };
        let mut parts = Vec::with_capacity(3);
        if literal {
            parts.push(if joined == Self::NonEmpty && !numeric {
                PhpType::non_empty_literal_string()
            } else {
                PhpType::literal_string()
            });
        }
        if numeric {
            parts.push(PhpType::numeric_string());
        }
        match joined {
            Self::NonFalsy => parts.push(PhpType::non_falsy_string()),
            // A numeric string is never empty, and `non-empty-literal-string`
            // already says it.
            Self::NonEmpty if parts.is_empty() => parts.push(PhpType::non_empty_string()),
            _ => {}
        }
        match parts.len() {
            0 => PhpType::string(),
            1 => parts.pop().unwrap_or_else(PhpType::string),
            _ => PhpType::intersection(parts),
        }
    }
}

/// The string type `lhs . rhs` gives for operands that do not fold to one
/// value. `rhs` is only resolved when the left side leaves the answer
/// open: a non-falsy left side that is neither written out nor a number
/// decides it alone.
pub(super) fn concat_result(
    lhs: &[ResolvedType],
    rhs: impl FnOnce() -> Vec<ResolvedType>,
) -> PhpType {
    let lhs_emptiness = StringEmptiness::of_operand(lhs);
    let lhs_literal = is_literal_operand(lhs);
    let lhs_numeric = NumericConcat::of_lhs(lhs);
    if lhs_emptiness == StringEmptiness::NonFalsy && !lhs_literal && lhs_numeric.is_none() {
        return PhpType::non_falsy_string();
    }
    let rhs = rhs();
    StringEmptiness::concat_type(
        lhs_emptiness,
        StringEmptiness::of_operand(&rhs),
        lhs_literal && is_literal_operand(&rhs),
        lhs_numeric.is_some_and(|rule| rule.admits(&rhs)),
    )
}

/// Whether converting any value of the resolved operand to a string gives
/// a `literal-string`. An unresolved operand is not.
pub(super) fn is_literal_operand(types: &[ResolvedType]) -> bool {
    !types.is_empty()
        && types
            .iter()
            .all(|resolved| is_literal_string_type(&resolved.type_string))
}

/// Whether converting any value of `ty` to a string gives a
/// `literal-string`: every member is one known scalar or already a literal
/// string.
pub(crate) fn is_literal_string_type(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Union(members) => members.iter().all(is_literal_string_type),
        TypeKind::Intersection(members) => members.iter().any(is_literal_string_type),
        TypeKind::Named(name) => with_keyword_lowercase(name, |lower| {
            matches!(lower, "literal-string" | "non-empty-literal-string")
        }),
        _ => Scalar::from_type(ty).is_some(),
    }
}

/// Whether every value of `ty` is a `numeric-string`.
fn is_numeric_string_type(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Intersection(members) => members.iter().any(is_numeric_string_type),
        TypeKind::Named(name) => name.eq_ignore_ascii_case("numeric-string"),
        _ => false,
    }
}

/// What the left operand of a concatenation lets the right one append
/// while the result stays a number, the way PHPStan's
/// `InitializerExprTypeResolver::resolveConcatType()` decides it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NumericConcat {
    /// An integer: a fraction or more digits keep it a number (`5 . '.01'`).
    AfterInt,
    /// A numeric string, which may already hold a `.` or an exponent: only
    /// more digits are safe.
    AfterNumericString,
}

impl NumericConcat {
    /// The rule for a left operand, or `None` when it is not always a
    /// number. A `''` alternative is ignored: appending to it leaves the
    /// right operand as it was.
    pub(super) fn of_lhs(types: &[ResolvedType]) -> Option<Self> {
        let mut rule = None;
        for member in types.iter().flat_map(|rt| rt.type_string.union_members()) {
            let member_rule = if IntInterval::of_int(member).is_some() {
                Self::AfterInt
            } else if is_numeric_string_type(member)
                || Scalar::from_type(member).is_some_and(|s| s.is_numeric_string())
            {
                Self::AfterNumericString
            } else if matches!(Scalar::from_type(member), Some(Scalar::String(s)) if s.is_empty()) {
                continue;
            } else {
                return None;
            };
            rule = Some(match rule {
                Some(Self::AfterNumericString) => Self::AfterNumericString,
                _ => member_rule,
            });
        }
        rule
    }

    /// Whether appending any value of the right operand keeps the left one
    /// a number: a non-negative integer, or written-out strings that
    /// continue the number.
    pub(super) fn admits(self, rhs: &[ResolvedType]) -> bool {
        let members: Vec<&PhpType> = rhs
            .iter()
            .flat_map(|rt| rt.type_string.union_members())
            .collect();
        if members.is_empty() {
            return false;
        }
        if members
            .iter()
            .all(|m| IntInterval::of_int(m).is_some_and(|i| i.min.is_some_and(|min| min >= 0)))
        {
            return true;
        }
        let mut appended = false;
        for member in members {
            let Some(text) = Scalar::from_type(member).and_then(|s| s.to_php_string()) else {
                return false;
            };
            if text.is_empty() {
                continue;
            }
            let continues = match self {
                Self::AfterNumericString => text.bytes().all(|b| b.is_ascii_digit()),
                // A sign, or whitespace PHP would skip, would land in the
                // middle of the number.
                Self::AfterInt => {
                    text.starts_with(|c: char| c.is_ascii_digit() || c == '.')
                        && LiteralValue::string_value(&text).is_numeric_string()
                }
            };
            if !continues {
                return false;
            }
            appended = true;
        }
        appended
    }
}

/// An interpolated string (`"$a-$b"`, a heredoc) whose every embedded
/// expression folds to a known scalar. `resolve` is only asked about
/// expressions [`is_cheap_scalar_operand`] accepts.
pub(super) fn fold_interpolated<'b>(
    string: &'b CompositeString<'b>,
    mut resolve: impl FnMut(&'b Expression<'b>) -> Vec<ResolvedType>,
) -> Option<PhpType> {
    if matches!(string, CompositeString::ShellExecute(_)) {
        return None;
    }
    let mut text = String::new();
    for part in string.parts().iter() {
        let expression = match part {
            StringPart::Literal(literal) => {
                text.push_str(bytes_to_str(literal.value?));
                continue;
            }
            StringPart::Expression(expression) => expression,
            StringPart::BracedExpression(braced) => braced.expression,
        };
        if !is_cheap_scalar_operand(expression) {
            return None;
        }
        text.push_str(&single_scalar(&resolve(expression))?.to_php_string()?);
    }
    Some(PhpType::literal_string_value(text))
}

/// The value a cast of a known scalar produces, for the casts that have
/// one. `(object)`, `(array)` and `(unset)` are left to the caller.
pub(super) fn fold_cast(operator: &UnaryPrefixOperator<'_>, operand: &Scalar) -> Option<PhpType> {
    Some(match operator {
        UnaryPrefixOperator::IntCast(..) | UnaryPrefixOperator::IntegerCast(..) => {
            PhpType::literal_int(operand.to_int()?.to_string())
        }
        UnaryPrefixOperator::FloatCast(..)
        | UnaryPrefixOperator::DoubleCast(..)
        | UnaryPrefixOperator::RealCast(..) => Scalar::Float(operand.to_float()?).into_type(),
        UnaryPrefixOperator::StringCast(..) | UnaryPrefixOperator::BinaryCast(..) => {
            PhpType::literal_string_value(operand.to_php_string()?)
        }
        UnaryPrefixOperator::BoolCast(..) | UnaryPrefixOperator::BooleanCast(..) => {
            Scalar::Bool(operand.truthiness()).into_type()
        }
        _ => return None,
    })
}

/// `lhs <=> rhs` for two known scalars, following PHP 8's comparison rules.
pub(super) fn fold_spaceship(lhs: &Scalar, rhs: &Scalar) -> Option<PhpType> {
    let value = match compare(lhs, rhs)? {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    };
    Some(PhpType::literal_int(value.to_string()))
}

fn compare(lhs: &Scalar, rhs: &Scalar) -> Option<Ordering> {
    use Scalar::{Bool, Float, Int, Null, String};
    match (lhs, rhs) {
        // `null` against a string compares as `''`.
        (Null, String(s)) => Some(b"".as_slice().cmp(s.as_bytes())),
        (String(s), Null) => Some(s.as_bytes().cmp(b"".as_slice())),
        // `bool` or `null` against anything compares as two bools.
        (Bool(_) | Null, _) | (_, Bool(_) | Null) => Some(lhs.truthiness().cmp(&rhs.truthiness())),
        (String(a), String(b)) => {
            if lhs.is_numeric_string() && rhs.is_numeric_string() {
                compare_numbers(&lhs.to_number()?, &rhs.to_number()?)
            } else {
                Some(a.as_bytes().cmp(b.as_bytes()))
            }
        }
        // A number against a non-numeric string compares as two strings.
        (String(s), number @ (Int(_) | Float(_))) if !lhs.is_numeric_string() => {
            Some(s.as_bytes().cmp(number.to_php_string()?.as_bytes()))
        }
        (number @ (Int(_) | Float(_)), String(s)) if !rhs.is_numeric_string() => {
            Some(number.to_php_string()?.as_bytes().cmp(s.as_bytes()))
        }
        _ => compare_numbers(&lhs.to_number()?, &rhs.to_number()?),
    }
}

fn compare_numbers(lhs: &Scalar, rhs: &Scalar) -> Option<Ordering> {
    match (lhs, rhs) {
        (Scalar::Int(a), Scalar::Int(b)) => Some(a.cmp(b)),
        (Scalar::Int(a), Scalar::Float(b)) => (*a as f64).partial_cmp(b),
        (Scalar::Float(a), Scalar::Int(b)) => a.partial_cmp(&(*b as f64)),
        (Scalar::Float(a), Scalar::Float(b)) => a.partial_cmp(b),
        _ => None,
    }
}

/// The string PHP converts a float to, under the default `precision=14`:
/// fourteen significant digits, trailing zeros dropped, and exponent form
/// (`1.0E+14`, `1.0E-5`) outside `1e-4 ..< 1e14`, the way `%.14G` decides.
fn php_float_to_string(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some(if value.is_sign_negative() { "-0" } else { "0" }.to_string());
    }
    let scientific = format!("{value:.13e}");
    let (mantissa, exponent) = scientific.split_once('e')?;
    let exponent: i32 = exponent.parse().ok()?;
    let (negative, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, mantissa),
    };
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if !(-4..14).contains(&exponent) {
        out.push_str(&digits[..1]);
        out.push('.');
        out.push_str(if digits.len() > 1 { &digits[1..] } else { "0" });
        out.push('E');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&exponent.unsigned_abs().to_string());
    } else if exponent < 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-exponent - 1) as usize));
        out.push_str(digits);
    } else {
        let integer_len = exponent as usize + 1;
        if digits.len() <= integer_len {
            out.push_str(digits);
            out.extend(std::iter::repeat_n('0', integer_len - digits.len()));
        } else {
            out.push_str(&digits[..integer_len]);
            out.push('.');
            out.push_str(&digits[integer_len..]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_convert_to_strings_the_way_php_does() {
        let cases = [
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.3"),
            (1e13, "10000000000000"),
            (1e14, "1.0E+14"),
            (1.5e20, "1.5E+20"),
            (0.0001, "0.0001"),
            (0.00001, "1.0E-5"),
            (123.456, "123.456"),
            (-0.0, "-0"),
        ];
        for (value, expected) in cases {
            assert_eq!(
                php_float_to_string(value).as_deref(),
                Some(expected),
                "{value}"
            );
        }
    }

    #[test]
    fn numeric_strings_read_as_numbers() {
        let s = |text: &str| Scalar::String(text.to_string());
        assert_eq!(s("1").to_number(), Some(Scalar::Int(1)));
        assert_eq!(s(" 1.5 ").to_number(), Some(Scalar::Float(1.5)));
        assert_eq!(s("1e3").to_int(), Some(1000));
        assert_eq!(s("abc").to_number(), None);
        assert_eq!(Scalar::Float(1.9).to_int(), Some(1));
        assert_eq!(Scalar::Float(-1.9).to_int(), Some(-1));
    }

    #[test]
    fn comparison_follows_php_8() {
        let s = |text: &str| Scalar::String(text.to_string());
        assert_eq!(compare(&s("1"), &s("a")), Some(Ordering::Less));
        assert_eq!(compare(&s("10"), &s("9")), Some(Ordering::Greater));
        assert_eq!(compare(&s("abc"), &s("abd")), Some(Ordering::Less));
        assert_eq!(compare(&Scalar::Int(0), &s("a")), Some(Ordering::Less));
        assert_eq!(compare(&Scalar::Int(1), &s("1.0")), Some(Ordering::Equal));
        assert_eq!(
            compare(&Scalar::Null, &Scalar::Bool(false)),
            Some(Ordering::Equal)
        );
        assert_eq!(compare(&Scalar::Null, &s("a")), Some(Ordering::Less));
    }
}
