//! Capture-group shape analysis for literal PCRE patterns.
//!
//! `preg_match($pattern, $subject, $matches)` fills `$matches` with one
//! entry per capture group in the pattern. When the pattern is a literal
//! string the group list is known statically, so the out-parameter can be
//! typed as an array shape instead of a bare `array`: key `0` is the whole
//! match, each capture group gets its number, and a named group
//! `(?<name>…)` contributes its name alongside its number, in the order PHP
//! itself stores them.
//!
//! That shape describes a *successful* match. A failed one leaves the empty
//! array behind, so where the outcome is not known the out-parameter holds
//! either ([`or_no_match`]), and a condition that decides the outcome says
//! which of the two a branch is looking at ([`preg_condition`]).
//!
//! The parser refuses any construct whose effect on group numbering or
//! group participation depends on the subject (conditional groups,
//! recursion, `(*ACCEPT)`), and the caller keeps the imprecise but correct
//! keyless array. A shape that is merely incomplete would be worse than no
//! shape at all: reads of the keys it forgot resolve to nothing.

use std::borrow::Cow;

use mago_syntax::cst::{
    Argument, ArgumentList, BinaryOperator, Call, Expression, Literal, UnaryPrefixOperator,
    Variable,
};

use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::php_type::{PhpType, ShapeEntry};

mod parse;
mod shape;

/// `PREG_PATTERN_ORDER`: `preg_match_all` groups the results by capture
/// group. The default when no order flag is passed.
pub(crate) const PREG_PATTERN_ORDER: i64 = 1;
/// `PREG_SET_ORDER`: `preg_match_all` groups the results by match.
pub(crate) const PREG_SET_ORDER: i64 = 2;
/// `PREG_OFFSET_CAPTURE`: every entry becomes a `[value, offset]` pair.
pub(crate) const PREG_OFFSET_CAPTURE: i64 = 256;
/// `PREG_UNMATCHED_AS_NULL`: unmatched groups report `null` rather than `''`.
pub(crate) const PREG_UNMATCHED_AS_NULL: i64 = 512;

/// The flag bits whose effect on the match shape this module models. A
/// caller that sees any other bit must not use the result.
const MODELLED_FLAGS: i64 =
    PREG_PATTERN_ORDER | PREG_SET_ORDER | PREG_OFFSET_CAPTURE | PREG_UNMATCHED_AS_NULL;

/// Whether `flags` contains only bits whose effect on the shape is modelled.
pub(crate) fn flags_are_modelled(flags: i64) -> bool {
    flags & !MODELLED_FLAGS == 0
}

/// A `preg_match` or `preg_match_all` call that fills a plain variable.
pub(crate) struct PregCall<'b> {
    /// Whether the call is `preg_match_all`, whose result holds every match
    /// rather than the first.
    pub matches_all: bool,
    /// The pattern argument, read by [`pattern_texts`].
    pub pattern: Option<&'b Expression<'b>>,
    /// The name of the `$matches` variable, `$` included.
    pub matches_var: &'b str,
    /// The `$flags` argument, when the call passes one. Its value decides
    /// the result's shape, so a caller that cannot read it must not use the
    /// analysis.
    pub flags: Option<&'b Expression<'b>>,
}

/// Recognise a `preg_match`/`preg_match_all` call whose `$matches` argument
/// is a plain variable.
pub(crate) fn preg_call<'b>(expr: &'b Expression<'b>) -> Option<PregCall<'b>> {
    let Expression::Call(Call::Function(call)) = expr else {
        return None;
    };
    let Expression::Identifier(ident) = call.function else {
        return None;
    };
    let raw = bytes_to_str(ident.value());
    let name = raw.strip_prefix('\\').unwrap_or(raw);
    let matches_all = if name.eq_ignore_ascii_case("preg_match") {
        false
    } else if name.eq_ignore_ascii_case("preg_match_all") {
        true
    } else {
        return None;
    };

    let matches_var = match argument(&call.argument_list, 2, "matches")? {
        Expression::Variable(Variable::Direct(var)) => bytes_to_str(var.name),
        _ => return None,
    };
    Some(PregCall {
        matches_all,
        pattern: argument(&call.argument_list, 0, "pattern"),
        matches_var,
        flags: argument(&call.argument_list, 3, "flags"),
    })
}

/// The match `preg_replace_callback()` hands its callback, described as the
/// `preg_match` call that would fill `$matches` the same way: the callback
/// runs once per successful match, with the groups of that match.
///
/// `function` is the called name as written and `arguments` the call's
/// argument list. `None` for any other function.
pub(crate) fn preg_replace_callback_match<'b>(
    function: &str,
    arguments: &'b ArgumentList<'b>,
) -> Option<PregCall<'b>> {
    let name = function.strip_prefix('\\').unwrap_or(function);
    if !name.eq_ignore_ascii_case("preg_replace_callback") {
        return None;
    }
    Some(PregCall {
        matches_all: false,
        pattern: argument(arguments, 0, "pattern"),
        matches_var: "",
        flags: argument(arguments, 5, "flags"),
    })
}

/// Recognise a condition whose truth decides whether a `preg_match` /
/// `preg_match_all` call found anything.
///
/// Returns the call and whether the condition being *true* means it matched,
/// which is what lets the branch that only runs on a successful match keep the
/// full shape while the other one gets the empty array.
///
/// The comparison forms read the result as the `0|1` the documentation
/// promises. `false`, which a malformed pattern returns, leaves `$matches`
/// empty exactly as a failed match does, so folding it into the no-match side
/// costs nothing.
pub(crate) fn preg_condition<'b>(expr: &'b Expression<'b>) -> Option<(PregCall<'b>, bool)> {
    match expr {
        Expression::Parenthesized(inner) => preg_condition(inner.expression),
        Expression::UnaryPrefix(unary) if matches!(unary.operator, UnaryPrefixOperator::Not(_)) => {
            let (call, matched) = preg_condition(unary.operand)?;
            Some((call, !matched))
        }
        // `(bool) preg_match(…)` is true exactly when the pattern matched.
        Expression::UnaryPrefix(unary)
            if matches!(
                unary.operator,
                UnaryPrefixOperator::BoolCast(..) | UnaryPrefixOperator::BooleanCast(..)
            ) =>
        {
            preg_condition(unary.operand)
        }
        Expression::Binary(binary) => {
            let comparison = Comparison::of(&binary.operator)?;
            // Normalise to the call on the left, so `0 < preg_match(…)` reads
            // as the `preg_match(…) > 0` it is.
            let (call, literal, comparison) = match preg_call(binary.lhs) {
                Some(call) => (call, binary.rhs, comparison),
                None => (preg_call(binary.rhs)?, binary.lhs, comparison.mirrored()),
            };
            Some((call, comparison.means_matched(int_literal(literal)?)?))
        }
        _ => Some((preg_call(expr)?, true)),
    }
}

/// How a condition compares a `preg_match` result against a literal.
#[derive(Clone, Copy)]
enum Comparison {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

impl Comparison {
    /// The comparison `operator` performs, or `None` for an operator that is
    /// not one. Loose and strict spellings agree here: both operands are
    /// integers.
    fn of(operator: &BinaryOperator<'_>) -> Option<Self> {
        Some(match operator {
            BinaryOperator::Equal(_) | BinaryOperator::Identical(_) => Comparison::Equal,
            BinaryOperator::NotEqual(_)
            | BinaryOperator::NotIdentical(_)
            | BinaryOperator::AngledNotEqual(_) => Comparison::NotEqual,
            BinaryOperator::GreaterThan(_) => Comparison::Greater,
            BinaryOperator::GreaterThanOrEqual(_) => Comparison::GreaterOrEqual,
            BinaryOperator::LessThan(_) => Comparison::Less,
            BinaryOperator::LessThanOrEqual(_) => Comparison::LessOrEqual,
            _ => return None,
        })
    }

    /// The same comparison with its operands swapped.
    fn mirrored(self) -> Self {
        match self {
            Comparison::Equal => Comparison::Equal,
            Comparison::NotEqual => Comparison::NotEqual,
            Comparison::Greater => Comparison::Less,
            Comparison::GreaterOrEqual => Comparison::LessOrEqual,
            Comparison::Less => Comparison::Greater,
            Comparison::LessOrEqual => Comparison::GreaterOrEqual,
        }
    }

    /// Whether comparing the result against `value` holds exactly when the
    /// match succeeded.
    ///
    /// `None` for a comparison that separates nothing: `>= 0` holds for both
    /// outcomes and `> 1` for neither, so a branch guarded by one learns
    /// nothing about the out-parameter.
    fn means_matched(self, value: u64) -> Option<bool> {
        Some(match (self, value) {
            (Comparison::Equal, 1)
            | (Comparison::NotEqual, 0)
            | (Comparison::Greater, 0)
            | (Comparison::GreaterOrEqual, 1) => true,
            (Comparison::Equal, 0)
            | (Comparison::NotEqual, 1)
            | (Comparison::Less, 1)
            | (Comparison::LessOrEqual, 0) => false,
            _ => return None,
        })
    }
}

/// The value of a non-negative integer literal.
fn int_literal(expr: &Expression<'_>) -> Option<u64> {
    match expr {
        Expression::Parenthesized(inner) => int_literal(inner.expression),
        Expression::Literal(Literal::Integer(integer)) => integer.value,
        _ => None,
    }
}

/// The argument at positional `index`, or the one named `name`.
fn argument<'b>(
    arguments: &'b ArgumentList<'b>,
    index: usize,
    name: &str,
) -> Option<&'b Expression<'b>> {
    let mut position = 0;
    for argument in arguments.arguments.iter() {
        match argument {
            Argument::Positional(positional) => {
                if position == index {
                    return Some(positional.value);
                }
                position += 1;
            }
            Argument::Named(named) => {
                if bytes_to_str(named.name.value) == name {
                    return Some(named.value);
                }
            }
        }
    }
    None
}

/// The content of a single- or double-quoted string literal with no
/// interpolation, which is the only form whose pattern is known statically.
fn literal_string<'b>(expr: &'b Expression<'b>) -> Option<Cow<'b, str>> {
    let Expression::Literal(Literal::String(string)) = expr else {
        return None;
    };
    match string.value {
        Some(value) => literal_bytes_to_str(value).map(Cow::Borrowed),
        None => crate::text_scan::decode_php_string_literal(bytes_to_str(string.raw)),
    }
}

/// How many patterns a pattern argument may stand for before the analysis
/// gives up on it.
const MAX_PATTERNS: usize = 16;

/// Every pattern text the `pattern` argument can hold, or `None` when one of
/// them is not known.
///
/// A literal is read directly; anything else goes through `resolve`, so a
/// variable assigned one of several literals, a constant, or a heredoc all
/// read the same. Concatenation is read piece by piece, with a
/// `preg_quote()` call standing in as a group-free `(?:.*)`: whatever text
/// it quotes, it matches that text literally and opens no group.
pub(crate) fn pattern_texts(
    pattern: &Expression<'_>,
    resolve: &dyn Fn(&Expression<'_>) -> Option<PhpType>,
) -> Option<Vec<String>> {
    match pattern {
        Expression::Parenthesized(inner) => pattern_texts(inner.expression, resolve),
        Expression::Literal(Literal::String(_)) if let Some(text) = literal_string(pattern) => {
            Some(vec![text.into_owned()])
        }
        Expression::Binary(binary)
            if matches!(binary.operator, BinaryOperator::StringConcat(_)) =>
        {
            let heads = pattern_texts(binary.lhs, resolve)?;
            let tails = pattern_texts(binary.rhs, resolve)?;
            if heads.len() * tails.len() > MAX_PATTERNS {
                return None;
            }
            Some(
                heads
                    .iter()
                    .flat_map(|head| tails.iter().map(move |tail| format!("{head}{tail}")))
                    .collect(),
            )
        }
        Expression::Call(Call::Function(call))
            if matches!(call.function, Expression::Identifier(ident)
                if bytes_to_str(ident.value()).trim_start_matches('\\').eq_ignore_ascii_case("preg_quote")) =>
        {
            Some(vec!["(?:.*)".to_string()])
        }
        _ => {
            let ty = resolve(pattern)?;
            let members = ty.union_members();
            if members.len() > MAX_PATTERNS {
                return None;
            }
            members
                .into_iter()
                .map(|member| {
                    // A literal int concatenated into the pattern reads as its
                    // digits.
                    let literal = member.as_literal()?;
                    match literal.string_content() {
                        Some(text) => Some(text.into_owned()),
                        None => literal.parse_i64().map(|n| n.to_string()),
                    }
                })
                .collect()
        }
    }
}

/// The type of the `$matches` out-parameter of `preg_match` (or
/// `preg_match_all` when `matches_all`) for a pattern argument that may hold
/// any of `patterns`.
///
/// Returns `None` when a pattern cannot be analysed, leaving the caller
/// with the keyless [`opaque_matches_type`] or the declared `array`.
pub(crate) fn matches_type_of_any<S: AsRef<str>>(
    patterns: &[S],
    flags: i64,
    matches_all: bool,
) -> Option<PhpType> {
    if !flags_are_modelled(flags) {
        return None;
    }
    let mut shapes = Vec::new();
    for pattern in patterns {
        let (body, modifiers) = split_delimiters(pattern.as_ref())?;
        let analysis = shape::Analysis::of(&parse::parse(body, modifiers)?)?;
        shapes.extend(analysis.matched_shapes(flags, matches_all));
    }
    Some(shape::finish_union(shapes, flags, matches_all))
}

/// The type of the `$matches` out-parameter when the pattern is not a
/// literal, so only the flags say anything about the result.
///
/// The keys are unknown but their values are not: every entry of a
/// `preg_match` result is a string, and every entry of a `preg_match_all`
/// result in pattern order is a list of them.
pub(crate) fn opaque_matches_type(flags: i64, matches_all: bool) -> Option<PhpType> {
    if !flags_are_modelled(flags) {
        return None;
    }
    let mut entry = PhpType::string();
    if flags & PREG_OFFSET_CAPTURE != 0 {
        entry = PhpType::array_shape(vec![
            ShapeEntry {
                key: None,
                value_type: entry,
                optional: false,
            },
            ShapeEntry {
                key: None,
                value_type: PhpType::int_range("-1", "max"),
                optional: false,
            },
        ]);
    }
    if matches_all && flags & PREG_SET_ORDER == 0 {
        entry = PhpType::list(entry);
    }
    let keyed = PhpType::generic_array(PhpType::named(atom("array-key")), entry);
    Some(if matches_all && flags & PREG_SET_ORDER != 0 {
        // One entry per match, each holding that match's groups.
        PhpType::list(keyed)
    } else {
        keyed
    })
}

/// The type of the `$matches` out-parameter of `preg_match` when the flags
/// are not known: any entry may be `null` (`PREG_UNMATCHED_AS_NULL`) or an
/// offset pair (`PREG_OFFSET_CAPTURE`).
pub(crate) fn any_flags_matches_type() -> PhpType {
    let value = PhpType::union(vec![PhpType::string(), PhpType::null()]);
    let pair = PhpType::array_shape(vec![
        ShapeEntry {
            key: None,
            value_type: value.clone(),
            optional: false,
        },
        ShapeEntry {
            key: None,
            value_type: PhpType::int_range("-1", "max"),
            optional: false,
        },
    ]);
    PhpType::generic_array(
        PhpType::named(atom("array-key")),
        PhpType::union(vec![pair, PhpType::string(), PhpType::null()]),
    )
}

/// What a failed match leaves in `$matches`: the empty array.
///
/// `None` for `preg_match_all`, which writes one empty list per group in
/// pattern order and the empty list in set order, both of which its success
/// type describes already.
pub(crate) fn no_match_type(matches_all: bool) -> Option<PhpType> {
    (!matches_all).then(|| PhpType::array_shape(Vec::new()))
}

/// What `$matches` holds after a call whose outcome is not known: either what
/// a successful match wrote or what a failed one left.
///
/// The success type comes first so that a consumer reading the first shape out
/// of the union (key completion, a shape-entry lookup) finds the informative
/// half rather than the empty one.
///
/// A success type that admits the empty array already (the keyless
/// `array<…>` an unreadable pattern falls back to) is left as it is.
pub(crate) fn or_no_match(matched: PhpType, matches_all: bool) -> PhpType {
    match no_match_type(matches_all) {
        Some(no_match) if !matched.accepts_empty_array() => PhpType::union(vec![matched, no_match]),
        _ => matched,
    }
}

/// Split `pattern` into the regex body and the trailing modifier letters.
///
/// PCRE allows leading whitespace before the delimiter, any non-alphanumeric
/// non-backslash character as the delimiter, and bracket-style delimiters
/// whose closing counterpart differs from the opener.
pub(crate) fn split_delimiters(pattern: &str) -> Option<(&str, &str)> {
    let pattern = pattern.trim_start();
    let open = pattern.chars().next()?;
    // A non-ASCII delimiter is legal but rare, and the byte-level scan below
    // would need to compare whole characters to find its counterpart.
    if !open.is_ascii() || open.is_alphanumeric() || open == '\\' || open.is_whitespace() {
        return None;
    }
    let body_start = 1;
    let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        other => other,
    };
    let bytes = pattern.as_bytes();
    let mut i = body_start;
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            c if c == close as u8 => {
                if depth == 0 {
                    return Some((&pattern[body_start..i], &pattern[i + 1..]));
                }
                depth -= 1;
            }
            // Only a bracket-style delimiter nests; for the symmetric form
            // `open == close` and the arm above already matched.
            c if c == open as u8 => depth += 1,
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
#[path = "regex_shape_tests.rs"]
mod tests;
