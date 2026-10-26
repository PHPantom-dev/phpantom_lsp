//! The types unary prefix operators produce: casts, `!`, `~`, the signs
//! `-`/`+`, and the numeric step of `++`/`--`.

use mago_syntax::cst::{Expression, unary};

use crate::atom::atom;
use crate::class_lookup::ResolvedClassLoader;
use crate::php_type::{LiteralValue, PhpType, ShapeEntry, TypeKind, with_keyword_lowercase};
use crate::types::{ClassInfo, ResolvedType};
use crate::virtual_members::ResolvedClassCache;

use super::arithmetic::OverloadedNumber;
use super::scalar_fold;

/// The type a unary prefix expression produces, for the operators whose
/// result is determined by the operator plus (for `(object)`, `~` and the
/// signs) the operand type.
///
/// A cast, sign or `~` of an operand that holds one known scalar folds to
/// the value PHP computes (`(int) '1'` is `1`, `~1` is `-2`, `-(1)` is
/// `-1`). `resolve_operand` is always called for `(string)`, `(object)`,
/// `(array)`, `~`, `-` and `+`, and for the other casts only when `operand`
/// is cheap to resolve (a literal, variable, or constant), so an `(int)`
/// cast of a call pays nothing for it.
///
/// `class_loader` and `cache` are used by `(string)` to read the type an
/// object's `__toString()` declares.
///
/// Returns `None` for `@`, `&`, `++` and `--`, whose result is the
/// operand's own type or its numeric step, which the caller works out.
pub(crate) fn unary_prefix_result_type(
    operator: &unary::UnaryPrefixOperator<'_>,
    operand: &Expression<'_>,
    resolve_operand: impl FnOnce() -> Vec<ResolvedType>,
    class_loader: &ResolvedClassLoader<'_>,
    cache: Option<&ResolvedClassCache>,
) -> Option<PhpType> {
    use unary::UnaryPrefixOperator;

    let base = match operator {
        UnaryPrefixOperator::Negation(_) | UnaryPrefixOperator::Plus(_) => {
            let negated = matches!(operator, UnaryPrefixOperator::Negation(_));
            let resolved = resolve_operand();
            // PHP compiles a sign as a multiplication by `-1` or `1`, which
            // `GMP` and `BcMath\Number` overload.
            if let Some(class) = OverloadedNumber::of_operand(&resolved) {
                return Some(class.to_type());
            }
            return Some(signed_numeric_type(&resolved, negated));
        }
        UnaryPrefixOperator::ObjectCast(..) => return Some(object_cast_type(resolve_operand())),
        UnaryPrefixOperator::BitwiseNot(_) => {
            let resolved = resolve_operand();
            if OverloadedNumber::of_operand(&resolved) == Some(OverloadedNumber::Gmp) {
                return Some(OverloadedNumber::Gmp.to_type());
            }
            if let Some(scalar_fold::Scalar::Int(value)) = scalar_fold::single_scalar(&resolved) {
                return Some(PhpType::literal_int((!value).to_string()));
            }
            return Some(bitwise_not_type(&resolved));
        }
        UnaryPrefixOperator::IntCast(..) | UnaryPrefixOperator::IntegerCast(..) => PhpType::int(),
        UnaryPrefixOperator::StringCast(..) | UnaryPrefixOperator::BinaryCast(..) => {
            return Some(string_cast_type(&resolve_operand(), class_loader, cache));
        }
        UnaryPrefixOperator::FloatCast(..)
        | UnaryPrefixOperator::DoubleCast(..)
        | UnaryPrefixOperator::RealCast(..) => PhpType::float(),
        UnaryPrefixOperator::BoolCast(..) | UnaryPrefixOperator::BooleanCast(..) => PhpType::bool(),
        UnaryPrefixOperator::ArrayCast(..) => return Some(array_cast_type(resolve_operand())),
        UnaryPrefixOperator::UnsetCast(..) => return Some(PhpType::named(atom("null"))),
        UnaryPrefixOperator::Not(_) => return Some(PhpType::bool()),
        _ => return None,
    };
    if scalar_fold::is_cheap_scalar_operand(operand)
        && let Some(folded) = scalar_fold::single_scalar(&resolve_operand())
            .and_then(|scalar| scalar_fold::fold_cast(operator, &scalar))
    {
        return Some(folded);
    }
    Some(base)
}

/// The type unary `-` (`negated`) or `+` produces: the operand's numeric
/// type with the sign applied, or `int|float` when the operand is not
/// known to be numeric.
fn signed_numeric_type(operand: &[ResolvedType], negated: bool) -> PhpType {
    (!operand.is_empty())
        .then(|| ResolvedType::types_joined(operand))
        .and_then(|ty| apply_numeric_sign(&ty, negated))
        .unwrap_or_else(|| PhpType::union(vec![PhpType::int(), PhpType::float()]))
}

/// Apply unary `+` or `-` to an already-resolved numeric type.
///
/// Exact literal members remain exact; negating a literal `PHP_INT_MIN`
/// overflows and gives up rather than reporting a wrong value. A broad
/// integer stays an integer, matching PHPStan: modelling the one input that
/// overflows to a float would cost precision on every other negation.
/// Any non-numeric branch makes the result unknown.
fn apply_numeric_sign(ty: &PhpType, negated: bool) -> Option<PhpType> {
    match ty.kind() {
        TypeKind::Literal(value) => match &**value {
            LiteralValue::Int(_) => {
                let value = value.parse_i64()?;
                let value = if negated { value.checked_neg()? } else { value };
                Some(PhpType::literal_int(value.to_string()))
            }
            LiteralValue::Float(raw) => {
                let raw = raw.trim();
                let signed = if negated {
                    if let Some(rest) = raw.strip_prefix('-') {
                        rest.to_string()
                    } else if let Some(rest) = raw.strip_prefix('+') {
                        format!("-{rest}")
                    } else {
                        format!("-{raw}")
                    }
                } else {
                    raw.strip_prefix('+').unwrap_or(raw).to_string()
                };
                Some(PhpType::literal_float(signed))
            }
            // A numeric string reads as the number it spells (`-'1'` is
            // `-1`); any other string warns or throws.
            LiteralValue::String(_) => value
                .numeric_string_value()
                .and_then(|number| apply_numeric_sign(&PhpType::literal(number), negated)),
        },
        TypeKind::Union(members) => {
            let mut signed = Vec::with_capacity(members.len());
            for member in members {
                let member = apply_numeric_sign(member, negated)?;
                for alternative in member.union_members() {
                    if !signed.iter().any(|existing| existing == alternative) {
                        signed.push(alternative.clone());
                    }
                }
            }
            match signed.len() {
                0 => None,
                1 => signed.into_iter().next(),
                _ => Some(PhpType::union(signed)),
            }
        }
        // PHP coerces null here, but the exact result depends on runtime
        // version and diagnostics policy; keep the established conservative
        // fallback instead of silently discarding nullability.
        TypeKind::Nullable(_) => None,
        _ if ty.is_int_subtype() => Some(PhpType::int()),
        _ if ty.is_float_subtype() => Some(PhpType::float()),
        _ if ty.is_named_ci("numeric") || ty.is_named("number") => {
            Some(PhpType::union(vec![PhpType::int(), PhpType::float()]))
        }
        _ => None,
    }
}

/// `(string) $expr`, applied member by member: a string stays as it is, a
/// number becomes a `numeric-string`, `bool` and `null` become the strings
/// PHP spells them as, and an object becomes what its `__toString()`
/// declares. Any member without a better answer makes the result `string`.
fn string_cast_type(
    operand: &[ResolvedType],
    class_loader: &ResolvedClassLoader<'_>,
    cache: Option<&ResolvedClassCache>,
) -> PhpType {
    fn cast_member(member: &PhpType, class: Option<&ClassInfo>, out: &mut Vec<PhpType>) -> bool {
        if let Some(scalar) = scalar_fold::Scalar::from_type(member)
            && let Some(text) = scalar.to_php_string()
        {
            out.push(PhpType::literal_string_value(text));
            return true;
        }
        match member.kind() {
            TypeKind::Union(members) => {
                return members.iter().all(|member| cast_member(member, class, out));
            }
            TypeKind::Nullable(inner) => {
                out.push(PhpType::literal_string_value(String::new()));
                return cast_member(inner, class, out);
            }
            _ => {}
        }
        // A non-finite float spells `INF` or `NAN`, which is not numeric;
        // like PHPStan, the cast ignores that to keep every other float's
        // `numeric-string`.
        let cast = if member.is_subtype_of(&PhpType::string()) {
            member.clone()
        } else if member.is_int_subtype()
            || member.is_float_subtype()
            || member.is_named_ci("numeric")
        {
            PhpType::numeric_string()
        } else if member.is_named_ci("bool") || member.is_named_ci("boolean") {
            out.push(PhpType::literal_string_value(String::new()));
            PhpType::literal_string_value("1")
        } else if let Some(declared) = class.and_then(to_string_return_type) {
            declared
        } else {
            return false;
        };
        out.push(cast);
        true
    }

    let mut members = Vec::new();
    for resolved in operand {
        // The class a resolved value carries belongs to its one object
        // member; with two in the same entry it is not clear which one.
        let class = resolved.class_info.as_deref().filter(|_| {
            resolved
                .type_string
                .union_members()
                .iter()
                .filter(|member| !member.is_scalar() && !member.is_null())
                .count()
                == 1
        });
        let class = class.map(|class| {
            crate::virtual_members::resolve_class_fully_maybe_cached(class, class_loader, cache)
        });
        if !cast_member(&resolved.type_string, class.as_deref(), &mut members) {
            return PhpType::string();
        }
    }
    if members.is_empty() {
        return PhpType::string();
    }
    PhpType::join_runtime_value_types(members)
}

/// The string type a class's `__toString()` declares, if it has one.
fn to_string_return_type(class: &ClassInfo) -> Option<PhpType> {
    class
        .get_method_ci("__toString")?
        .return_type
        .clone()
        .filter(|ty| ty.is_subtype_of(&PhpType::string()))
}

/// `~` yields a string for string operands and an int otherwise.
fn bitwise_not_type(operand: &[ResolvedType]) -> PhpType {
    let is_string = !operand.is_empty()
        && operand
            .iter()
            .all(|rt| rt.type_string.is_subtype_of(&PhpType::string()));
    if is_string {
        PhpType::string()
    } else {
        PhpType::int()
    }
}

/// The type `++`/`--` produces for a numeric operand.
///
/// The step widens a literal (`5` becomes `int`, not `6`) but stays inside
/// the operand's own domain. Non-numeric operands are left unresolved: PHP
/// increments strings alphanumerically, leaves bools and arrays untouched,
/// and treats null asymmetrically (`++null` is `1`, `--null` is `null`).
pub(super) fn stepped_numeric_type(operand: &[ResolvedType]) -> Option<PhpType> {
    if operand.is_empty() {
        return None;
    }
    let joined = ResolvedType::types_joined(operand);
    let mut stepped = Vec::new();
    for member in joined.union_members() {
        if member.is_int_subtype() {
            push_unique_type(&mut stepped, PhpType::int());
        } else if member.is_float_subtype() {
            push_unique_type(&mut stepped, PhpType::float());
        } else {
            return None;
        }
    }
    match stepped.len() {
        0 => None,
        1 => stepped.into_iter().next(),
        _ => Some(PhpType::union(stepped)),
    }
}

fn push_unique_type(types: &mut Vec<PhpType>, member: PhpType) {
    if !types.iter().any(|existing| existing == &member) {
        types.push(member);
    }
}

/// The object shape `(object) $expr` produces: an array shape casts
/// key-for-key, a scalar becomes `object{scalar: T}`, and anything else
/// (including an unresolved operand) falls back to `stdClass`. A cast
/// always produces a `stdClass` instance, so a shape result keeps
/// `stdClass`'s class identity via an intersection.
///
/// The cast does not preserve literal precision, so shape values widen.
fn object_cast_type(operand: Vec<ResolvedType>) -> PhpType {
    let inner =
        (!operand.is_empty()).then(|| ResolvedType::types_joined(&operand).widen_scalar_literals());
    let std_class = || PhpType::named(atom("stdClass"));
    match inner.as_ref().map(PhpType::kind) {
        // `(object) []` is a `stdClass` with no properties; an `object{}`
        // shape would say the same thing in a spelling nothing else uses.
        Some(TypeKind::ArrayShape(entries)) if entries.is_empty() => std_class(),
        Some(TypeKind::ArrayShape(entries)) => PhpType::intersection(vec![
            PhpType::object_shape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.widen_scalar_literals(),
                        optional: e.optional,
                    })
                    .collect(),
            ),
            std_class(),
        ]),
        Some(_) if inner.as_ref().is_some_and(is_object_cast_scalar_type) => {
            PhpType::intersection(vec![
                PhpType::object_shape(vec![ShapeEntry {
                    key: Some("scalar".to_string()),
                    value_type: inner.as_ref().unwrap().clone(),
                    optional: false,
                }]),
                std_class(),
            ])
        }
        _ => std_class(),
    }
}

/// `(array) $expr`, applied member by member: an array stays as it is,
/// `null` becomes `[]`, and a scalar becomes the one-entry list holding it.
/// An object (its properties) or a value of unknown type is some array.
fn array_cast_type(operand: Vec<ResolvedType>) -> PhpType {
    fn cast_member(member: &PhpType, scalar: &PhpType, out: &mut Vec<PhpType>) {
        match member.kind() {
            TypeKind::Union(members) => {
                for member in members.iter() {
                    cast_member(member, scalar, out);
                }
            }
            TypeKind::Nullable(inner) => {
                cast_member(inner, scalar, out);
                out.push(PhpType::array_shape(Vec::new()));
            }
            _ if member.is_null() => out.push(PhpType::array_shape(Vec::new())),
            // `iterable` may be a `Traversable`, which casts to its
            // properties rather than to the values it yields.
            _ if member.is_array_like() && !member.is_named("iterable") => {
                out.push(member.clone());
            }
            _ if member.is_subtype_of(scalar) => out.push(PhpType::array_shape(vec![ShapeEntry {
                key: None,
                value_type: member.clone(),
                optional: false,
            }])),
            _ => out.push(PhpType::array()),
        }
    }

    if operand.is_empty() {
        return PhpType::array();
    }
    let scalar = PhpType::union(vec![
        PhpType::int(),
        PhpType::float(),
        PhpType::string(),
        PhpType::bool(),
    ]);
    let mut members = Vec::new();
    cast_member(&ResolvedType::types_joined(&operand), &scalar, &mut members);
    // The shape a scalar casts to adds nothing beside an array member that
    // already holds it: `string|list<string>` casts to `list<string>`.
    let keep: Vec<bool> = members
        .iter()
        .map(|member| {
            member.shape_entries().is_none()
                || !members
                    .iter()
                    .any(|other| other.shape_entries().is_none() && member.is_subtype_of(other))
        })
        .collect();
    crate::util::retain_by_mask(&mut members, &keep);
    PhpType::join_runtime_value_types(members)
}

/// Whether `(object) $expr` on this type produces an `object{scalar: T}`
/// wrapper rather than a plain `stdClass`.
fn is_object_cast_scalar_type(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(name) => with_keyword_lowercase(name, |lower| {
            matches!(
                lower,
                "int"
                    | "integer"
                    | "string"
                    | "float"
                    | "double"
                    | "real"
                    | "bool"
                    | "boolean"
                    | "true"
                    | "false"
            )
        }),
        TypeKind::Union(members) => {
            !members.is_empty() && members.iter().all(is_object_cast_scalar_type)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_sign_distinguishes_number_pseudo_type_from_number_class() {
        assert_eq!(
            apply_numeric_sign(&PhpType::named(atom("number")), true),
            Some(PhpType::union(vec![PhpType::int(), PhpType::float()]))
        );
        assert_eq!(
            apply_numeric_sign(&PhpType::named(atom("Number")), true),
            None
        );
    }
}
