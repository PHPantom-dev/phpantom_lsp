/// Binary-operator result-type inference.
///
/// Shared by [`resolve_rhs_expression`](super::resolve_rhs_expression) and
/// the forward walker's compound-assignment handling (`+=`, `-=`, …), so a
/// fix here reaches every consumer that asks "what type does this operator
/// produce?" rather than being answered twice.
use mago_syntax::cst::binary::{Binary, BinaryOperator};

use crate::php_type::{PhpType, TypeKind, keyword_lowercase};
use crate::type_engine::resolver::VarResolutionCtx;
use crate::type_engine::types::const_fold::{self, BitwiseOp};
use crate::types::ResolvedType;

use super::resolve_rhs_expression;

/// The result type a binary operator produces, or `None` for an operator
/// this module does not classify (concatenation and `??` are handled by
/// their own dedicated callers before this is reached).
pub(super) fn resolve_binary_result_type<'b>(
    binary: &'b Binary<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    // Spaceship (<=>): always int (-1, 0, or 1).
    if matches!(binary.operator, BinaryOperator::Spaceship(_)) {
        return Some(vec![ResolvedType::from_type_string(PhpType::int())]);
    }

    // instanceof, comparison, logical: always bool.
    if binary.operator.is_instanceof()
        || binary.operator.is_comparison()
        || binary.operator.is_logical()
    {
        return Some(vec![ResolvedType::from_type_string(PhpType::bool())]);
    }

    // Modulo (%): always int.
    if matches!(binary.operator, BinaryOperator::Modulo(_)) {
        return Some(vec![ResolvedType::from_type_string(PhpType::int())]);
    }

    // Addition (+): PHP overloads this for array union vs numeric addition.
    if matches!(binary.operator, BinaryOperator::Addition(_)) {
        let lhs_types = resolve_rhs_expression(binary.lhs, ctx);
        let rhs_types = resolve_rhs_expression(binary.rhs, ctx);
        return Some(vec![ResolvedType::from_type_string(
            infer_addition_result_type(&lhs_types, &rhs_types),
        )]);
    }

    // Arithmetic: -, *, /, **.
    if matches!(
        binary.operator,
        BinaryOperator::Subtraction(_)
            | BinaryOperator::Multiplication(_)
            | BinaryOperator::Division(_)
            | BinaryOperator::Exponentiation(_)
    ) {
        let lhs_types = resolve_rhs_expression(binary.lhs, ctx);
        let rhs_types = resolve_rhs_expression(binary.rhs, ctx);
        let op_kind = ArithmeticOpKind::from_binary_operator(&binary.operator);
        return Some(vec![ResolvedType::from_type_string(
            infer_arithmetic_result_type(&lhs_types, &rhs_types, op_kind),
        )]);
    }

    // Bitwise operators (&, |, ^, <<, >>).
    // When both operands are strings, PHP applies bitwise ops
    // character-by-character and returns a string.  Otherwise int — or the
    // exact value, when both operands are known integers.
    if let Some(op) = bitwise_op(&binary.operator) {
        let lhs_types = resolve_rhs_expression(binary.lhs, ctx);
        let rhs_types = resolve_rhs_expression(binary.rhs, ctx);
        // `&`, `|` and `^` are the ones PHP overloads for strings; a shift
        // always produces an int.
        if matches!(op, BitwiseOp::And | BitwiseOp::Or | BitwiseOp::Xor) {
            let both_strings = !lhs_types.is_empty()
                && !rhs_types.is_empty()
                && lhs_types
                    .iter()
                    .all(|rt| rt.type_string.is_subtype_of(&PhpType::string()))
                && rhs_types
                    .iter()
                    .all(|rt| rt.type_string.is_subtype_of(&PhpType::string()));
            if both_strings {
                return Some(vec![ResolvedType::from_type_string(PhpType::string())]);
            }
            // Two operands nobody typed decide nothing: the same operator
            // produces a string from two strings and an int from two
            // numbers, and a `mixed` could be either. The honest answer is
            // the pair — benevolent, because it is a gap in what the code
            // said rather than a value measured to be two things.
            if operand_is_undecided(&lhs_types) && operand_is_undecided(&rhs_types) {
                return Some(vec![ResolvedType::from_type_string(PhpType::benevolent(
                    PhpType::union(vec![PhpType::int(), PhpType::string()]),
                ))]);
            }
        }
        // A mask built from constants (`$flags = JSON_PRETTY_PRINT |
        // JSON_THROW_ON_ERROR`) keeps its value, so a call that is handed it
        // can still read the bits it sets.
        if let Some(value) = single_literal_int(&lhs_types)
            .zip(single_literal_int(&rhs_types))
            .and_then(|(lhs, rhs)| const_fold::apply_bitwise(op, lhs, rhs))
        {
            return Some(vec![ResolvedType::from_type_string(PhpType::literal_int(
                value.to_string(),
            ))]);
        }
        return Some(vec![ResolvedType::from_type_string(PhpType::int())]);
    }

    None
}

/// Classify a resolved operand as `int`, `float`, or unknown for arithmetic
/// type promotion.
///
/// Returns `Some(true)` for float, `Some(false)` for int/bool, `None` when
/// the type is mixed or otherwise ambiguous. Handles unions and nullable
/// types by classifying each member.
pub(crate) fn classify_numeric_operand(types: &[ResolvedType]) -> Option<bool> {
    if types.is_empty() {
        return None;
    }
    let mut saw_float = false;
    let mut saw_int = false;
    for rt in types {
        classify_php_type(&rt.type_string, &mut saw_float, &mut saw_int)?;
    }
    if saw_float && saw_int {
        // Both int-like and float-like members present (e.g. int|float
        // union) — the runtime result could be either, so return None to
        // fall back to the conservative int|float.
        None
    } else if saw_float {
        Some(true)
    } else if saw_int {
        Some(false)
    } else {
        None
    }
}

/// Recursively classify a `PhpType` as int-like or float-like.
///
/// Returns `None` (and short-circuits) if any member is ambiguous (mixed,
/// string, object, etc.). Updates `saw_float` and `saw_int` flags for known
/// numeric members. `null` members are ignored since they coerce to 0 in
/// arithmetic context.
fn classify_php_type(ty: &PhpType, saw_float: &mut bool, saw_int: &mut bool) -> Option<()> {
    // Defer to `is_int_subtype`/`is_float_subtype` so every PHPDoc int
    // refinement (`int<0,max>`, `positive-int`, …) and float spelling is
    // recognised, not just the bare `int`/`float` names.
    if ty.is_float_subtype() {
        *saw_float = true;
        return Some(());
    }
    if ty.is_int_subtype() {
        *saw_int = true;
        return Some(());
    }
    match ty.kind() {
        TypeKind::Named(n) => {
            let lower = keyword_lowercase(n);
            if lower == "bool" || lower == "boolean" || lower == "true" || lower == "false" {
                *saw_int = true;
            } else if lower == "numeric" || n == "number" {
                *saw_int = true;
                *saw_float = true;
            } else if lower == "null" {
                // null coerces to 0 (int) in arithmetic; ignore it so that
                // `int|null` classifies as int-like.
            } else {
                return None; // mixed, string, object, etc.
            }
            Some(())
        }
        TypeKind::Union(members) => {
            for member in members {
                classify_php_type(member, saw_float, saw_int)?;
            }
            Some(())
        }
        TypeKind::Nullable(inner) => {
            // ?T is T|null — classify the inner type, ignore null.
            classify_php_type(inner, saw_float, saw_int)
        }
        _ => None,
    }
}

/// Which arithmetic operator is being inferred, distinguishing the two
/// operators whose `int op int` case can still produce a `float` at
/// runtime from the ones that cannot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArithmeticOpKind {
    /// `/` / `/=`: an uneven division produces a float (e.g. `7 / 2`).
    Division,
    /// `**` / `**=`: overflow or a negative exponent produces a float
    /// (e.g. `2 ** 64`, `2 ** -1`).
    Exponentiation,
    /// `-`, `*` and their compound forms: `int op int` always stays `int`.
    Other,
}

impl ArithmeticOpKind {
    pub(crate) fn from_binary_operator(operator: &BinaryOperator<'_>) -> Self {
        match operator {
            BinaryOperator::Division(_) => Self::Division,
            BinaryOperator::Exponentiation(_) => Self::Exponentiation,
            _ => Self::Other,
        }
    }
}

/// Infer the result type of an arithmetic operation based on operand types,
/// following PHP's numeric type promotion rules.
///
/// - `int op int` → `int` (for `+`, `-`, `*`)
/// - `int op float` or `float op int` → `float`
/// - `float op float` → `float`
/// - `int / int` or `int ** int` → benevolent `int|float` (either can
///   produce a float depending on the operand values)
/// - Anything else → `int|float`
pub(crate) fn infer_arithmetic_result_type(
    lhs_types: &[ResolvedType],
    rhs_types: &[ResolvedType],
    op_kind: ArithmeticOpKind,
) -> PhpType {
    let lhs = classify_numeric_operand(lhs_types);
    let rhs = classify_numeric_operand(rhs_types);
    match (lhs, rhs) {
        // Both are known int (not float): int op int.
        (Some(false), Some(false)) => {
            if op_kind != ArithmeticOpKind::Other {
                // int / int can return float (e.g. 7/2 = 3.5), and
                // int ** int can too (overflow, or a negative exponent, e.g.
                // 2 ** 64 or 2 ** -1) — but in both cases the float half only
                // materialises for particular operand *values*, which no
                // annotation on the operands can rule out. Enforcing the
                // whole union would turn every `$total / $count` or
                // `$base ** $exp` handed to an `int` parameter into a
                // mismatch, so the union is tagged benevolent: one branch
                // satisfying a target is enough. PHPStan resolves both
                // operators to `__benevolent<int|float>` for the same
                // reason.
                //
                // Only this branch is tagged. When an operand is already
                // `int|float` the union is the operand's own, not one the
                // operator invented, and it stays strict.
                PhpType::benevolent(PhpType::union(vec![PhpType::int(), PhpType::float()]))
            } else {
                PhpType::int()
            }
        }
        // At least one float, the other is known: result is float.
        (Some(true), Some(_)) | (Some(_), Some(true)) => PhpType::float(),
        // One or both operands are unknown: fall back to int|float.
        _ => {
            let union = PhpType::union(vec![PhpType::int(), PhpType::float()]);
            // An operand nobody typed decides neither half. The union is
            // the operator's own invention over a gap in what the code
            // said, not a value measured to be two things, so enforcing
            // both halves would turn every `$row['count'] - 1` handed to
            // an `int` parameter into a mismatch. Tag it benevolent —
            // one branch satisfying the target is enough. PHPStan
            // resolves the same operands to `__benevolent<int|float>`.
            //
            // An operand that resolved to a real but non-numeric type
            // (`string`, an object) keeps the strict union: there the
            // code did say something, and PHP's own answer for it is not
            // a number this can vouch for.
            if operand_is_unenforceable(lhs_types) || operand_is_unenforceable(rhs_types) {
                PhpType::benevolent(union)
            } else {
                union
            }
        }
    }
}

/// Infer the result type of `+` / `+=`, which PHP overloads for the array
/// union as well as numeric addition.
///
/// Two arrays union their keys, which
/// [`merge_array_plus`](super::super::resolution::merge_array_plus) works
/// out from whatever both sides know. Only a mix of an array and a number
/// has no meaningful result type: PHP raises a `TypeError` for it, so a bare
/// `array` stands in rather than a number the operation cannot produce.
pub(crate) fn infer_addition_result_type(
    lhs_types: &[ResolvedType],
    rhs_types: &[ResolvedType],
) -> PhpType {
    let lhs_is_array = lhs_types.iter().any(|rt| rt.type_string.is_array_like());
    let rhs_is_array = rhs_types.iter().any(|rt| rt.type_string.is_array_like());
    if rhs_is_array && (lhs_is_array || lhs_types.is_empty()) {
        // An operand with no tracked type still gets everything the array
        // beside it contributes: `+` only accepts arrays, so whatever it
        // held was one too.
        let lhs_type = if lhs_types.is_empty() {
            PhpType::array()
        } else {
            ResolvedType::types_joined(lhs_types)
        };
        return super::super::resolution::merge_array_plus(
            &lhs_type,
            &ResolvedType::types_joined(rhs_types),
        );
    }
    if lhs_is_array || rhs_is_array {
        return PhpType::array();
    }
    infer_arithmetic_result_type(lhs_types, rhs_types, ArithmeticOpKind::Other)
}

/// The bitwise operator `operator` is, or `None` for every other binary
/// operator.
fn bitwise_op(operator: &BinaryOperator<'_>) -> Option<BitwiseOp> {
    Some(match operator {
        BinaryOperator::BitwiseAnd(_) => BitwiseOp::And,
        BinaryOperator::BitwiseOr(_) => BitwiseOp::Or,
        BinaryOperator::BitwiseXor(_) => BitwiseOp::Xor,
        BinaryOperator::LeftShift(_) => BitwiseOp::LeftShift,
        BinaryOperator::RightShift(_) => BitwiseOp::RightShift,
        _ => return None,
    })
}

/// Whether an operand says nothing about which of PHP's two bitwise
/// overloads applies — either it resolved to nothing at all, or every
/// branch it resolved to is `mixed`.
fn operand_is_undecided(types: &[ResolvedType]) -> bool {
    types.is_empty() || types.iter().all(|rt| rt.type_string.is_mixed())
}

/// Whether an operand leaves the arithmetic result unenforceable — either
/// nothing typed it at all ([`operand_is_undecided`]), or it is already a
/// benevolent union that an earlier operator produced for the same reason.
///
/// The second case is what keeps a chain honest: in `strlen($s) + $line - 1`
/// the addition already answered benevolent `int|float`, and re-reading that
/// as a plain two-valued union would make the subtraction strict again and
/// reject the `int` the whole expression is declared to return.
fn operand_is_unenforceable(types: &[ResolvedType]) -> bool {
    operand_is_undecided(types) || types.iter().any(|rt| rt.type_string.is_benevolent())
}

/// The integer an operand holds, when it resolved to exactly one literal
/// integer. An operand that could be several types is not a known value.
fn single_literal_int(types: &[ResolvedType]) -> Option<i64> {
    let [only] = types else {
        return None;
    };
    const_fold::literal_int_value(&only.type_string)
}
