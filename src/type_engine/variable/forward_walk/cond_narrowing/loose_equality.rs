use super::*;

/// Apply what a loose comparison against a literal (`$x == 'one'`,
/// `$x != 3.5`, `$a == []`) proves about the subject, in whichever
/// direction the branch establishes.
///
/// `==` compares across types, so it pins the subject to the literal only
/// where, for the alternative in hand, it means the same as `===`: an
/// integer against an integer, a float against a float, a string against a
/// string that is not numeric (`'01' == '1'` holds, `'a' == 'b'` does not),
/// and an array against `[]`.  Every other alternative stays as it is,
/// since some of its values may compare equal.  A literal alternative is
/// kept or dropped by comparing it directly.
///
/// `true`, `false` and `null` have passes of their own.
pub(super) fn apply_loose_literal_narrowing(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    let (inner, negated) = narrowing::unwrap_condition_negation(condition);
    let Expression::Binary(bin) = inner else {
        return;
    };
    let equal = match bin.operator {
        BinaryOperator::Equal(_) => !negated,
        BinaryOperator::NotEqual(_) => negated,
        _ => return,
    };
    let (subject, literal) = match (loose_comparand_type(bin.rhs), loose_comparand_type(bin.lhs)) {
        (Some(ty), _) => (bin.lhs, ty),
        (_, Some(ty)) => (bin.rhs, ty),
        _ => return,
    };
    let Some(var_name) = expr_to_subject(subject) else {
        return;
    };
    let equal = equal == truthy;
    refine_subject(&var_name, scope, ctx, |ty| {
        if equal {
            loosely_equal_part(ty, std::slice::from_ref(&literal))
        } else {
            loosely_unequal_part(ty, std::slice::from_ref(&literal))
        }
    });
}

/// Apply a non-strict `in_array($x, [...])` check whose haystack holds only
/// literals, by the same rules as `==` against each of them.
///
/// The strict form is [`apply_in_array_narrowing`]'s.
pub(crate) fn apply_loose_in_array_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    inverted: bool,
) {
    let (inner, negated) = narrowing::unwrap_condition_negation(condition);
    let Some((needle, haystack)) = loose_in_array_target(inner) else {
        return;
    };
    let Some(var_name) = expr_to_subject(needle) else {
        return;
    };
    let Some(element) = resolve_in_array_element_type_fw(haystack, scope, ctx) else {
        return;
    };
    let literals: Vec<PhpType> = element.union_members().into_iter().cloned().collect();
    if literals.is_empty() || !literals.iter().all(is_loose_comparand) {
        return;
    }
    let found = !(inverted ^ negated);
    refine_subject(&var_name, scope, ctx, |ty| {
        if found {
            loosely_equal_part(ty, &literals)
        } else {
            loosely_unequal_part(ty, &literals)
        }
    });
}

/// The needle and haystack of an `in_array()` call that compares loosely:
/// two arguments, or a third that is written `false`.
fn loose_in_array_target<'b>(
    expr: &'b Expression<'b>,
) -> Option<(&'b Expression<'b>, &'b Expression<'b>)> {
    let Expression::Call(Call::Function(call)) = unwrap_parens(expr) else {
        return None;
    };
    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    if !crate::util::strip_fqn_prefix(bytes_to_str(ident.value())).eq_ignore_ascii_case("in_array")
    {
        return None;
    }
    let args: Vec<_> = call.argument_list.arguments.iter().collect();
    match args.len() {
        2 => {}
        3 if is_false_expr(unwrap_parens(narrowing::argument_value(args[2]))) => {}
        _ => return None,
    }
    Some((
        narrowing::argument_value(args[0]),
        narrowing::argument_value(args[1]),
    ))
}

/// Replace the scope's type for `var_name` with what `refine` leaves of
/// it, unless that is nothing or no change.
fn refine_subject(
    var_name: &str,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    refine: impl Fn(&PhpType) -> Option<PhpType>,
) {
    seed_synthetic_key_if_needed(var_name, scope, ctx);
    let types = scope.get(var_name);
    if types.is_empty() {
        return;
    }
    let mut changed = false;
    let mut narrowed = Vec::with_capacity(types.len());
    for rt in types {
        match refine(&rt.type_string) {
            Some(ty) if ty == rt.type_string || ty.equivalent(&rt.type_string) => {
                narrowed.push(rt.clone())
            }
            Some(ty) => {
                changed = true;
                narrowed.push(ResolvedType::from_type_string(ty));
            }
            None => changed = true,
        }
    }
    // Nothing left means the branch cannot run with what the subject holds,
    // which is the reachability question rather than this one.
    if changed && !narrowed.is_empty() {
        scope.set(var_name, narrowed);
        write_offset_key_into_shapes(var_name, scope);
    }
}

/// The comparand types a loose comparison is narrowed against: an int,
/// float or string literal, or the empty array.
fn loose_comparand_type(expr: &Expression<'_>) -> Option<PhpType> {
    literal_comparand_type(expr).filter(is_loose_comparand)
}

fn is_loose_comparand(ty: &PhpType) -> bool {
    ty.as_literal().is_some() || is_empty_array(ty)
}

fn is_empty_array(ty: &PhpType) -> bool {
    matches!(ty.kind(), TypeKind::ArrayShape(entries) if entries.is_empty())
}

/// The part of `ty` that can loosely equal one of `literals`, or `None`
/// when no part of it can.
fn loosely_equal_part(ty: &PhpType, literals: &[PhpType]) -> Option<PhpType> {
    let mut kept: Vec<PhpType> = Vec::new();
    for member in expand_nullable(ty) {
        for literal in literals {
            if let Some(part) = loosely_equal_member(&member, literal) {
                kept.push(part);
            }
        }
    }
    match kept.len() {
        0 => None,
        _ => Some(PhpType::join_runtime_value_types(kept)),
    }
}

/// The part of one non-union alternative that can loosely equal `literal`.
fn loosely_equal_member(member: &PhpType, literal: &PhpType) -> Option<PhpType> {
    if is_empty_array(literal) {
        if !member.is_array_like() {
            return Some(member.clone());
        }
        return (!member.is_provably_non_empty()).then(|| literal.clone());
    }
    let literal_value = literal.as_literal()?;
    if member.is_mixed() {
        return mixed_loosely_equal_part(literal, literal_value);
    }
    // `null` compares against a string as `''` and against a number as `0`,
    // so it only equals the literals those are.
    if member.is_null() {
        let equals_null = match literal_value {
            LiteralValue::String(_) => literal_value
                .string_content()
                .is_some_and(|text| text.is_empty()),
            LiteralValue::Int(_) => literal_value.parse_i64() == Some(0),
            LiteralValue::Float(_) => literal_value.parse_f64() == Some(0.0),
        };
        return equals_null.then(|| member.clone());
    }
    if let Some(value) = member.as_literal() {
        return match value.loosely_equals(literal_value) {
            Some(false) => None,
            _ => Some(member.clone()),
        };
    }
    if !loose_means_identity(member, literal_value) {
        // A string compares with a number, or with another string, as a
        // number only when it is numeric itself (`'abc' == 1` is false), so a
        // string equal to a numeric literal is a numeric string, though not
        // necessarily that spelling of it (`' 1' == '1'`).
        let numeric_literal = match literal_value {
            LiteralValue::String(_) => literal_value.is_numeric_string(),
            LiteralValue::Int(_) | LiteralValue::Float(_) => true,
        };
        if numeric_literal && member.is_string_subtype() && member.as_literal().is_none() {
            return Some(PhpType::parse("numeric-string"));
        }
        return Some(member.clone());
    }
    literal.is_subtype_of(member).then(|| literal.clone())
}

/// The values of every type that PHP 8's `==` holds equal to `literal`, for
/// a subject that could be anything.
///
/// A number equals the ints and floats of its value and every numeric
/// string; a non-numeric string equals only itself among strings and no
/// number at all (`0 == ''` is false since PHP 8).  A bool equals the
/// literal when their truthiness agrees, and `null` equals `''` and `0`.
/// Arrays equal no scalar.  An object can: a `Stringable` one compares by
/// its string, and a number-like one (`GMP`, `BcMath\Number`) by its value.
fn mixed_loosely_equal_part(literal: &PhpType, value: &LiteralValue) -> Option<PhpType> {
    let number = match value {
        LiteralValue::Int(_) => value.parse_i64().map(|v| v as f64),
        LiteralValue::Float(_) => value.parse_f64(),
        LiteralValue::String(_) => value
            .numeric_string_value()
            .and_then(|n| n.parse_i64().map(|v| v as f64).or_else(|| n.parse_f64())),
    };
    let mut parts: Vec<PhpType> = Vec::new();
    match number {
        Some(number) => {
            if number.fract() == 0.0 && number.abs() < i64::MAX as f64 {
                parts.push(PhpType::literal_int((number as i64).to_string()));
            }
            parts.push(PhpType::literal_float(format!("{number:?}")));
            parts.push(PhpType::parse("numeric-string"));
        }
        None => {
            // A float's string form is numeric except for `INF`, `-INF` and
            // `NAN`, which a non-numeric string can spell.
            let content = value.string_content().unwrap_or_default();
            if matches!(content.as_ref(), "INF" | "-INF" | "NAN") {
                parts.push(PhpType::float());
            }
            parts.push(literal.clone());
        }
    }
    parts.push(if literal.truthiness() == Some(true) {
        PhpType::true_()
    } else {
        PhpType::false_()
    });
    if let Some(null) = loosely_equal_member(&PhpType::null(), literal) {
        parts.push(null);
    }
    parts.push(if number.is_some() {
        PhpType::object()
    } else {
        PhpType::named(atom("Stringable"))
    });
    Some(PhpType::join_runtime_value_types(parts))
}

/// The part of `ty` that cannot loosely equal any of `literals`, or `None`
/// when nothing is left.
fn loosely_unequal_part(ty: &PhpType, literals: &[PhpType]) -> Option<PhpType> {
    let mut kept: Vec<PhpType> = Vec::new();
    for member in expand_nullable(ty) {
        let mut part = Some(member);
        for literal in literals {
            part = part.and_then(|m| loosely_unequal_member(&m, literal));
        }
        kept.extend(part);
    }
    match kept.len() {
        0 => None,
        1 => kept.pop(),
        _ => Some(PhpType::union(kept)),
    }
}

/// The part of one non-union alternative that cannot loosely equal
/// `literal`.  Ruling out one value of a type that holds others leaves
/// the type, except that an array unequal to `[]` has entries.
fn loosely_unequal_member(member: &PhpType, literal: &PhpType) -> Option<PhpType> {
    if is_empty_array(literal) {
        if !member.is_array_like() {
            return Some(member.clone());
        }
        if is_empty_array(member) {
            return None;
        }
        return Some(member.non_empty_array_form());
    }
    let literal_value = literal.as_literal()?;
    match member.as_literal() {
        Some(value) if value.loosely_equals(literal_value) == Some(true) => None,
        _ => Some(member.clone()),
    }
}

/// Whether `==` against `literal` holds for a value of `member` exactly
/// when `===` does.
fn loose_means_identity(member: &PhpType, literal: &LiteralValue) -> bool {
    match literal {
        LiteralValue::Int(_) => member.is_int_subtype(),
        LiteralValue::Float(_) => member.is_float(),
        LiteralValue::String(_) => !literal.is_numeric_string() && member.is_string_subtype(),
    }
}

/// The alternatives of `ty`, with a nullable wrapper split into its inner
/// type and `null`.
fn expand_nullable(ty: &PhpType) -> Vec<PhpType> {
    let mut out = Vec::new();
    for member in ty.union_members() {
        match member.kind() {
            TypeKind::Nullable(inner) => {
                out.extend(inner.union_members().into_iter().cloned());
                out.push(PhpType::null());
            }
            _ => out.push(member.clone()),
        }
    }
    out
}
