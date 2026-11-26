//! `preg_split()`: the shape of the pieces its flags ask for, and the pieces
//! themselves when every argument is a literal. Mirrors PHPStan's
//! `PregSplitDynamicReturnTypeExtension`.
//!
//! A literal call is folded by running the pattern through the `regex`
//! crate, which only agrees with PCRE on a common subset. Anything outside
//! that subset (a modifier other than `i`, `m`, `s` or `u`, a `$` whose
//! trailing-newline rule differs, a pattern that can match the empty string)
//! is typed from the flags alone instead.

use crate::php_type::{PhpType, ShapeEntry};

use super::{CallArgs, literal_int, split_bitwise_or};

const PREG_SPLIT_NO_EMPTY: i64 = 1;
const PREG_SPLIT_DELIM_CAPTURE: i64 = 2;
const PREG_SPLIT_OFFSET_CAPTURE: i64 = 4;

/// How many results a literal call may fold to before it is typed from its
/// flags instead: one per combination of literal alternatives.
const MAX_FOLDED_CALLS: usize = 16;
/// How many pieces one folded result may hold.
const MAX_FOLDED_PIECES: usize = 64;

pub(super) fn return_type(args: &CallArgs<'_>) -> Option<PhpType> {
    let pattern = args.resolve(&args.text("$pattern")?)?;
    let subject = args.resolve(&args.text("$subject")?)?;
    let limits = match args.text("$limit") {
        None => Some(vec![-1]),
        Some(text) => int_values(args, &text)?,
    };
    let flags_text = args.text("$flags");
    let flags = match &flags_text {
        None => Some(vec![0]),
        Some(text) => int_values(args, text)?,
    };

    if let (Some(patterns), Some(subjects), Some(limits), Some(flags)) = (
        literal_strings(&pattern),
        literal_strings(&subject),
        limits.as_ref(),
        flags.as_ref(),
    ) && let Some(folded) = fold(&patterns, &subjects, limits, flags)
    {
        return Some(folded);
    }

    let flag = |bit: i64| match (&flags, &flags_text) {
        (Some(values), _) => FlagState::of_values(values, bit),
        (None, Some(text)) => flag_state(args, text, bit),
        (None, None) => FlagState::No,
    };
    let piece = if flag(PREG_SPLIT_NO_EMPTY) == FlagState::Yes {
        PhpType::non_empty_string()
    } else {
        PhpType::string()
    };
    let captured = PhpType::array_shape(vec![
        ShapeEntry {
            key: None,
            value_type: piece.clone(),
            optional: false,
        },
        ShapeEntry {
            key: None,
            value_type: PhpType::int_range("0", "max"),
            optional: false,
        },
    ]);
    let element = match flag(PREG_SPLIT_OFFSET_CAPTURE) {
        FlagState::Yes => captured,
        FlagState::No => piece,
        FlagState::Maybe => PhpType::union(vec![captured, PhpType::string()]),
    };
    let list = PhpType::list(element);
    // Splitting a non-empty subject always leaves a piece, unless the limit
    // or `PREG_SPLIT_NO_EMPTY` drops it: PHP keeps the last one either way.
    let list = if subject.is_subtype_of(&PhpType::non_empty_string()) {
        list.non_empty_array_form()
    } else {
        list
    };
    Some(PhpType::union(vec![list, PhpType::false_()]))
}

/// Every value of a limit or flags argument when it is a literal integer,
/// a numeric string, or a union of them: `Some(None)` when the argument is
/// an `int` of unknown value, `None` when it is not an integer at all.
fn int_values(args: &CallArgs<'_>, text: &str) -> Option<Option<Vec<i64>>> {
    if let Some(value) = crate::php_type::parse_php_int_literal(text.trim()) {
        return Some(Some(vec![value]));
    }
    let ty = args.resolve(text)?;
    let mut values = Vec::new();
    for member in ty.union_members() {
        let value = literal_int(member).or_else(|| {
            let content = member.as_literal()?.string_content()?;
            content.trim().parse::<i64>().ok()
        });
        match value {
            Some(value) => values.push(value),
            None if member.is_int_subtype() => return Some(None),
            None => return None,
        }
    }
    Some(Some(values))
}

fn literal_strings(ty: &PhpType) -> Option<Vec<String>> {
    ty.union_members()
        .into_iter()
        .map(|member| Some(member.as_literal()?.string_content()?.into_owned()))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlagState {
    Yes,
    No,
    Maybe,
}

impl FlagState {
    fn of_values(values: &[i64], bit: i64) -> Self {
        if values.iter().all(|v| v & bit != 0) {
            FlagState::Yes
        } else if values.iter().all(|v| v & bit == 0) {
            FlagState::No
        } else {
            FlagState::Maybe
        }
    }
}

/// Whether a flags expression that is not one known value sets `bit`: yes
/// when any `|`-operand always sets it, no when every operand is known and
/// none does.
fn flag_state(args: &CallArgs<'_>, text: &str, bit: i64) -> FlagState {
    let mut all_no = true;
    for term in split_bitwise_or(text) {
        match int_values(args, term).flatten() {
            Some(values) => match FlagState::of_values(&values, bit) {
                FlagState::Yes => return FlagState::Yes,
                FlagState::No => {}
                FlagState::Maybe => all_no = false,
            },
            None => all_no = false,
        }
    }
    if all_no {
        FlagState::No
    } else {
        FlagState::Maybe
    }
}

/// The result of every combination of literal arguments, or `None` when
/// any of them cannot be folded.
fn fold(
    patterns: &[String],
    subjects: &[String],
    limits: &[i64],
    flags: &[i64],
) -> Option<PhpType> {
    if patterns.len() * subjects.len() * limits.len() * flags.len() > MAX_FOLDED_CALLS {
        return None;
    }
    let mut results: Vec<PhpType> = Vec::new();
    for pattern in patterns {
        let regex = compile(pattern)?;
        for subject in subjects {
            for &limit in limits {
                for &flag in flags {
                    let result = split(&regex, subject, limit, flag)?;
                    if !results.contains(&result) {
                        results.push(result);
                    }
                }
            }
        }
    }
    results.push(PhpType::false_());
    Some(PhpType::union(results))
}

/// The pattern as a `regex` that matches what PCRE would, or `None` when
/// the two may disagree.
fn compile(pattern: &str) -> Option<regex::bytes::Regex> {
    let (body, modifiers) = crate::type_engine::regex_shape::split_delimiters(pattern)?;
    let mut prefix = String::from("(?");
    let mut unicode = false;
    for modifier in modifiers.trim_end().chars() {
        match modifier {
            'i' | 'm' | 's' => prefix.push(modifier),
            'u' => unicode = true,
            _ => return None,
        }
    }
    // Without `u`, PCRE matches bytes.
    prefix.push_str(if unicode { "u)" } else { "-u)" });
    // `$` also matches before a trailing newline in PCRE; `\v`, `\h` and
    // `\N` name classes there but characters (or nothing) in `regex`.
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'$' => return None,
            b'\\' => {
                if matches!(
                    bytes.get(i + 1),
                    Some(b'v' | b'h' | b'N' | b'V' | b'H' | b'R' | b'K')
                ) {
                    return None;
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    regex::bytes::Regex::new(&format!("{prefix}{body}")).ok()
}

/// `preg_split()` of one literal subject, following `php_pcre_split_impl`.
fn split(regex: &regex::bytes::Regex, subject: &str, limit: i64, flags: i64) -> Option<PhpType> {
    let no_empty = flags & PREG_SPLIT_NO_EMPTY != 0;
    let delim_capture = flags & PREG_SPLIT_DELIM_CAPTURE != 0;
    let offset_capture = flags & PREG_SPLIT_OFFSET_CAPTURE != 0;
    let mut remaining = if limit == 0 { -1 } else { limit };
    let bytes = subject.as_bytes();
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let mut last = 0;
    if remaining == -1 || remaining > 1 {
        for captures in regex.captures_iter(bytes) {
            let whole = captures.get(0)?;
            // Empty matches split between characters by rules this does not
            // model.
            if whole.is_empty() {
                return None;
            }
            if !no_empty || whole.start() != last {
                pieces.push((last, whole.start()));
                if remaining != -1 {
                    remaining -= 1;
                }
            }
            if delim_capture {
                let count = (1..captures.len())
                    .rev()
                    .find(|&i| captures.get(i).is_some())
                    .map_or(1, |i| i + 1);
                for i in 1..count {
                    // An unset group before the last set one has no offset.
                    let group = captures.get(i)?;
                    if !no_empty || !group.is_empty() {
                        pieces.push((group.start(), group.end()));
                    }
                }
            }
            last = whole.end();
            if pieces.len() > MAX_FOLDED_PIECES {
                return None;
            }
            if remaining != -1 && remaining <= 1 {
                break;
            }
        }
    }
    if !no_empty || last < bytes.len() {
        pieces.push((last, bytes.len()));
    }
    let entries = pieces
        .into_iter()
        .map(|(start, end)| {
            let text = std::str::from_utf8(&bytes[start..end]).ok()?;
            let piece = PhpType::literal_string_value(text);
            let value_type = if offset_capture {
                PhpType::array_shape(vec![
                    ShapeEntry {
                        key: None,
                        value_type: piece,
                        optional: false,
                    },
                    ShapeEntry {
                        key: None,
                        value_type: PhpType::literal_int(start.to_string()),
                        optional: false,
                    },
                ])
            } else {
                piece
            };
            Some(ShapeEntry {
                key: None,
                value_type,
                optional: false,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(PhpType::array_shape(entries))
}
