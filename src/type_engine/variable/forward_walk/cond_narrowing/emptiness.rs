use super::*;

/// Extract the subject of a `count()`/`sizeof()` comparison against an
/// integer literal, plus whether the condition holds exactly when the
/// subject has entries.
///
/// `count($x) > 0`, `count($x) !== 0` and `count($x) >= 1` all prove the
/// subject is non-empty; `count($x) === 0` and `count($x) < 1` prove the
/// opposite, which is what the inverse branch of the same `if` (or the
/// fall-through of `if (count($x) === 0) { throw; }`) reads. A bound that
/// neither proves — `count($x) < 5` says nothing, and `count($x) > -1` is
/// vacuous — yields `None`.
///
/// The second flag says whether the *other* branch proves the opposite.
/// `count($x) > 1` proves the subject non-empty where it holds, but
/// falling through it leaves one entry exactly as possible as none, so
/// nothing may be concluded there.
///
/// The literal may be written on either side, and a leading `!` flips the
/// answer.
pub(super) fn extract_count_emptiness_check(expr: &Expression<'_>) -> Option<(String, bool, bool)> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    let Expression::Binary(bin) = inner else {
        return None;
    };

    // `count($x) OP n`, or `n OP count($x)` with the comparison flipped so
    // the subject is always on the left.
    let (subject, comparison, bound) =
        match (count_call_subject(bin.lhs), count_call_subject(bin.rhs)) {
            (Some(subject), _) => (
                subject,
                Comparison::of(&bin.operator)?,
                count_bound(bin.rhs)?,
            ),
            (_, Some(subject)) => (
                subject,
                Comparison::of(&bin.operator)?.flipped(),
                count_bound(bin.lhs)?,
            ),
            _ => return None,
        };

    // `count()` never returns a negative number, so a bound below zero
    // makes the comparison say nothing about the subject either way.
    let non_empty = match comparison {
        Comparison::Greater if bound >= 0 => true,
        Comparison::GreaterOrEqual if bound >= 1 => true,
        Comparison::NotEqual if bound == 0 => true,
        Comparison::Equal if bound == 0 => false,
        Comparison::Less if bound == 1 => false,
        Comparison::LessOrEqual if bound == 0 => false,
        _ => return None,
    };

    // The comparison splits emptiness in two only when its boundary sits
    // at "has at least one entry".  Every arm that proves emptiness does;
    // of the ones that prove the opposite, only the three that mean
    // `count($x) >= 1` do.
    let complement_exact = !non_empty
        || matches!(
            (comparison, bound),
            (Comparison::Greater, 0) | (Comparison::GreaterOrEqual, 1) | (Comparison::NotEqual, 0)
        );

    Some((subject, non_empty != negated, complement_exact))
}

/// The comparison a `count()` check is written with, reduced to the six
/// orderings so the subject can be moved to the left of it.
#[derive(Clone, Copy)]
enum Comparison {
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Equal,
    NotEqual,
}

impl Comparison {
    fn of(operator: &BinaryOperator<'_>) -> Option<Comparison> {
        Some(match operator {
            BinaryOperator::LessThan(_) => Comparison::Less,
            BinaryOperator::LessThanOrEqual(_) => Comparison::LessOrEqual,
            BinaryOperator::GreaterThan(_) => Comparison::Greater,
            BinaryOperator::GreaterThanOrEqual(_) => Comparison::GreaterOrEqual,
            BinaryOperator::Equal(_) | BinaryOperator::Identical(_) => Comparison::Equal,
            BinaryOperator::NotEqual(_) | BinaryOperator::NotIdentical(_) => Comparison::NotEqual,
            _ => return None,
        })
    }

    /// The comparison that holds when its two operands swap places.
    fn flipped(self) -> Comparison {
        match self {
            Comparison::Less => Comparison::Greater,
            Comparison::LessOrEqual => Comparison::GreaterOrEqual,
            Comparison::Greater => Comparison::Less,
            Comparison::GreaterOrEqual => Comparison::LessOrEqual,
            Comparison::Equal => Comparison::Equal,
            Comparison::NotEqual => Comparison::NotEqual,
        }
    }
}

/// The subject of a `count($x)`/`sizeof($x)` call, as a narrowing key.
fn count_call_subject(expr: &Expression<'_>) -> Option<String> {
    let Expression::Call(Call::Function(call)) = unwrap_parens(expr) else {
        return None;
    };
    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value())).to_ascii_lowercase();
    if name != "count" && name != "sizeof" {
        return None;
    }
    // `count($x, COUNT_RECURSIVE)` still counts the top level's entries,
    // so the second argument does not change what a zero/non-zero result
    // proves about the subject.
    let first = call.argument_list.arguments.first()?;
    let arg = match first {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    expr_to_var_name(arg).or_else(|| narrowing::expr_to_subject_key(arg))
}

/// The bound a `count()` comparison is written against: a plain decimal
/// integer literal, optionally negated. A hexadecimal, octal, or
/// separator-laden literal is not worth decoding for the handful of bounds
/// this reads.
fn count_bound(expr: &Expression<'_>) -> Option<i64> {
    match unwrap_parens(expr) {
        Expression::Literal(Literal::Integer(lit)) => bytes_to_str(lit.raw).parse().ok(),
        Expression::UnaryPrefix(prefix)
            if matches!(prefix.operator, UnaryPrefixOperator::Negation(_)) =>
        {
            count_bound(prefix.operand).map(|value| -value)
        }
        _ => None,
    }
}

/// Apply what a strict comparison against a literal proves about the
/// subject, in whichever direction the branch establishes.
///
/// The equal branch pins the subject to the literal, but only when the
/// type it carries has room for it: a comparison no alternative could
/// satisfy describes a branch that cannot run, which is not this
/// function's business to decide. The unequal branch drops every
/// alternative the literal covers, which is what lets a discriminant
/// (`if ($this->state === 'notLoaded') { … }`) leave its sentinel behind
/// in the branch that ruled it out.
pub(super) fn apply_literal_identity_narrowing(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    let Some((var_name, literal, equal)) = extract_literal_identity_check(condition) else {
        return;
    };
    let equal = equal == truthy;
    seed_synthetic_key_if_needed(&var_name, scope, ctx);
    let types = scope.get(&var_name).to_vec();
    if types.is_empty() {
        return;
    }

    if equal {
        let admits = types.iter().any(|rt| {
            rt.type_string
                .union_members()
                .iter()
                .any(|member| literal.is_subtype_of(member))
        });
        if admits {
            scope.set(&var_name, vec![ResolvedType::from_type_string(literal)]);
        }
        return;
    }

    let kept: Vec<ResolvedType> = types
        .into_iter()
        .filter_map(|mut rt| {
            rt.type_string = strip_literal_from_type(&rt.type_string, &literal)?;
            Some(rt)
        })
        .collect();
    // Nothing is recorded when the subtraction empties the type: the
    // remaining alternatives are what the branch runs on, and "no type at
    // all" is not one of them.
    if !kept.is_empty() {
        scope.set(&var_name, kept);
    }
}

/// Remove every alternative of `ty` that the literal `excluded` covers,
/// returning `None` when that leaves nothing.
///
/// Only alternatives the literal fully accounts for go: `'notLoaded'`
/// drops out of `bool|'notLoaded'|null`, while a bare `string` stays put
/// in `string|null` — ruling out one of its values does not rule out the
/// type.
pub(super) fn strip_literal_from_type(ty: &PhpType, excluded: &PhpType) -> Option<PhpType> {
    if let TypeKind::Union(members) = ty.kind() {
        let kept: Vec<PhpType> = members
            .iter()
            .filter_map(|member| strip_literal_from_type(member, excluded))
            .collect();
        return match kept.len() {
            0 => None,
            1 => kept.into_iter().next(),
            _ => Some(PhpType::union(kept)),
        };
    }
    if let TypeKind::Nullable(inner) = ty.kind() {
        if excluded.is_null() {
            return Some(inner.clone());
        }
        let kept = strip_literal_from_type(inner, excluded)?;
        return Some(PhpType::nullable(kept));
    }
    (!ty.is_subtype_of(excluded)).then(|| ty.clone())
}

/// Apply [`refine_non_empty_in_scope`]'s rule to one `PhpType`, returning
/// `None` when every member was the empty value being ruled out.
pub(super) fn refine_non_empty_type(ty: &PhpType, empty: EmptyValue) -> Option<PhpType> {
    if let TypeKind::Union(members) = ty.kind() {
        let refined: Vec<PhpType> = members
            .iter()
            .filter_map(|member| refine_non_empty_type(member, empty))
            .collect();
        return match refined.len() {
            0 => None,
            1 => refined.into_iter().next(),
            _ => Some(PhpType::union(refined)),
        };
    }

    match empty {
        EmptyValue::String => {
            if ty
                .as_literal()
                .and_then(LiteralValue::string_content)
                .as_deref()
                == Some("")
            {
                return None;
            }
            match ty.kind() {
                TypeKind::Named(name) if name == "string" => {
                    Some(PhpType::named(atom("non-empty-string")))
                }
                _ => Some(ty.clone()),
            }
        }
        EmptyValue::Array => match ty.kind() {
            TypeKind::ArrayShape(entries) if entries.is_empty() => None,
            _ => Some(ty.non_empty_array_form()),
        },
    }
}

/// Apply [`refine_empty_in_scope`]'s rule to one `PhpType`, returning
/// `None` when the type cannot hold the empty value at all.
pub(super) fn refine_empty_type(ty: &PhpType, empty: EmptyValue) -> Option<PhpType> {
    if let TypeKind::Union(members) = ty.kind() {
        let refined: Vec<PhpType> = members
            .iter()
            .filter_map(|member| refine_empty_type(member, empty))
            .collect();
        return match refined.len() {
            0 => None,
            1 => refined.into_iter().next(),
            _ => Some(PhpType::union(refined)),
        };
    }

    match empty {
        EmptyValue::String => {
            if let Some(content) = ty.as_literal().and_then(LiteralValue::string_content) {
                return content.is_empty().then(|| ty.clone());
            }
            match ty.kind() {
                TypeKind::Named(name) if name == "non-empty-string" => None,
                TypeKind::Named(name) if name == "string" => {
                    Some(PhpType::literal_string_value(""))
                }
                _ => Some(ty.clone()),
            }
        }
        EmptyValue::Array => {
            if !ty.is_array_like() {
                return Some(ty.clone());
            }
            if ty.is_provably_non_empty() {
                return None;
            }
            Some(PhpType::array_shape(Vec::new()))
        }
    }
}
