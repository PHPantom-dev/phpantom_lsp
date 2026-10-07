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
//! The walk refuses more than it accepts. Any construct whose effect on
//! group numbering or group participation it cannot model — branch reset
//! `(?|`, conditional groups, recursion, `\Q…\E` quoting, the `x` extended
//! mode — abandons the analysis, and the caller keeps the imprecise but
//! correct bare `array`. A shape that is merely incomplete would be worse
//! than no shape at all: reads of the keys it forgot resolve to nothing.

use std::borrow::Cow;

use mago_syntax::cst::{
    Argument, ArgumentList, BinaryOperator, Call, Expression, Literal, UnaryPrefixOperator,
    Variable,
};

use crate::atom::{atom, bytes_to_str, literal_bytes_to_str};
use crate::php_type::{PhpType, ShapeEntry};

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

/// One capture group of a parsed pattern, in numbering order.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureGroup {
    /// The group's name, for `(?<name>…)`, `(?'name'…)` and `(?P<name>…)`.
    name: Option<String>,
    /// Whether a successful overall match can leave this group unmatched:
    /// it carries a quantifier that allows zero repetitions, sits inside a
    /// group that does, is in one branch of an alternation, or is inside a
    /// negative lookaround.
    optional: bool,
}

/// Whether `flags` contains only bits whose effect on the shape is modelled.
pub(crate) fn flags_are_modelled(flags: i64) -> bool {
    flags & !MODELLED_FLAGS == 0
}

/// A `preg_match` or `preg_match_all` call that fills a plain variable.
pub(crate) struct PregCall<'b> {
    /// Whether the call is `preg_match_all`, whose result holds every match
    /// rather than the first.
    pub matches_all: bool,
    /// The pattern, when the argument is a plain string literal.
    pub pattern: Option<Cow<'b, str>>,
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
        pattern: argument(&call.argument_list, 0, "pattern").and_then(literal_string),
        matches_var,
        flags: argument(&call.argument_list, 3, "flags"),
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

/// The type of the `$matches` out-parameter of `preg_match` (or
/// `preg_match_all` when `matches_all`) for a literal `pattern`.
///
/// Returns `None` when the pattern cannot be analysed, leaving the caller
/// with the keyless [`opaque_matches_type`] or the declared `array`.
pub(crate) fn matches_type(pattern: &str, flags: i64, matches_all: bool) -> Option<PhpType> {
    if !flags_are_modelled(flags) {
        return None;
    }
    let groups = capture_groups(pattern)?;
    Some(build_shape(&groups, flags, matches_all))
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
    let entry = entry_value_type(flags, false, matches_all);
    let keyed = PhpType::generic_array(PhpType::named(atom("array-key")), entry);
    Some(if matches_all && flags & PREG_SET_ORDER != 0 {
        // One entry per match, each holding that match's groups.
        PhpType::list(keyed)
    } else {
        keyed
    })
}

/// What a failed match leaves in `$matches`, when that differs from what a
/// successful one leaves: the empty array, which satisfies none of the shapes
/// [`matches_type`] builds.
///
/// `None` where the success type already covers the no-match case, so a branch
/// that knows the match failed has nothing to narrow to. `preg_match_all`
/// writes one empty list per group in pattern order and the empty list in set
/// order, both of which its success type describes; and the keyless
/// `array<…>` an unreadable pattern falls back to admits the empty array on
/// its own.
pub(crate) fn no_match_type(matched: &PhpType, matches_all: bool) -> Option<PhpType> {
    (!matches_all && !matched.accepts_empty_array()).then(|| PhpType::array_shape(Vec::new()))
}

/// What `$matches` holds after a call whose outcome is not known: either what
/// a successful match wrote or what a failed one left.
///
/// The success type comes first so that a consumer reading the first shape out
/// of the union (key completion, a shape-entry lookup) finds the informative
/// half rather than the empty one.
pub(crate) fn or_no_match(matched: PhpType, matches_all: bool) -> PhpType {
    match no_match_type(&matched, matches_all) {
        Some(no_match) => PhpType::union(vec![matched, no_match]),
        None => matched,
    }
}

/// Assemble the shape from the parsed group list.
fn build_shape(groups: &[CaptureGroup], flags: i64, matches_all: bool) -> PhpType {
    let set_order = matches_all && flags & PREG_SET_ORDER != 0;
    // Only a run of optional groups at the very end of the pattern can be
    // missing from the result: PHP reports an unmatched group that is
    // followed by a matched one as `''`, and drops only the trailing ones.
    let trailing_optional = groups.iter().rev().take_while(|g| g.optional).count();
    let first_trailing = groups.len() - trailing_optional;
    // Under `PREG_UNMATCHED_AS_NULL` every group is reported, `null` when it
    // did not match, so no key is ever missing. `preg_match_all` in pattern
    // order likewise reports every group, as a list.
    let keys_always_present = flags & PREG_UNMATCHED_AS_NULL != 0 || (matches_all && !set_order);

    let mut entries = Vec::with_capacity(groups.len() + 1);
    entries.push(ShapeEntry {
        key: Some("0".to_string()),
        value_type: entry_value_type(flags, false, matches_all),
        optional: false,
    });
    for (index, group) in groups.iter().enumerate() {
        let value_type = entry_value_type(flags, group.optional, matches_all);
        let optional = !keys_always_present && index >= first_trailing;
        if let Some(name) = &group.name {
            entries.push(ShapeEntry {
                key: Some(name.clone()),
                value_type: value_type.clone(),
                optional,
            });
        }
        entries.push(ShapeEntry {
            key: Some((index + 1).to_string()),
            value_type,
            optional,
        });
    }

    let shape = PhpType::array_shape(entries);
    if set_order {
        // One shape per match, in a list.
        PhpType::list(shape)
    } else {
        shape
    }
}

/// The type of one entry of the result: the matched substring, wrapped for
/// whichever flags apply.
fn entry_value_type(flags: i64, group_optional: bool, matches_all: bool) -> PhpType {
    let mut value = PhpType::string();
    if flags & PREG_UNMATCHED_AS_NULL != 0 && group_optional {
        value = value.or_null();
    }
    if flags & PREG_OFFSET_CAPTURE != 0 {
        // An unmatched group reports `-1` as its offset.
        value = PhpType::array_shape(vec![
            ShapeEntry {
                key: None,
                value_type: value,
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
        // Pattern order: each key holds every match of that group.
        value = PhpType::list(value);
    }
    value
}

/// Split a PHP pattern into its body and modifiers, then walk the body for
/// capture groups.
fn capture_groups(pattern: &str) -> Option<Vec<CaptureGroup>> {
    let (body, modifiers) = split_delimiters(pattern)?;
    // Only the modifiers that leave the group structure alone are accepted.
    // `x` turns whitespace into padding and `#` into a comment marker, which
    // changes where a `(` opens a group; `n` makes an unnamed group
    // non-capturing; and a letter PHP does not define is not a pattern it
    // would run at all.
    let mut no_auto_capture = false;
    for c in modifiers.chars() {
        match c {
            'i' | 'm' | 's' | 'A' | 'D' | 'S' | 'U' | 'X' | 'J' | 'u' => {}
            'n' => no_auto_capture = true,
            _ if c.is_whitespace() => {}
            _ => return None,
        }
    }
    let groups = walk_body(body, no_auto_capture)?;
    // Duplicate names (which the `J` modifier allows) would put the same key
    // in the shape twice, one of them wrong.
    let mut names: Vec<&str> = groups.iter().filter_map(|g| g.name.as_deref()).collect();
    let named = names.len();
    names.sort_unstable();
    names.dedup();
    (names.len() == named).then_some(groups)
}

/// Split `pattern` into the regex body and the trailing modifier letters.
///
/// PCRE allows leading whitespace before the delimiter, any non-alphanumeric
/// non-backslash character as the delimiter, and bracket-style delimiters
/// whose closing counterpart differs from the opener.
fn split_delimiters(pattern: &str) -> Option<(&str, &str)> {
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

/// A group currently being scanned, or the pattern itself.
struct Frame {
    /// Index into the group list of the capture group this frame opened.
    group: Option<usize>,
    /// The group index the frame opened at, so the groups nested inside it
    /// are `groups[first_nested..]` once it closes.
    first_nested: usize,
    /// A `|` appeared at this frame's own level, so each of its branches
    /// leaves the groups of the others unmatched.
    has_alternation: bool,
    /// The frame is a negative lookaround, so a successful overall match
    /// means its groups did *not* participate.
    negated: bool,
}

/// Walk the pattern body and collect its capture groups in numbering order.
fn walk_body(body: &str, no_auto_capture: bool) -> Option<Vec<CaptureGroup>> {
    let bytes = body.as_bytes();
    let mut groups: Vec<CaptureGroup> = Vec::new();
    let mut stack = vec![Frame {
        group: None,
        first_nested: 0,
        has_alternation: false,
        negated: false,
    }];
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                // `\Q…\E` suspends metacharacters, so a `(` inside it is a
                // literal. Rather than track the quoted region, refuse it.
                if matches!(bytes.get(i + 1), Some(b'Q')) {
                    return None;
                }
                i += 2;
            }
            b'[' => i = skip_character_class(bytes, i)?,
            b'|' => {
                stack.last_mut()?.has_alternation = true;
                i += 1;
            }
            b'(' => {
                let (frame, next) = open_group(bytes, i, &mut groups, no_auto_capture)?;
                // A construct that consumed its own closing paren (a comment,
                // a backreference, a modifier setting) opens no frame.
                if let Some(frame) = frame {
                    stack.push(frame);
                }
                i = next;
            }
            b')' => {
                let frame = stack.pop()?;
                // The outermost frame is the pattern itself; a `)` that
                // closes it is unbalanced.
                if stack.is_empty() {
                    return None;
                }
                let (allows_zero, next) = read_quantifier(bytes, i + 1);
                if allows_zero || frame.negated {
                    for group in &mut groups[frame.first_nested..] {
                        group.optional = true;
                    }
                }
                if frame.has_alternation {
                    // The frame's own group still matches; only the groups
                    // in its branches are alternatives to one another.
                    let nested = frame.first_nested + usize::from(frame.group.is_some());
                    for group in &mut groups[nested..] {
                        group.optional = true;
                    }
                }
                i = next;
            }
            _ => i += 1,
        }
    }

    let top = stack.pop()?;
    if !stack.is_empty() {
        return None;
    }
    if top.has_alternation {
        for group in &mut groups {
            group.optional = true;
        }
    }
    Some(groups)
}

/// Handle a `(` at `start`.
///
/// Returns the frame it opens (`None` for a construct that is complete in
/// itself, such as `(?#comment)`) and the offset to continue scanning from.
fn open_group(
    bytes: &[u8],
    start: usize,
    groups: &mut Vec<CaptureGroup>,
    no_auto_capture: bool,
) -> Option<(Option<Frame>, usize)> {
    let plain = |group: Option<usize>, first_nested: usize, negated: bool, next: usize| {
        Some((
            Some(Frame {
                group,
                first_nested,
                has_alternation: false,
                negated,
            }),
            next,
        ))
    };
    let capturing = |groups: &mut Vec<CaptureGroup>, name: Option<String>, next: usize| {
        let index = groups.len();
        groups.push(CaptureGroup {
            name,
            optional: false,
        });
        Some((
            Some(Frame {
                group: Some(index),
                first_nested: index,
                has_alternation: false,
                negated: false,
            }),
            next,
        ))
    };

    if bytes.get(start + 1) != Some(&b'?') {
        // `(*ACCEPT)`, `(*MARK:…)` and friends change control flow, and
        // `MARK` adds a key of its own.
        if bytes.get(start + 1) == Some(&b'*') {
            return None;
        }
        if no_auto_capture {
            return plain(None, groups.len(), false, start + 1);
        }
        return capturing(groups, None, start + 1);
    }

    match bytes.get(start + 2)? {
        b':' => plain(None, groups.len(), false, start + 3),
        b'=' => plain(None, groups.len(), false, start + 3),
        b'!' => plain(None, groups.len(), true, start + 3),
        b'>' => plain(None, groups.len(), false, start + 3),
        // A comment runs to the first `)`; it cannot contain an escaped one.
        b'#' => {
            let end = bytes[start + 3..].iter().position(|&c| c == b')')?;
            Some((None, start + 4 + end))
        }
        b'\'' => {
            let (name, next) = read_group_name(bytes, start + 3, b'\'')?;
            capturing(groups, Some(name), next)
        }
        b'P' => match bytes.get(start + 3)? {
            b'<' => {
                let (name, next) = read_group_name(bytes, start + 4, b'>')?;
                capturing(groups, Some(name), next)
            }
            // `(?P=name)` is a backreference, `(?P>name)` a subroutine call.
            b'=' => {
                let end = bytes[start + 4..].iter().position(|&c| c == b')')?;
                Some((None, start + 5 + end))
            }
            _ => None,
        },
        b'<' => match bytes.get(start + 3)? {
            b'=' => plain(None, groups.len(), false, start + 4),
            b'!' => plain(None, groups.len(), true, start + 4),
            _ => {
                let (name, next) = read_group_name(bytes, start + 3, b'>')?;
                capturing(groups, Some(name), next)
            }
        },
        // Branch reset restarts the group counter per branch, conditionals
        // and recursion make participation depend on the subject.
        b'|' | b'(' | b'R' | b'&' | b'+' | b'C' => None,
        b'0'..=b'9' => None,
        _ => read_modifier_setting(bytes, start, groups.len()),
    }
}

/// Handle `(?imsx…)` and `(?imsx…:…)`.
///
/// The first form applies to the rest of the enclosing group and is complete
/// in itself; the second opens a non-capturing group.
fn read_modifier_setting(
    bytes: &[u8],
    start: usize,
    first_nested: usize,
) -> Option<(Option<Frame>, usize)> {
    let mut i = start + 2;
    while i < bytes.len() {
        match bytes[i] {
            b'i' | b'm' | b's' | b'U' | b'X' | b'J' | b'-' | b'^' => i += 1,
            // Extended mode and no-auto-capture change how the *rest* of
            // the pattern parses, which this walk applies pattern-wide or
            // not at all.
            b'x' | b'n' => return None,
            b':' => {
                return Some((
                    Some(Frame {
                        group: None,
                        first_nested,
                        has_alternation: false,
                        negated: false,
                    }),
                    i + 1,
                ));
            }
            b')' => return Some((None, i + 1)),
            _ => return None,
        }
    }
    None
}

/// Read a group name terminated by `close`, starting at `start`.
///
/// Returns the name and the offset just past the terminator.
fn read_group_name(bytes: &[u8], start: usize, close: u8) -> Option<(String, usize)> {
    let mut i = start;
    while i < bytes.len() && bytes[i] != close {
        if !bytes[i].is_ascii_alphanumeric() && bytes[i] != b'_' {
            return None;
        }
        i += 1;
    }
    if i == start || i >= bytes.len() {
        return None;
    }
    let name = std::str::from_utf8(&bytes[start..i]).ok()?.to_string();
    Some((name, i + 1))
}

/// Skip the character class starting at `start` (which indexes the `[`),
/// returning the offset just past its `]`.
fn skip_character_class(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    // A `]` in first position (after an optional `^`) is a literal.
    if bytes.get(i) == Some(&b'^') {
        i += 1;
    }
    if bytes.get(i) == Some(&b']') {
        i += 1;
    }
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b']' => return Some(i + 1),
            // A POSIX class such as `[:alpha:]` brings its own `]`.
            b'[' if matches!(bytes.get(i + 1), Some(b':')) => {
                let end = bytes[i + 2..]
                    .windows(2)
                    .position(|w| w == b":]")
                    .map(|p| i + 2 + p + 2)?;
                i = end;
            }
            _ => i += 1,
        }
    }
    None
}

/// Read the quantifier at `start`, if any.
///
/// Returns whether it allows zero repetitions (so the quantified item may
/// not participate in a match) and the offset just past it.
fn read_quantifier(bytes: &[u8], start: usize) -> (bool, usize) {
    let (allows_zero, mut i) = match bytes.get(start) {
        Some(b'?') | Some(b'*') => (true, start + 1),
        Some(b'+') => (false, start + 1),
        Some(b'{') => match read_brace_quantifier(bytes, start) {
            Some((allows_zero, next)) => (allows_zero, next),
            // Not a quantifier: a literal `{`.
            None => return (false, start),
        },
        _ => return (false, start),
    };
    // A lazy or possessive marker (`??`, `*+`, `{2,3}?`) does not change
    // whether zero repetitions are allowed.
    if matches!(bytes.get(i), Some(b'?') | Some(b'+')) {
        i += 1;
    }
    (allows_zero, i)
}

/// Read a `{n}`, `{n,}`, `{n,m}` or `{,m}` quantifier at `start`.
///
/// Returns `None` when the braces do not spell a quantifier, in which case
/// PCRE treats the `{` as a literal.
fn read_brace_quantifier(bytes: &[u8], start: usize) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let min_start = i;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let min = &bytes[min_start..i];
    if bytes.get(i) == Some(&b',') {
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    } else if min.is_empty() {
        return None;
    }
    if bytes.get(i) != Some(&b'}') {
        return None;
    }
    // `{,m}` means zero to m; `{0…}` is spelled out.
    let allows_zero = min.iter().all(|&c| c == b'0');
    Some((allows_zero, i + 1))
}

#[cfg(test)]
#[path = "regex_shape_tests.rs"]
mod tests;
