use super::*;

/// The subject key an expression names: a plain variable, or the member
/// path or array offset that stands in for one.
///
/// `$x`, `$this->handle` and `$row['name']` are all subjects a condition
/// can prove something about, so every extractor in this module reads its
/// operands through here.
pub(crate) fn expr_to_subject(expr: &Expression<'_>) -> Option<String> {
    expr_to_var_name(expr).or_else(|| narrowing::expr_to_subject_key(expr))
}

/// Which side of an equality a branch is narrowing.
#[derive(Clone, Copy)]
enum Sentinel {
    /// The subject *is* the sentinel, as written: `$x === null`.
    Is,
    /// The subject is *not* the sentinel: `$x !== null`, or a negated
    /// `=== null`.
    IsNot,
}

/// The subject of a comparison against a sentinel value (`null`, `false`).
///
/// The two polarities are deliberately not mirror images. `IsNot` also
/// reads the negation of the equality form (`!($x === null)` proves the
/// same as `$x !== null`), while `Is` reads only the equality as written:
/// its complement is already covered by `IsNot`, and the extractors are
/// called in pairs on the same condition.
fn extract_sentinel_check(
    expr: &Expression<'_>,
    sentinel: Sentinel,
    is_sentinel: fn(&Expression<'_>) -> bool,
) -> Option<String> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    let Expression::Binary(bin) = inner else {
        return None;
    };
    let equal = matches!(
        bin.operator,
        BinaryOperator::Identical(_) | BinaryOperator::Equal(_)
    );
    let not_equal = matches!(
        bin.operator,
        BinaryOperator::NotIdentical(_) | BinaryOperator::NotEqual(_)
    );
    let wanted = match sentinel {
        Sentinel::Is => equal && !negated,
        Sentinel::IsNot => (not_equal && !negated) || (equal && negated),
    };
    if !wanted {
        return None;
    }
    if is_sentinel(bin.rhs) {
        return expr_to_subject(bin.lhs);
    }
    if is_sentinel(bin.lhs) {
        return expr_to_subject(bin.rhs);
    }
    None
}

/// Extract the subjects of an `isset(…)` call, `wanted` saying whether the
/// call read is the negated one (`!isset($x)`) or the plain one.
///
/// Handles simple variables (`$x`) and property/array access keys
/// (`$obj->prop`, `$arr["key"]`). Returns an empty vec when the expression
/// is not an `isset()` call of the wanted polarity.
fn extract_isset_subjects(expr: &Expression<'_>, wanted_negated: bool) -> Vec<String> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    if negated != wanted_negated {
        return vec![];
    }
    // `isset()` is a language construct, parsed as Expression::Construct(Construct::Isset).
    let Expression::Construct(Construct::Isset(isset)) = inner else {
        return vec![];
    };
    isset
        .values
        .iter()
        .filter_map(|value| expr_to_subject(value))
        .collect()
}

/// Extract variable name from `$x !== null` or `null !== $x` patterns.
pub(crate) fn extract_non_null_check_var(expr: &Expression<'_>) -> Option<String> {
    extract_sentinel_check(expr, Sentinel::IsNot, is_null_expr)
}

/// Extract all variable names from an `isset(…)` call (non-negated).
pub(crate) fn extract_isset_vars(expr: &Expression<'_>) -> Vec<String> {
    extract_isset_subjects(expr, false)
}

/// Extract all variable names from a `!isset(…)` call (negated isset).
pub(crate) fn extract_not_isset_vars(expr: &Expression<'_>) -> Vec<String> {
    extract_isset_subjects(expr, true)
}

/// Extract variable name from `$x === null` or `null === $x` patterns.
pub(crate) fn extract_null_equality_check_var(expr: &Expression<'_>) -> Option<String> {
    extract_sentinel_check(expr, Sentinel::Is, is_null_expr)
}

/// Extract the subject of an identity comparison against a class
/// constant, paired with the constant expression itself.
///
/// `proves_equal` selects which polarity the caller is narrowing: `true`
/// for the branch where the subject *is* the constant (the truthy side of
/// `$x === C`, the guard fall-through of `$x !== C`), `false` for the
/// branch where it is not.
///
/// An enum case is the case that matters — `$land === Land::Be` is how
/// enum code is written — but every class constant carries the same
/// proof, so the constant's own type decides what the comparison rules
/// out rather than the syntax.
pub(super) fn extract_class_constant_identity<'b>(
    expr: &'b Expression<'b>,
    proves_equal: bool,
) -> Option<(String, &'b Expression<'b>)> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    let Expression::Binary(bin) = inner else {
        return None;
    };
    let identical = match bin.operator {
        BinaryOperator::Identical(_) => true,
        BinaryOperator::NotIdentical(_) => false,
        _ => return None,
    };
    // Whether "subject is the constant" is what holds when the written
    // condition is true; the caller says which of the two branches it is
    // narrowing.
    if (identical != negated) != proves_equal {
        return None;
    }
    for (candidate, other) in [(bin.rhs, bin.lhs), (bin.lhs, bin.rhs)] {
        if !matches!(candidate, Expression::Access(Access::ClassConstant(_))) {
            continue;
        }
        if let Some(name) = expr_to_subject(other) {
            return Some((name, candidate));
        }
    }
    None
}

/// Extract variable name from `!empty($x)` (negated empty check).
///
/// A member path or an array offset is as much a subject as a bare variable,
/// the same way it is for `isset($x)` and `empty($x)`: `!empty($row['name'])`
/// proves that entry is there and truthy.
pub(crate) fn extract_not_empty_var(expr: &Expression<'_>) -> Option<String> {
    if let Expression::UnaryPrefix(prefix) = expr
        && prefix.operator.is_not()
        && let Expression::Construct(Construct::Empty(empty)) = prefix.operand
    {
        return expr_to_subject(empty.value);
    }
    None
}

/// Extract the subject of a falsy check: `!$x`, `empty($x)`.
///
/// A member path is as much a subject here as a bare variable is, so
/// `!$this->handle` names `$this->handle` — the guard-clause idiom
/// (`if (!$this->handle) { throw; }`) proves the same thing about a
/// property that it does about a local.
pub(crate) fn extract_falsy_check_var(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            expr_to_subject(prefix.operand)
        }
        // `empty($x)` — language construct, parsed as Expression::Construct(Construct::Empty).
        Expression::Construct(Construct::Empty(empty)) => expr_to_subject(empty.value),
        _ => None,
    }
}

/// Extract variable name from `$x === false` or `false === $x` patterns.
///
/// Mirrors [`extract_null_equality_check_var`] but for `false` — needed
/// for the common "resource-like handle" idiom (`finfo_open()`,
/// `pg_connect()`, …) that returns `T|false` and is guarded with a
/// strict equality check rather than `!$x`/`empty($x)`.
pub(crate) fn extract_false_equality_check_var(expr: &Expression<'_>) -> Option<String> {
    extract_sentinel_check(expr, Sentinel::Is, is_false_expr)
}

/// Extract variable name from `$x !== false` or `false !== $x` patterns.
///
/// Mirrors [`extract_non_null_check_var`] but for `false`, which is what
/// the truthy branch of an `if`/`while` guarding a `T|false` return has
/// ruled out. The loose form (`$x != false`) rules out every falsy value,
/// so treating it as `false` alone is a subset of what it proves.
pub(crate) fn extract_non_false_check_var(expr: &Expression<'_>) -> Option<String> {
    extract_sentinel_check(expr, Sentinel::IsNot, is_false_expr)
}

/// The empty value a condition compares a subject against.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmptyValue {
    String,
    Array,
}

/// Extract the subject of a strict comparison against an empty literal:
/// `$x !== ''`, `'' === $x`, `$x !== []`, and their negations.
///
/// Returns the subject key plus which empty value it was compared to, and
/// whether the comparison proves the subject is non-empty (`true`) or empty
/// (`false`).
///
/// Only the strict operators are recognised. `$x != ''` also rules out
/// `null`, and PHP 8 changed how `0 == ''` compares, so the loose form does
/// not map onto a single refinement.
pub(crate) fn extract_empty_value_check(
    expr: &Expression<'_>,
) -> Option<(String, EmptyValue, bool)> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    let Expression::Binary(bin) = inner else {
        return None;
    };
    let non_empty = match bin.operator {
        BinaryOperator::NotIdentical(_) => !negated,
        BinaryOperator::Identical(_) => negated,
        _ => return None,
    };
    let (subject, empty) = match (empty_literal_kind(bin.rhs), empty_literal_kind(bin.lhs)) {
        (Some(kind), _) => (bin.lhs, kind),
        (_, Some(kind)) => (bin.rhs, kind),
        _ => return None,
    };
    let name = expr_to_subject(subject)?;
    Some((name, empty, non_empty))
}

/// Extract the subject of a strict comparison against a literal value:
/// `$x === 0`, `0.0 === $x`, `$x !== []`, and their negations.
///
/// Returns the subject key, the literal's own type, and whether the
/// branch being narrowed is the one where the two were equal.
///
/// Only `===`/`!==` are read. The loose operators compare across types
/// (`0 == ''` changed meaning in PHP 8), so they do not pin the subject
/// to the literal's type the way strict identity does.
pub(crate) fn extract_literal_identity_check(
    expr: &Expression<'_>,
) -> Option<(String, PhpType, bool)> {
    let (inner, negated) = narrowing::unwrap_condition_negation(expr);
    let Expression::Binary(bin) = inner else {
        return None;
    };
    let equal = match bin.operator {
        BinaryOperator::Identical(_) => !negated,
        BinaryOperator::NotIdentical(_) => negated,
        _ => return None,
    };
    let (subject, literal) = match (
        literal_comparand_type(bin.rhs),
        literal_comparand_type(bin.lhs),
    ) {
        (Some(ty), _) => (bin.lhs, ty),
        (_, Some(ty)) => (bin.rhs, ty),
        _ => return None,
    };
    let name = expr_to_subject(subject)?;
    Some((name, literal, equal))
}

/// The type of the literal an expression writes, for the comparands that
/// pin a subject to one exact value.
///
/// A `-1` is a unary minus over a literal rather than a literal of its
/// own, so the sign is folded back in; anything else that is not written
/// out as a value in the source has no literal type.
fn literal_comparand_type(expr: &Expression<'_>) -> Option<PhpType> {
    match expr {
        Expression::Parenthesized(paren) => literal_comparand_type(paren.expression),
        Expression::UnaryPrefix(prefix) => {
            let negated = match prefix.operator {
                UnaryPrefixOperator::Negation(_) => true,
                UnaryPrefixOperator::Plus(_) => false,
                _ => return None,
            };
            let inner = literal_comparand_type(prefix.operand)?;
            if !negated {
                return Some(inner);
            }
            match inner.as_literal()? {
                LiteralValue::Int(raw) => Some(PhpType::literal_int(format!("-{raw}"))),
                LiteralValue::Float(raw) => Some(PhpType::literal_float(format!("-{raw}"))),
                _ => None,
            }
        }
        Expression::Literal(Literal::Integer(int)) => int
            .value
            .map(|value| PhpType::literal_int(value.to_string())),
        Expression::Literal(Literal::Float(float)) => {
            Some(PhpType::literal_float(float.value.into_inner().to_string()))
        }
        Expression::Literal(Literal::String(string)) => string
            .value
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .map(PhpType::literal_string_value),
        Expression::Literal(Literal::True(_)) => Some(PhpType::true_()),
        Expression::Literal(Literal::False(_)) => Some(PhpType::false_()),
        _ if is_null_expr(expr) => Some(PhpType::null()),
        Expression::Array(array) => array.elements.is_empty().then(|| PhpType::parse("array{}")),
        Expression::LegacyArray(array) => {
            array.elements.is_empty().then(|| PhpType::parse("array{}"))
        }
        _ => None,
    }
}

/// Which empty literal an expression is, if any: `''`/`""` or `[]`/`array()`.
fn empty_literal_kind(expr: &Expression<'_>) -> Option<EmptyValue> {
    match expr {
        Expression::Parenthesized(paren) => empty_literal_kind(paren.expression),
        Expression::Literal(Literal::String(s)) => s
            .value
            .is_some_and(|value| value.is_empty())
            .then_some(EmptyValue::String),
        Expression::Array(array) => array.elements.is_empty().then_some(EmptyValue::Array),
        Expression::LegacyArray(array) => array.elements.is_empty().then_some(EmptyValue::Array),
        _ => None,
    }
}

/// Check if an expression is the `false` literal.
pub(crate) fn is_false_expr(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Literal(Literal::False(_)))
}

/// Check if an expression is `null`.
pub(crate) fn is_null_expr(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Literal(Literal::Null(_)) => true,
        Expression::ConstantAccess(ca) => {
            let name = ca.name.value();
            let clean = crate::util::strip_fqn_prefix(bytes_to_str(name));
            clean.eq_ignore_ascii_case("null")
        }
        _ => false,
    }
}

/// Extract a direct variable name from an expression.
///
/// An assignment stands for the variable it wrote, so the
/// assign-and-check idiom (`while (($line = fgets($h)) !== false)`,
/// `if ($row = next())`) resolves to `$line`/`$row` — the subject the
/// surrounding check narrows.  Parentheses are peeled on the way.
pub(crate) fn expr_to_var_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => Some(bytes_to_str(dv.name).to_string()),
        Expression::Parenthesized(paren) => expr_to_var_name(paren.expression),
        Expression::Assignment(assignment) if assignment.operator.is_assign() => {
            expr_to_var_name(assignment.lhs)
        }
        _ => None,
    }
}
