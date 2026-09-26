//! Folding a constant expression to the value PHP would compute for it.
//!
//! A flags argument, a `match` subject, and a comparison against a constant
//! all need the constant's *value*, not just its type. PHP lets a constant be
//! defined in terms of other constants, so the value is often one level of
//! indirection away from the initialiser text:
//!
//! ```php
//! const FLAGS = JSON_THROW_ON_ERROR;
//! const COMBO = JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR;
//! ```
//!
//! Reading the initialiser as text stops at the first name. This module folds
//! the rest: names are resolved through the shared resolver the caller hands
//! in, and the bitwise and integer arithmetic operators PHP allows in a
//! constant expression are applied to the integers they come back as. A term that cannot be pinned
//! down leaves the whole expression unfolded, so a mask nobody can read stays
//! unread rather than becoming a wrong number.
//!
//! Names are resolved in the scope of the *reading* file, not the file the
//! constant was declared in, because that is all the shared text resolver has
//! to go on. An unqualified `Flags::BITS` written behind a `use` in another
//! file therefore folds only when the same name resolves the same way here;
//! otherwise it stays unfolded.

use std::cell::RefCell;

use crate::php_type::{LiteralValue, PhpType, TypeKind, parse_php_int_literal};

/// Resolver from an expression's source text to its type, as
/// [`crate::type_engine::types::conditional::ArgTypeResolver`] provides.
type TextResolver<'a> = &'a dyn Fn(&str) -> Option<PhpType>;

/// A PHP bitwise operator, as far as folding two integers with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BitwiseOp {
    And,
    Or,
    Xor,
    LeftShift,
    RightShift,
}

/// The integer `lhs op rhs` produces, or `None` when PHP would not produce one
/// (a negative shift count raises an `ArithmeticError`).
///
/// A shift count of 64 or more is not undefined in PHP the way it is in Rust:
/// the bits are all shifted out, leaving `0`, or `-1` for a negative value
/// shifted right (the sign bit is what is left).
pub(crate) fn apply_bitwise(op: BitwiseOp, lhs: i64, rhs: i64) -> Option<i64> {
    Some(match op {
        BitwiseOp::And => lhs & rhs,
        BitwiseOp::Or => lhs | rhs,
        BitwiseOp::Xor => lhs ^ rhs,
        BitwiseOp::LeftShift if rhs < 0 => return None,
        BitwiseOp::LeftShift if rhs >= 64 => 0,
        BitwiseOp::LeftShift => lhs.wrapping_shl(rhs as u32),
        BitwiseOp::RightShift if rhs < 0 => return None,
        BitwiseOp::RightShift if rhs >= 64 => {
            if lhs < 0 {
                -1
            } else {
                0
            }
        }
        BitwiseOp::RightShift => lhs >> rhs,
    })
}

/// The value of a literal-integer type, or `None` for anything else.
pub(crate) fn literal_int_value(ty: &PhpType) -> Option<i64> {
    ty.as_literal().and_then(LiteralValue::parse_i64)
}

/// An operator [`fold_int_expression`] splits an expression at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IntOp {
    Bitwise(BitwiseOp),
    Add,
    Sub,
    Mul,
}

impl IntOp {
    /// How tightly the operator binds; the lower, the earlier the split.
    fn precedence(self) -> u8 {
        match self {
            IntOp::Bitwise(BitwiseOp::Or) => 0,
            IntOp::Bitwise(BitwiseOp::Xor) => 1,
            IntOp::Bitwise(BitwiseOp::And) => 2,
            IntOp::Bitwise(BitwiseOp::LeftShift | BitwiseOp::RightShift) => 3,
            IntOp::Add | IntOp::Sub => 4,
            IntOp::Mul => 5,
        }
    }

    /// The integer `lhs op rhs` produces, or `None` when PHP would produce a
    /// float instead (an overflow) or no value at all.
    fn apply(self, lhs: i64, rhs: i64) -> Option<i64> {
        match self {
            IntOp::Bitwise(op) => apply_bitwise(op, lhs, rhs),
            IntOp::Add => lhs.checked_add(rhs),
            IntOp::Sub => lhs.checked_sub(rhs),
            IntOp::Mul => lhs.checked_mul(rhs),
        }
    }
}

/// The integer the constant expression `text` folds to.
///
/// Handles the operators PHP allows in a constant expression that produce an
/// integer from integers: `|`, `^`, `&`, `<<`, `>>`, `+`, `-`, `*`, unary
/// `~`/`-`/`+`, and parentheses. An arithmetic result that overflows into a
/// float leaves the expression unfolded. Every other term is handed to `resolve` and folds only when it
/// comes back a literal integer, which covers a global constant, a class
/// constant, and a variable holding one.
pub(crate) fn fold_int_expression(text: &str, resolve: TextResolver<'_>) -> Option<i64> {
    let text = strip_wrapping_parens(text.trim());
    match split_point(text) {
        Some((index, op_len, op)) => {
            let lhs = fold_int_expression(&text[..index], resolve)?;
            let rhs = fold_int_expression(&text[index + op_len..], resolve)?;
            op.apply(lhs, rhs)
        }
        None => fold_term(text, resolve),
    }
}

/// Whether `text` has a bitwise or integer arithmetic operator outside any
/// nested expression, i.e. whether [`fold_int_expression`] would read it as
/// an operator expression rather than as a single term.
///
/// Callers that resolve arbitrary expression text consult this first: folding
/// a single term asks `resolve` about that same text, which for a caller
/// reached *from* `resolve` would not terminate.
pub(crate) fn has_top_level_int_operator(text: &str) -> bool {
    split_point(strip_wrapping_parens(text.trim())).is_some()
}

/// Where [`fold_int_expression`] splits `text`, as the byte index of the
/// operator, its length, and which operator it is.
///
/// The split goes at the loosest-binding operator the expression has, and at
/// its last occurrence within that tier, so the recursion reproduces PHP's
/// precedence and left associativity. Quoted text and anything inside a
/// bracket pair is not top level, the doubled forms `||` and `&&` are the
/// boolean operators rather than the bitwise ones, and a `+`/`-` that does
/// not follow an operand is a sign rather than a binary operator. Returns
/// `None` for an expression with no operator of its own.
fn split_point(text: &str) -> Option<(usize, usize, IntOp)> {
    let bytes = text.as_bytes();
    let mut depth: u32 = 0;
    let mut quote: Option<u8> = None;
    let mut best: Option<(usize, usize, IntOp)> = None;
    // Whether the last significant byte ended an operand, which is what makes
    // a following `+`/`-` a binary operator rather than a sign.
    let mut after_operand = false;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if let Some(q) = quote {
            if byte == b'\\' {
                i += 2;
                continue;
            }
            if byte == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if byte.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => {
                quote = Some(byte);
                after_operand = true;
                i += 1;
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                after_operand = true;
                i += 1;
                continue;
            }
            b'|' | b'&' | b'*' if bytes.get(i + 1) == Some(&byte) => {
                // `||`, `&&` and `**` are not operators this folds, so they
                // are never a split point either.
                after_operand = false;
                i += 2;
                continue;
            }
            b'-' if bytes.get(i + 1) == Some(&b'>') => {
                i += 2;
                continue;
            }
            _ if depth == 0 => {
                let matched = match byte {
                    b'|' => Some((1, IntOp::Bitwise(BitwiseOp::Or))),
                    b'^' => Some((1, IntOp::Bitwise(BitwiseOp::Xor))),
                    b'&' => Some((1, IntOp::Bitwise(BitwiseOp::And))),
                    b'<' if bytes.get(i + 1) == Some(&b'<') => {
                        Some((2, IntOp::Bitwise(BitwiseOp::LeftShift)))
                    }
                    b'>' if bytes.get(i + 1) == Some(&b'>') => {
                        Some((2, IntOp::Bitwise(BitwiseOp::RightShift)))
                    }
                    b'+' if after_operand => Some((1, IntOp::Add)),
                    b'-' if after_operand => Some((1, IntOp::Sub)),
                    b'*' => Some((1, IntOp::Mul)),
                    _ => None,
                };
                if let Some((op_len, op)) = matched {
                    if best.is_none_or(|(_, _, found)| op.precedence() <= found.precedence()) {
                        best = Some((i, op_len, op));
                    }
                    after_operand = false;
                    i += op_len;
                    continue;
                }
            }
            _ => {}
        }
        after_operand = byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'\\');
        i += 1;
    }
    best
}

/// The integer a single operand folds to: an integer literal, a unary
/// operator applied to one, or a name the shared resolver reports as a
/// literal integer.
fn fold_term(text: &str, resolve: TextResolver<'_>) -> Option<i64> {
    if text.is_empty() {
        return None;
    }
    if let Some(rest) = text.strip_prefix('~') {
        return fold_int_expression(rest, resolve).map(|value| !value);
    }
    if let Some(rest) = text.strip_prefix('-') {
        return fold_int_expression(rest, resolve).and_then(i64::checked_neg);
    }
    if let Some(rest) = text.strip_prefix('+') {
        return fold_int_expression(rest, resolve);
    }
    if let Some(value) = parse_php_int_literal(text) {
        return Some(value);
    }
    resolve(text).as_ref().and_then(literal_int_value)
}

/// `text` without a pair of parentheses wrapping the whole of it.
fn strip_wrapping_parens(text: &str) -> &str {
    let mut text = text;
    while let Some(inner) = text.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        // `(a) | (b)` also starts with `(` and ends with `)`, but the two
        // pairs are not one wrapper: the depth returns to zero in between.
        if !is_balanced(inner) {
            return text;
        }
        text = inner.trim();
    }
    text
}

/// Whether every bracket in `text` is closed within it, i.e. whether it is a
/// complete expression rather than the middle of one.
fn is_balanced(text: &str) -> bool {
    let mut depth: i32 = 0;
    for byte in text.bytes() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

thread_local! {
    /// The constants whose initialiser is being folded further up the stack,
    /// named as the caller spells them (`Foo::FLAGS`, `JSON_FLAGS`).
    ///
    /// A constant defined in terms of itself, directly or through another
    /// constant, would otherwise fold forever. Re-entry on the same constant
    /// reports it unfoldable, which is what it was before this module read it.
    ///
    /// A stack rather than a set: folding nests strictly, so the guard pops
    /// its own entry and no key has to be kept a second time to remove it.
    static FOLDING: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// RAII guard for [`FOLDING`].
///
/// Also used outside this module by [`super::super::call_resolution::resolve_static_access_type`]
/// to guard its own recursive re-resolution of an untyped constant's
/// initialiser, which shares the same re-entrancy hazard (a constant
/// defined in terms of itself) but is not itself a fold.
pub(crate) struct FoldGuard;

impl FoldGuard {
    /// Claim `key` for folding, or `None` when it is already being folded
    /// further up the stack.
    pub(crate) fn acquire(key: &str) -> Option<Self> {
        FOLDING.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.iter().any(|entry| entry == key) {
                return None;
            }
            stack.push(key.to_string());
            Some(FoldGuard)
        })
    }
}

impl Drop for FoldGuard {
    fn drop(&mut self) {
        FOLDING.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

/// The literal type the constant named `key` holds, folded from its
/// initialiser text `value`.
///
/// This is the fallback for an initialiser that
/// [`crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value`]
/// could not read on its own, which is every initialiser that names another
/// constant. Returns `None` when the value stays unknown, leaving the
/// constant's type to be read from its declaration as before.
pub(crate) fn folded_constant_type(
    key: &str,
    value: &str,
    resolve: TextResolver<'_>,
) -> Option<PhpType> {
    let _guard = FoldGuard::acquire(key)?;
    let value = strip_wrapping_parens(value.trim());

    // An operator expression is folded operand by operand.
    if has_top_level_int_operator(value) || value.starts_with(['~', '-', '+']) {
        return fold_int_expression(value, resolve)
            .map(|folded| PhpType::literal_int(folded.to_string()));
    }
    // Anything else is one term, and a constant that is an alias of another
    // one (`const NS = Base::NS;`) holds whatever that one holds, whether or
    // not it is an integer. Asking the resolver directly rather than through
    // the fold covers the non-integer values too, and asks only once.
    resolve(value).filter(|ty| matches!(ty.kind(), TypeKind::Literal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for the shared resolver: the flags a `json_encode()` call
    /// would name, plus one non-integer constant.
    fn resolve(text: &str) -> Option<PhpType> {
        match text {
            "JSON_PRETTY_PRINT" => Some(PhpType::literal_int("128")),
            "JSON_THROW_ON_ERROR" => Some(PhpType::literal_int("4194304")),
            "Foo::NS" => Some(PhpType::literal_string_raw("'App\\\\Models'")),
            "$flags" => Some(PhpType::int()),
            _ => None,
        }
    }

    fn fold(text: &str) -> Option<i64> {
        fold_int_expression(text, &resolve)
    }

    #[test]
    fn a_single_name_folds_to_its_value() {
        assert_eq!(fold("JSON_THROW_ON_ERROR"), Some(4194304));
        assert_eq!(fold("  JSON_PRETTY_PRINT  "), Some(128));
    }

    #[test]
    fn a_bitwise_chain_folds_to_the_combined_mask() {
        assert_eq!(
            fold("JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR"),
            Some(4194432)
        );
        assert_eq!(fold("JSON_PRETTY_PRINT | 1 | 2"), Some(131));
        assert_eq!(fold("4194432 & JSON_PRETTY_PRINT"), Some(128));
        assert_eq!(fold("JSON_PRETTY_PRINT ^ 128"), Some(0));
    }

    #[test]
    fn shifts_and_unary_operators_fold() {
        assert_eq!(fold("1 << 22"), Some(4194304));
        assert_eq!(fold("1 << 22 | 128"), Some(4194432));
        assert_eq!(fold("-16 >> 2"), Some(-4));
        assert_eq!(fold("~0"), Some(-1));
        assert_eq!(fold("JSON_PRETTY_PRINT & ~128"), Some(0));
    }

    #[test]
    fn mixed_operators_follow_php_precedence() {
        // `^` binds tighter than `|`, and `&` tighter than both.
        assert_eq!(fold("1 | 2 ^ 3"), Some(1));
        assert_eq!(fold("1 ^ 2 | 3"), Some(3));
        assert_eq!(fold("1 | 6 & 3"), Some(3));
        assert_eq!(fold("1 << 2 | 1"), Some(5));
        // Same tier, left-associative: `(16 >> 2) << 1`.
        assert_eq!(fold("16 >> 2 << 1"), Some(8));
    }

    #[test]
    fn parentheses_group_operands() {
        assert_eq!(fold("(JSON_PRETTY_PRINT)"), Some(128));
        assert_eq!(fold("(1 | 2) << 4"), Some(48));
        assert_eq!(fold("(1) | (2)"), Some(3));
    }

    #[test]
    fn a_term_that_is_not_an_integer_leaves_the_expression_unfolded() {
        assert_eq!(fold("$flags"), None);
        assert_eq!(fold("$flags | JSON_THROW_ON_ERROR"), None);
        assert_eq!(fold("Foo::NS"), None);
        assert_eq!(fold("UNKNOWN_FLAG"), None);
        assert_eq!(fold(""), None);
    }

    #[test]
    fn integer_arithmetic_folds() {
        // The stub initialiser of `PHP_INT_MIN`.
        assert_eq!(fold("-9223372036854775807 - 1"), Some(i64::MIN));
        assert_eq!(fold("2 + 3 * 4"), Some(14));
        assert_eq!(fold("10 - 3 - 2"), Some(5));
        assert_eq!(fold("-5 - -1"), Some(-4));
        assert_eq!(fold("1 << 2 + 1"), Some(8));
        assert_eq!(fold("JSON_PRETTY_PRINT - 1"), Some(127));
    }

    #[test]
    fn integer_arithmetic_that_overflows_or_is_not_integer_stays_unfolded() {
        assert_eq!(fold("9223372036854775807 + 1"), None);
        assert_eq!(fold("2 ** 3"), None);
        assert_eq!(fold("2 * 3 ** 2"), None);
        assert_eq!(fold("$flags - 1"), None);
    }

    #[test]
    fn a_negative_shift_count_has_no_value() {
        assert_eq!(fold("1 << -1"), None);
        assert_eq!(fold("1 << 64"), Some(0));
        assert_eq!(fold("-1 >> 64"), Some(-1));
    }

    #[test]
    fn boolean_operators_are_not_bitwise_ones() {
        assert!(!has_top_level_int_operator("$a || JSON_THROW_ON_ERROR"));
        assert!(!has_top_level_int_operator("$a && $b"));
        assert!(!has_top_level_int_operator("$foo->bar"));
        assert!(!has_top_level_int_operator("mask(1 | 2)"));
        assert!(has_top_level_int_operator("1 | 2"));
        assert!(has_top_level_int_operator("(1 | 2)"));
        assert!(has_top_level_int_operator("(1 | 2) << 4"));
        assert!(!has_top_level_int_operator("-1"));
        assert!(has_top_level_int_operator("-1 - 1"));
    }

    #[test]
    fn an_alias_of_a_string_constant_folds_to_its_value() {
        let key = "Bar::NS";
        assert_eq!(
            folded_constant_type(key, "Foo::NS", &resolve).map(|ty| ty.to_string()),
            Some("'App\\\\Models'".to_string())
        );
    }

    #[test]
    fn a_cyclic_initialiser_terminates() {
        // `const A = self::B;` / `const B = self::A;`: resolving either one
        // re-enters folding for the other, and the resolver re-enters the
        // fold the way the real one does.
        fn cyclic(text: &str) -> Option<PhpType> {
            match text {
                "A" => folded_constant_type("A", "B", &cyclic),
                "B" => folded_constant_type("B", "A", &cyclic),
                _ => None,
            }
        }
        assert_eq!(folded_constant_type("A", "B", &cyclic), None);
    }
}
