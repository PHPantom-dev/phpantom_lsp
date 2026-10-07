use super::*;

/// Extract the subject of a `count()`/`sizeof()` or `strlen()` comparison
/// against an integer literal, which empty value the call measures, and
/// whether the condition holds exactly when the subject is not that value.
///
/// `count($x) > 0`, `count($x) !== 0` and `count($x) >= 1` all prove the
/// subject is non-empty; `count($x) === 0` and `count($x) < 1` prove the
/// opposite, which is what the inverse branch of the same `if` (or the
/// fall-through of `if (count($x) === 0) { throw; }`) reads. A bound that
/// neither proves — `count($x) < 5` says nothing, and `count($x) > -1` is
/// vacuous — yields `None`.  A length says the same of a string: zero is
/// `''`, anything more is a `non-empty-string`.
///
/// The last flag says whether the *other* branch proves the opposite.
/// `count($x) > 1` proves the subject non-empty where it holds, but
/// falling through it leaves one entry exactly as possible as none, so
/// nothing may be concluded there.
///
/// The literal may be written on either side, and a leading `!` flips the
/// answer.
pub(super) fn extract_count_emptiness_check(
    expr: &Expression<'_>,
) -> Option<(String, EmptyValue, bool, bool)> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    let Expression::Binary(bin) = inner else {
        return None;
    };

    // `count($x) OP n`, or `n OP count($x)` with the comparison flipped so
    // the subject is always on the left.
    let ((subject, empty), comparison, bound) =
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

    // Neither call returns a negative number, so a bound below zero
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

    Some((subject, empty, non_empty != negated, complement_exact))
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

    /// Whether the comparison holds for operands that order this way.
    fn admits(self, ordering: std::cmp::Ordering) -> bool {
        use std::cmp::Ordering;
        match self {
            Comparison::Less => ordering == Ordering::Less,
            Comparison::LessOrEqual => ordering != Ordering::Greater,
            Comparison::Greater => ordering == Ordering::Greater,
            Comparison::GreaterOrEqual => ordering != Ordering::Less,
            Comparison::Equal => ordering == Ordering::Equal,
            Comparison::NotEqual => ordering != Ordering::Equal,
        }
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

/// The subject of a `count($x)`/`sizeof($x)` or `strlen($x)`/
/// `mb_strlen($x)` call, as a narrowing key, with the empty value the call
/// returns zero for.
fn count_call_subject(expr: &Expression<'_>) -> Option<(String, EmptyValue)> {
    let Expression::Call(Call::Function(call)) = unwrap_parens(expr) else {
        return None;
    };
    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    let name = crate::ci_map::fold(crate::util::strip_fqn_prefix(bytes_to_str(ident.value())));
    let empty = match name.as_ref() {
        "count" | "sizeof" => EmptyValue::Array,
        "strlen" | "mb_strlen" => EmptyValue::String,
        _ => return None,
    };
    // `count($x, COUNT_RECURSIVE)` still counts the top level's entries,
    // and `mb_strlen($x, $encoding)` is zero only for `''` whatever the
    // encoding, so a second argument does not change what a zero/non-zero
    // result proves about the subject.
    let first = call.argument_list.arguments.first()?;
    let arg = match first {
        Argument::Positional(pos) => pos.value,
        Argument::Named(named) => named.value,
    };
    Some((expr_to_subject(arg)?, empty))
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
/// in the branch that ruled it out, and exhausts a subject that held
/// nothing else.
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
            write_offset_key_into_shapes(&var_name, scope);
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
    if kept.is_empty() {
        mark_exhausted(&var_name, scope);
    } else {
        scope.set(&var_name, kept);
        write_offset_key_into_shapes(&var_name, scope);
    }
}

/// Rebuild a union from what a per-member refinement leaves of it.
///
/// Every refinement below distributes over a union this way: `None` when
/// no member survives, the member itself when exactly one does. Each one
/// therefore calls this on its way in and then only has to answer for a
/// single type.
fn refine_union_members(
    members: &[PhpType],
    refine: impl Fn(&PhpType) -> Option<PhpType>,
) -> Option<PhpType> {
    let refined: Vec<PhpType> = members.iter().filter_map(refine).collect();
    match refined.len() {
        0 => None,
        1 => refined.into_iter().next(),
        _ => Some(PhpType::union(refined)),
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
        return refine_union_members(members, |member| strip_literal_from_type(member, excluded));
    }
    if let TypeKind::Nullable(inner) = ty.kind() {
        if excluded.is_null() {
            return Some(inner.clone());
        }
        let kept = strip_literal_from_type(inner, excluded)?;
        return Some(PhpType::nullable(kept));
    }
    if let Some(kept) = strip_array_literal_from_shape(ty, excluded) {
        return kept;
    }
    (!ty.is_subtype_of(excluded)).then(|| ty.clone())
}

/// Rule the array literal `excluded` out of the sealed shape `ty`.
///
/// A shape holds every combination of the values its entries allow, and
/// taking one combination away leaves a shape only when the others agree
/// with it in every entry but one: that entry then loses the literal's
/// value, which is what `$arr !== [null]` proves about an `array{?string}`.
/// When two entries can each differ from the literal, what is left is no
/// longer one shape (`array{int, ?string}` minus `[1, null]` still holds
/// `[2, null]`), so the shape is kept whole.
///
/// The outer `None` means the rule does not apply: either side is not a
/// sealed shape, `ty` has an optional entry, `excluded` is not one value, or
/// the keys differ (`===` compares their order too). The inner `None` means
/// nothing is left.
fn strip_array_literal_from_shape(ty: &PhpType, excluded: &PhpType) -> Option<Option<PhpType>> {
    let (TypeKind::ArrayShape(entries), TypeKind::ArrayShape(literal)) =
        (ty.kind(), excluded.kind())
    else {
        return None;
    };
    if entries.iter().any(|entry| entry.optional) || !is_single_value(excluded) {
        return None;
    }
    if crate::php_type::runtime_shape_keys(entries)?
        != crate::php_type::runtime_shape_keys(literal)?
    {
        return None;
    }
    let mut differing = entries
        .iter()
        .zip(literal.iter())
        .enumerate()
        .filter(|(_, (entry, value))| !entry.value_type.is_subtype_of(&value.value_type));
    let Some((index, (entry, value))) = differing.next() else {
        // Every entry already holds the literal's value.
        return Some(None);
    };
    if differing.next().is_some() {
        return Some(Some(ty.clone()));
    }
    let Some(kept) = strip_literal_from_type(&entry.value_type, &value.value_type) else {
        return Some(None);
    };
    let mut narrowed = entries.to_vec();
    narrowed[index].value_type = kept;
    let shape = PhpType::array_shape(narrowed);
    Some(Some(if ty.is_list_shape() {
        PhpType::as_list_shape(shape)
    } else {
        shape
    }))
}

/// Whether `ty` holds exactly one value: a scalar literal, `null`, `true`,
/// `false`, or a sealed shape whose every entry is one of those.
pub(super) fn is_single_value(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Literal(_) => true,
        TypeKind::Named(name) => ["null", "true", "false"]
            .iter()
            .any(|keyword| name.eq_ignore_ascii_case(keyword)),
        TypeKind::ArrayShape(entries) => entries
            .iter()
            .all(|entry| !entry.optional && is_single_value(&entry.value_type)),
        _ => false,
    }
}

/// Narrow a `switch` subject to what one of its arms can see.
///
/// A `case` compares with `==`, so an arm entered through the labels in
/// `matched` keeps only the literal alternatives one of them loosely
/// equals, and the `default` arm (`matched` empty) drops the ones any label
/// in `excluded` equals.  `"foo"|"bar"` is `'foo'` under `case "foo":`.
///
/// Alternatives that are not literals stay: a `string` holds values no
/// label mentions, and whether `null` or a `bool` equals a label is a
/// question this does not try to answer.  A `matched` label that is not a
/// literal could be any value, so it leaves the subject alone.
pub(crate) fn apply_switch_arm_narrowing(
    subject: &Expression<'_>,
    matched: &[&Expression<'_>],
    excluded: &[&Expression<'_>],
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let Some(var_name) = expr_to_subject(subject) else {
        return;
    };
    let literals = |labels: &[&Expression<'_>]| -> Vec<PhpType> {
        labels
            .iter()
            .filter_map(|label| literal_comparand_type(label))
            .filter(|ty| ty.as_literal().is_some())
            .collect()
    };
    let (labels, keep_equal) = if matched.is_empty() {
        (literals(excluded), false)
    } else {
        let labels = literals(matched);
        if labels.len() != matched.len() {
            return;
        }
        (labels, true)
    };
    if labels.is_empty() {
        return;
    }
    let keep = |member: &LiteralValue| {
        let equal = labels.iter().filter_map(PhpType::as_literal).any(|label| {
            // An unreadable value might be equal, and might not be.
            member.loosely_equals(label).unwrap_or(keep_equal)
        });
        equal == keep_equal
    };

    // `switch ($x['type'])` discriminates the shapes `$x` can be, which is
    // only visible once the offset has a type of its own to narrow.
    if narrowing::split_trailing_bracket(&var_name).is_some() {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
    }
    retain_literals_in_scope(&var_name, scope, &keep);
}

/// Narrow a subject whose alternatives include number literals by an
/// ordering comparison against a written-out number.
///
/// `0.0|1.0` is `1.0` inside `if ($x > 0)` and `0.0` past it: each literal
/// either passes the comparison or does not.  Alternatives that are not
/// number literals stay, since a comparison rules out only some of the
/// values a `float` or an `int` can hold.
pub(super) fn apply_literal_order_narrowing(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    let (inner, negated) = narrowing::unwrap_condition_negation(condition);
    let Expression::Binary(bin) = inner else {
        return;
    };
    let Some(comparison) = Comparison::of(&bin.operator) else {
        return;
    };
    if matches!(comparison, Comparison::Equal | Comparison::NotEqual) {
        return;
    }
    let number_literal = |expr: &Expression<'_>| {
        literal_comparand_type(expr).filter(|ty| {
            matches!(
                ty.as_literal(),
                Some(LiteralValue::Int(_) | LiteralValue::Float(_))
            )
        })
    };
    let (subject, comparison, bound) = match (number_literal(bin.rhs), number_literal(bin.lhs)) {
        (Some(bound), _) => (bin.lhs, comparison, bound),
        (_, Some(bound)) => (bin.rhs, comparison.flipped(), bound),
        _ => return,
    };
    let Some(var_name) = expr_to_subject(subject) else {
        return;
    };
    let Some(bound) = bound.as_literal() else {
        return;
    };
    let holds = truthy != negated;
    let keep = |member: &LiteralValue| {
        let Some(ordering) = compare_numbers(member, bound) else {
            return true;
        };
        comparison.admits(ordering) == holds
    };
    seed_synthetic_key_if_needed(&var_name, scope, ctx);
    retain_literals_in_scope(&var_name, scope, &keep);
}

/// How two number literals order, comparing two integers exactly and
/// anything else as floats.  `None` when either is not a number.
fn compare_numbers(a: &LiteralValue, b: &LiteralValue) -> Option<std::cmp::Ordering> {
    if let (Some(a), Some(b)) = (a.parse_i64(), b.parse_i64()) {
        return Some(a.cmp(&b));
    }
    let as_float = |value: &LiteralValue| match value {
        LiteralValue::Int(_) => value.parse_i64().map(|n| n as f64),
        LiteralValue::Float(_) => value.parse_f64(),
        LiteralValue::String(_) => None,
    };
    as_float(a)?.partial_cmp(&as_float(b)?)
}

/// Drop the literal alternatives `keep` rejects from a subject's types,
/// exhausting it when nothing is left.
fn retain_literals_in_scope(
    var_name: &str,
    scope: &mut ScopeState,
    keep: &impl Fn(&LiteralValue) -> bool,
) {
    let types = scope.get(var_name);
    let mut changed = false;
    let mut narrowed = Vec::with_capacity(types.len());
    for rt in types {
        match filter_literal_members(&rt.type_string, keep) {
            Some(ty) if ty == rt.type_string => narrowed.push(rt.clone()),
            Some(ty) => {
                changed = true;
                narrowed.push(ResolvedType {
                    type_string: ty,
                    ..rt.clone()
                });
            }
            None => changed = true,
        }
    }
    if !changed {
        return;
    }
    // Nothing left means the branch cannot run with what the subject holds.
    if narrowed.is_empty() {
        mark_exhausted(var_name, scope);
    } else {
        scope.set(var_name, narrowed);
        write_offset_key_into_shapes(var_name, scope);
    }
}

/// `ty` with the literal alternatives `keep` rejects removed, or `None`
/// when that leaves nothing.
fn filter_literal_members(ty: &PhpType, keep: &impl Fn(&LiteralValue) -> bool) -> Option<PhpType> {
    match ty.kind() {
        TypeKind::Union(members) => {
            refine_union_members(members, |member| filter_literal_members(member, keep))
        }
        TypeKind::Nullable(inner) => Some(match filter_literal_members(inner, keep) {
            Some(kept) => PhpType::nullable(kept),
            None => PhpType::null(),
        }),
        _ => match ty.as_literal() {
            Some(literal) if !keep(literal) => None,
            _ => Some(ty.clone()),
        },
    }
}

/// Apply [`refine_non_empty_in_scope`]'s rule to one `PhpType`, returning
/// `None` when every member was the empty value being ruled out.
pub(super) fn refine_non_empty_type(ty: &PhpType, empty: EmptyValue) -> Option<PhpType> {
    if let TypeKind::Union(members) = ty.kind() {
        return refine_union_members(members, |member| refine_non_empty_type(member, empty));
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
        return refine_union_members(members, |member| refine_empty_type(member, empty));
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
