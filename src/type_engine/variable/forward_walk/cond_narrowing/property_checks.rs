use super::*;

/// A check on `subject`'s `property` that a union member may be unable
/// to pass.
pub(super) struct PropertyCheck {
    subject: String,
    property: String,
    test: PropertyTest,
}

enum PropertyTest {
    /// The property passed (or, when `expect_match` is false, failed) a
    /// type guard such as `is_string()`.
    Guard {
        kind: narrowing::TypeGuardKind,
        expect_match: bool,
    },
    /// The property is identical (or, when `expect_equal` is false, not
    /// identical) to an exact value.
    Value {
        value: ExactValue,
        value_type: PhpType,
        expect_equal: bool,
    },
}

/// A value a comparison can pin a property to exactly.  Floats are left
/// out: `===` on them is a trap, and they are never written as a
/// discriminant.
#[derive(Debug, PartialEq)]
pub(super) enum ExactValue {
    Str(String),
    Int(i64),
    Bool(bool),
    Null,
}

impl PropertyTest {
    /// Report whether a member declaring `prop_type` for the property
    /// could have reached the branch this check guards.
    fn admits(
        &self,
        prop_type: &PhpType,
        class_loader: &dyn Fn(&str) -> Option<Arc<crate::types::ClassInfo>>,
    ) -> bool {
        match self {
            PropertyTest::Guard { kind, expect_match } => narrowing::guard_outcome_possible(
                prop_type,
                *kind,
                *expect_match,
                Some(class_loader),
            ),
            PropertyTest::Value {
                value,
                value_type,
                expect_equal: true,
            } => property_can_equal(prop_type, value, value_type),
            PropertyTest::Value {
                value,
                expect_equal: false,
                ..
            } => !exact_value_of_type(prop_type).is_some_and(|own| own == *value),
        }
    }
}

/// Read the check a single condition operand makes about one property.
pub(super) fn extract_property_check(
    operand: &Expression<'_>,
    truthy: bool,
) -> Option<PropertyCheck> {
    let (inner, negated) = narrowing::unwrap_condition_negation(operand);
    // Whether the branch being narrowed is the one where the check held.
    let holds = truthy != negated;

    match inner {
        // `is_string($b->v)` and the other type-guard functions.
        Expression::Call(Call::Function(_)) => {
            let key = collect_condition_property_keys(inner)
                .into_iter()
                .find(|k| k.contains("->"))?;
            let (kind, guard_negated) = narrowing::try_extract_type_guard(inner, &key)?;
            let (subject, property) = split_property_key(&key)?;
            Some(PropertyCheck {
                subject,
                property,
                test: PropertyTest::Guard {
                    kind,
                    expect_match: holds != guard_negated,
                },
            })
        }
        // `$b->v === 'x'` / `$b->v !== 'x'`.
        Expression::Binary(bin) => {
            let identical = match bin.operator {
                BinaryOperator::Identical(_) => true,
                BinaryOperator::NotIdentical(_) => false,
                _ => return None,
            };
            let (key, other) = property_key_operand(bin.lhs, bin.rhs)?;
            let (value, value_type) = exact_value_of_expr(other)?;
            let (subject, property) = split_property_key(&key)?;
            Some(PropertyCheck {
                subject,
                property,
                test: PropertyTest::Value {
                    value,
                    value_type,
                    expect_equal: holds == identical,
                },
            })
        }
        _ => None,
    }
}

/// Pick whichever side of a comparison is a property path, paired with
/// the other side.
fn property_key_operand<'b>(
    lhs: &'b Expression<'b>,
    rhs: &'b Expression<'b>,
) -> Option<(String, &'b Expression<'b>)> {
    for (candidate, other) in [(lhs, rhs), (rhs, lhs)] {
        if let Some(key) = narrowing::expr_to_subject_key(candidate)
            && key.contains("->")
        {
            return Some((key, other));
        }
    }
    None
}

/// Split `$b->v` into its subject (`$b`) and property name (`v`).
/// A call key (`$b->v()`) is not a property and is left out.
fn split_property_key(key: &str) -> Option<(String, String)> {
    let arrow = key.rfind("->")?;
    let property = &key[arrow + 2..];
    if property.is_empty() || !property.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some((key[..arrow].to_string(), property.to_string()))
}

/// Drop the members of the subject's union whose declaration of the
/// property rules the check out.
pub(super) fn narrow_union_by_property_check(
    check: &PropertyCheck,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let entries = scope.get(&check.subject);
    // Nothing to discriminate between below two class-bearing members.
    if entries.iter().filter(|rt| rt.class_info.is_some()).count() < 2 {
        return;
    }

    let mut kept: Vec<ResolvedType> = Vec::with_capacity(entries.len());
    let mut dropped = false;
    for rt in entries {
        // Entries that name no class (a `null` alternative, a scalar the
        // subject may also hold) carry no property to read, so the check
        // says nothing about them.
        let admitted = match rt.class_info.as_ref() {
            Some(cls) => crate::inheritance::resolve_property_type_hint(
                cls,
                &check.property,
                ctx.class_loader,
            )
            .is_none_or(|hint| check.test.admits(&hint, ctx.class_loader)),
            None => true,
        };
        if admitted {
            kept.push(rt.clone());
        } else {
            dropped = true;
        }
    }

    // Keeping nothing would mean the check can never pass — a claim the
    // subject's declared type is more likely wrong about than the code is.
    if dropped && kept.iter().any(|rt| rt.class_info.is_some()) {
        scope.set(&check.subject, kept);
    }
}

/// Report whether a property declared `prop_type` could be identical to
/// the compared value.
fn property_can_equal(prop_type: &PhpType, value: &ExactValue, value_type: &PhpType) -> bool {
    match prop_type.kind() {
        TypeKind::Union(members) => members
            .iter()
            .any(|m| property_can_equal(m, value, value_type)),
        TypeKind::Nullable(inner) => {
            *value == ExactValue::Null || property_can_equal(inner, value, value_type)
        }
        _ => match exact_value_of_type(prop_type) {
            Some(own) => own == *value,
            // Only a scalar declaration is precise enough to rule a value
            // out.  A class, a template parameter, or anything else the
            // subtype check cannot speak for keeps the member.
            None if is_scalar_declaration(prop_type) => value_type.is_subtype_of(prop_type),
            None => true,
        },
    }
}

/// Report whether a type pins its values to one scalar family, so that a
/// value outside it cannot be identical to anything the type holds.
fn is_scalar_declaration(ty: &PhpType) -> bool {
    ty.is_null()
        || ty.is_subtype_of(&PhpType::string())
        || ty.is_subtype_of(&PhpType::int())
        || ty.is_subtype_of(&PhpType::float())
        || ty.is_subtype_of(&PhpType::bool())
}

/// Read the single value a type is pinned to, when it has one.
fn exact_value_of_type(ty: &PhpType) -> Option<ExactValue> {
    if let Some(literal) = ty.as_literal() {
        return match literal {
            crate::php_type::LiteralValue::String(_) => literal
                .string_content()
                .map(|c| ExactValue::Str(c.into_owned())),
            crate::php_type::LiteralValue::Int(_) => literal.parse_i64().map(ExactValue::Int),
            crate::php_type::LiteralValue::Float(_) => None,
        };
    }
    match ty.kind() {
        TypeKind::Named(name) => match name.to_ascii_lowercase().as_str() {
            "true" => Some(ExactValue::Bool(true)),
            "false" => Some(ExactValue::Bool(false)),
            "null" => Some(ExactValue::Null),
            _ => None,
        },
        _ => None,
    }
}

/// Read the value a literal operand compares against, with the type that
/// value has.
pub(super) fn exact_value_of_expr(expr: &Expression<'_>) -> Option<(ExactValue, PhpType)> {
    match expr {
        Expression::Parenthesized(paren) => exact_value_of_expr(paren.expression),
        Expression::Literal(Literal::String(string)) => {
            let raw = bytes_to_str(string.raw);
            let ty = PhpType::literal_string_raw(raw.to_string());
            let value = ty.as_literal()?.string_content()?.into_owned();
            Some((ExactValue::Str(value), ty))
        }
        Expression::Literal(Literal::Integer(integer)) => {
            let value = i64::try_from(integer.value?).ok()?;
            Some((
                ExactValue::Int(value),
                PhpType::literal_int(value.to_string()),
            ))
        }
        Expression::Literal(Literal::True(_)) => Some((ExactValue::Bool(true), PhpType::bool())),
        Expression::Literal(Literal::False(_)) => Some((ExactValue::Bool(false), PhpType::bool())),
        _ if is_null_expr(expr) => Some((ExactValue::Null, PhpType::null())),
        _ => None,
    }
}
