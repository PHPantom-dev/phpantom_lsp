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
use mago_syntax::cst::{Access, ClassLikeConstantSelector, Expression, Variable};

use crate::atom::bytes_to_str;
use crate::php_type::{LiteralValue, PhpType, TypeKind, with_keyword_lowercase};
use crate::type_engine::type_ops::int_interval::IntInterval;
use crate::types::ResolvedType;

/// The string PHP converts a value of type `ty` to, when `ty` pins it to
/// one scalar (`123` is `'123'`, `null` is `''`).
pub(crate) fn php_string_of_type(ty: &PhpType) -> Option<String> {
    Scalar::from_type(ty)?.to_php_string()
}

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
                    .exact_string_content()
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
    pub(super) fn to_number(&self) -> Option<Scalar> {
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

    /// The number a cast reads: a string's leading numeric prefix, or `0`
    /// when it has none (`(int) '12abc'` is `12`, `(int) 'abc'` is `0`),
    /// where arithmetic would reject the string.
    fn to_cast_number(&self) -> Option<Scalar> {
        let Scalar::String(content) = self else {
            return self.to_number();
        };
        let text = content.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
        let bytes = text.as_bytes();
        let digits_from = |mut i: usize| {
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            i
        };
        let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
        let int_start = end;
        end = digits_from(end);
        let mut has_digits = end > int_start;
        let mut fractional = false;
        if bytes.get(end) == Some(&b'.') {
            let frac_end = digits_from(end + 1);
            if has_digits || frac_end > end + 1 {
                has_digits = true;
                fractional = true;
                end = frac_end;
            }
        }
        if !has_digits {
            return Some(Scalar::Int(0));
        }
        if matches!(bytes.get(end), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
            let exp_end = digits_from(end + 1 + sign);
            if exp_end > end + 1 + sign {
                fractional = true;
                end = exp_end;
            }
        }
        let prefix = &text[..end];
        if !fractional && let Ok(value) = prefix.parse::<i64>() {
            return Some(Scalar::Int(value));
        }
        prefix.parse::<f64>().ok().map(Scalar::Float)
    }

    /// The value of `(int)`.
    fn to_int(&self) -> Option<i64> {
        match self.to_cast_number()? {
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
        match self.to_cast_number()? {
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
/// folds: a literal, a variable, a constant, a call to a builtin whose
/// arguments decide its result (`strtolower()`, `rand()`), or a
/// sign/cast/concatenation/interpolation built from them. A concatenation
/// or interpolated string counts as cheap without looking inside:
/// resolving it applies this same check to its own operands, so a long
/// concatenation chain is still only walked once.
pub(super) fn is_cheap_scalar_operand(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Parenthesized(inner) => is_cheap_scalar_operand(inner.expression),
        Expression::Literal(_)
        | Expression::MagicConstant(_)
        | Expression::Variable(Variable::Direct(_))
        | Expression::ConstantAccess(_)
        | Expression::Access(Access::ClassConstant(_))
        | Expression::CompositeString(_) => true,
        Expression::Call(mago_syntax::cst::Call::Function(call)) => {
            matches!(call.function, Expression::Identifier(name) if {
                let name = bytes_to_str(name.value());
                crate::type_engine::variable::string_func_rules::has_string_func_rule(name)
                    || crate::type_engine::type_ops::call_site_returns::has_call_site_return(name)
            }) && call
                .argument_list
                .arguments
                .iter()
                .all(|argument| is_cheap_scalar_operand(argument.value()))
        }
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

/// How many strings a concatenation or interpolation may fold to before it
/// answers with the general string type instead of listing them all.
/// PHPStan's `CALCULATE_SCALARS_LIMIT`.
const CONCAT_COMBINATION_LIMIT: usize = 128;

/// Every string converting a value of the resolved operand gives, when the
/// operand is a union of known scalars (`'a'|'b'`, `?'a'`, `bool`), or
/// `None` when it admits a value that is not one known scalar.
pub(super) fn operand_strings(types: &[ResolvedType]) -> Option<Vec<String>> {
    fn collect(ty: &PhpType, out: &mut Vec<String>) -> Option<()> {
        match ty.kind() {
            TypeKind::Union(members) => {
                for member in members.iter() {
                    collect(member, out)?;
                }
            }
            TypeKind::Nullable(inner) => {
                out.push(String::new());
                collect(inner, out)?;
            }
            TypeKind::Named(name)
                if name.eq_ignore_ascii_case("bool") || name.eq_ignore_ascii_case("boolean") =>
            {
                out.push(String::new());
                out.push("1".to_string());
            }
            _ => out.push(Scalar::from_type(ty)?.to_php_string()?),
        }
        Some(())
    }
    if types.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for resolved in types {
        collect(&resolved.type_string, &mut out)?;
        if out.len() > CONCAT_COMBINATION_LIMIT {
            return None;
        }
    }
    out.sort_unstable();
    out.dedup();
    Some(out)
}

/// Every string a concatenation operand converts to: the ones its type
/// pins (see [`operand_strings`]), or the name `Foo::class` gives.  PHP
/// resolves a class name written before `::class` at compile time, so it
/// is one known string; the type, `class-string<Foo>`, also admits the
/// names of subclasses, so the expression is read rather than the type.
/// `static::class` and `$object::class` name the runtime class, which is
/// not known, and so does `self::class` in a trait.
pub(super) fn concat_operand_strings(
    expr: &Expression<'_>,
    types: &[ResolvedType],
) -> Option<Vec<String>> {
    if let Some(strings) = operand_strings(types) {
        return Some(strings);
    }
    let mut expr = expr;
    while let Expression::Parenthesized(inner) = expr {
        expr = inner.expression;
    }
    let Expression::Access(Access::ClassConstant(access)) = expr else {
        return None;
    };
    let ClassLikeConstantSelector::Identifier(constant) = &access.constant else {
        return None;
    };
    if !constant.value.eq_ignore_ascii_case(b"class")
        || !matches!(access.class, Expression::Identifier(_))
    {
        return None;
    }
    let [only] = types else {
        return None;
    };
    match only.type_string.kind() {
        TypeKind::ClassString(Some(inner)) => match inner.kind() {
            TypeKind::Named(name) => Some(vec![name.to_string()]),
            _ => None,
        },
        _ => None,
    }
}

/// `lhs . rhs` for two known scalars.
fn fold_concat(lhs: &Scalar, rhs: &Scalar) -> Option<PhpType> {
    let mut text = lhs.to_php_string()?;
    text.push_str(&rhs.to_php_string()?);
    Some(PhpType::literal_string_value(text))
}

/// Every string `lhs . rhs` can give, for operands that each convert to
/// one of a known set of strings, or `None` when there are more than
/// [`CONCAT_COMBINATION_LIMIT`] of them.
pub(super) fn concat_string_sets(lhs: &[String], rhs: &[String]) -> Option<Vec<String>> {
    if lhs.len() * rhs.len() > CONCAT_COMBINATION_LIMIT {
        return None;
    }
    let mut joined: Vec<String> = lhs
        .iter()
        .flat_map(|left| rhs.iter().map(move |right| format!("{left}{right}")))
        .collect();
    joined.sort_unstable();
    joined.dedup();
    Some(joined)
}

/// Widen a loop-carried local whose literal strings grew from one entry of
/// the loop body to the next, the way PHPStan's `generalizeWith()` does.
///
/// `$s .= 'x'` enters the body as `''`, then as `''|'x'`, and the loop may
/// run any number of times, so the literals are replaced by the
/// `literal-string` they all are, keeping what they promise about being
/// empty. Every other alternative the local holds is kept.
///
/// Returns `None` when no new literal string appeared, or the widened
/// types otherwise.
pub(crate) fn widen_grown_strings(
    previous: &[ResolvedType],
    current: &[ResolvedType],
) -> Option<Vec<ResolvedType>> {
    let is_literal_string = |ty: &PhpType| matches!(ty.as_literal(), Some(LiteralValue::String(_)));
    let literals_of = |types: &[ResolvedType]| -> Vec<PhpType> {
        types
            .iter()
            .flat_map(|rt| rt.type_string.union_members())
            .filter(|member| is_literal_string(member))
            .cloned()
            .collect()
    };
    let now = literals_of(current);
    if now.is_empty() {
        return None;
    }
    let before = literals_of(previous);
    if now.iter().all(|literal| before.contains(literal)) {
        return None;
    }

    let emptiness = now
        .iter()
        .map(StringEmptiness::of_type)
        .min()
        .unwrap_or(StringEmptiness::MaybeEmpty);
    let widened = match emptiness {
        StringEmptiness::NonFalsy => {
            PhpType::intersection(vec![PhpType::literal_string(), PhpType::non_falsy_string()])
        }
        StringEmptiness::NonEmpty => PhpType::non_empty_literal_string(),
        StringEmptiness::MaybeEmpty => PhpType::literal_string(),
    };
    let mut types = vec![ResolvedType::from_type_string(widened)];
    for rt in current {
        let members = rt.type_string.union_members();
        if !members.iter().any(|member| is_literal_string(member)) {
            types.push(rt.clone());
            continue;
        }
        let rest: Vec<PhpType> = members
            .into_iter()
            .filter(|member| !is_literal_string(member))
            .cloned()
            .collect();
        if !rest.is_empty() {
            types.push(ResolvedType::from_type_string(PhpType::union(rest)));
        }
    }
    Some(types)
}

/// The union of literal strings a set of known strings makes.
pub(super) fn literal_strings_type(strings: Vec<String>) -> PhpType {
    PhpType::union(
        strings
            .into_iter()
            .map(PhpType::literal_string_value)
            .collect(),
    )
}

/// `lhs . rhs` folded over every pair of the values the operands can hold,
/// or `None` when either holds a value that is not one known scalar, or
/// there are too many pairs to list.
fn fold_concat_operands(lhs: &[ResolvedType], rhs: &[ResolvedType]) -> Option<PhpType> {
    concat_string_sets(&operand_strings(lhs)?, &operand_strings(rhs)?).map(literal_strings_type)
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

/// The string type `lhs . rhs` gives: the strings it can be when both
/// operands are unions of known scalars, else the general string type.
/// `rhs` is only resolved when the left side leaves the answer open: a
/// non-falsy left side that is neither written out nor a number decides it
/// alone.
pub(super) fn concat_result(
    lhs: &[ResolvedType],
    rhs: impl FnOnce() -> Vec<ResolvedType>,
) -> PhpType {
    if !is_literal_operand(lhs)
        && StringEmptiness::of_operand(lhs) == StringEmptiness::NonFalsy
        && NumericConcat::of_lhs(lhs).is_none()
    {
        return PhpType::non_falsy_string();
    }
    let rhs = rhs();
    if let Some(folded) = fold_concat_operands(lhs, &rhs) {
        return folded;
    }
    unfolded_concat_result(lhs, &rhs)
}

/// The string type `lhs . rhs` gives without listing the strings it can
/// be.
pub(super) fn unfolded_concat_result(lhs: &[ResolvedType], rhs: &[ResolvedType]) -> PhpType {
    let lhs_numeric = NumericConcat::of_lhs(lhs);
    StringEmptiness::concat_type(
        StringEmptiness::of_operand(lhs),
        StringEmptiness::of_operand(rhs),
        is_literal_operand(lhs) && is_literal_operand(rhs),
        lhs_numeric.is_some_and(|rule| rule.admits(rhs)),
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
    /// number. A `''` alternative (`false`, `null`) is ignored: appending
    /// to it leaves the right operand as it was. A `bool` or a `float`
    /// converts to a numeric string that may already hold a `.` or an
    /// exponent, the way PHPStan reads them.
    pub(super) fn of_lhs(types: &[ResolvedType]) -> Option<Self> {
        let mut rule = None;
        for member in types.iter().flat_map(|rt| rt.type_string.union_members()) {
            let member_rule = if IntInterval::of_int(member).is_some() {
                Self::AfterInt
            } else if is_numeric_string_type(member) || is_bool_or_float_type(member) {
                Self::AfterNumericString
            } else {
                match Scalar::from_type(member).and_then(|s| s.to_php_string()) {
                    Some(text) if text.is_empty() => continue,
                    Some(text) if LiteralValue::string_value(&text).is_numeric_string() => {
                        Self::AfterNumericString
                    }
                    _ => return None,
                }
            };
            rule = Some(match rule {
                Some(Self::AfterNumericString) => Self::AfterNumericString,
                _ => member_rule,
            });
        }
        rule
    }

    /// Whether appending any value of the right operand keeps the left one
    /// a number: a non-negative integer, or written-out strings (a `bool`
    /// reads as `''` or `'1'`) that continue the number.
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
            let text = if is_bool_type(member) {
                "1".to_string()
            } else if let Some(text) = Scalar::from_type(member).and_then(|s| s.to_php_string()) {
                text
            } else {
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

/// Whether `ty` is the `bool` type, whose `false` converts to `''` and
/// `true` to `'1'`.
fn is_bool_type(ty: &PhpType) -> bool {
    matches!(ty.kind(), TypeKind::Named(name) if name.eq_ignore_ascii_case("bool") || name.eq_ignore_ascii_case("boolean"))
}

/// Whether converting any non-empty value of `ty` to a string gives a
/// number: `bool` (`'1'`) and `float`.
fn is_bool_or_float_type(ty: &PhpType) -> bool {
    is_bool_type(ty)
        || matches!(ty.kind(), TypeKind::Named(name) if name.eq_ignore_ascii_case("float") || name.eq_ignore_ascii_case("double"))
}

/// The type of an interpolated string (`"$a-$b"`, a heredoc): every string
/// it can build while each embedded expression is a union of known scalars
/// and there are few enough of them, else the string type joining its parts
/// with `.` gives. `resolve` is only asked about expressions
/// [`is_cheap_scalar_operand`] accepts; any other part counts as an unknown
/// string.
pub(super) fn interpolated_type<'b>(
    string: &'b CompositeString<'b>,
    mut resolve: impl FnMut(&'b Expression<'b>) -> Vec<ResolvedType>,
) -> PhpType {
    /// What is known about the parts joined so far.
    enum Joined {
        /// Every string they can be.
        Strings(Vec<String>),
        /// Only their type.
        General(PhpType),
    }
    let mut joined = Joined::Strings(vec![String::new()]);
    for part in string.parts().iter() {
        let part_types = match part {
            StringPart::Literal(literal) => {
                let Some(value) = literal.value else {
                    return PhpType::string();
                };
                let literal = bytes_to_str(value);
                if let Joined::Strings(texts) = &mut joined {
                    for text in texts {
                        text.push_str(literal);
                    }
                    continue;
                }
                vec![ResolvedType::from_type_string(
                    PhpType::literal_string_value(literal),
                )]
            }
            StringPart::Expression(expression) if is_cheap_scalar_operand(expression) => {
                resolve(expression)
            }
            StringPart::BracedExpression(braced) if is_cheap_scalar_operand(braced.expression) => {
                resolve(braced.expression)
            }
            StringPart::Expression(_) | StringPart::BracedExpression(_) => Vec::new(),
        };
        let lhs = match joined {
            Joined::Strings(texts) => {
                if let Some(product) = operand_strings(&part_types)
                    .and_then(|values| concat_string_sets(&texts, &values))
                {
                    joined = Joined::Strings(product);
                    continue;
                }
                literal_strings_type(texts)
            }
            Joined::General(lhs) => lhs,
        };
        joined = Joined::General(unfolded_concat_result(
            &[ResolvedType::from_type_string(lhs)],
            &part_types,
        ));
    }
    match joined {
        Joined::Strings(texts) => literal_strings_type(texts),
        Joined::General(ty) => ty,
    }
}

/// The value a cast of a known scalar produces, for the casts that have
/// one. `(object)`, `(array)` and `(unset)` are left to the caller.
pub(super) fn fold_cast(kind: super::unary_prefix::CastKind, operand: &Scalar) -> Option<PhpType> {
    use super::unary_prefix::CastKind;
    Some(match kind {
        CastKind::Int => PhpType::literal_int(operand.to_int()?.to_string()),
        CastKind::Float => Scalar::Float(operand.to_float()?).into_type(),
        CastKind::String => PhpType::literal_string_value(operand.to_php_string()?),
        CastKind::Bool => Scalar::Bool(operand.truthiness()).into_type(),
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
    fn a_literal_whose_escapes_are_not_utf8_is_not_a_known_scalar() {
        let bytes = PhpType::literal_string_raw(r#""\xfe""#);
        assert_eq!(Scalar::from_type(&bytes), None);
        let text = PhpType::literal_string_raw(r#""\x41""#);
        assert_eq!(
            Scalar::from_type(&text),
            Some(Scalar::String("A".to_string()))
        );
    }

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
    fn casts_read_a_leading_numeric_prefix() {
        let s = |text: &str| Scalar::String(text.to_string());
        assert_eq!(s("12abc").to_int(), Some(12));
        assert_eq!(s("abc").to_int(), Some(0));
        assert_eq!(s("").to_float(), Some(0.0));
        assert_eq!(s(" -1.5e2x").to_float(), Some(-150.0));
        assert_eq!(s("1e").to_int(), Some(1));
        assert_eq!(s(".5").to_float(), Some(0.5));
        assert_eq!(s("-").to_int(), Some(0));
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
