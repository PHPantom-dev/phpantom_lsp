//! Constant folding and refined return types for the string standard
//! library.
//!
//! A signature like `strtolower(string $string): string` describes the
//! widest result the function can produce, not the result *this* call
//! produces.  When every argument the transform reads is a literal the
//! answer is a literal too, and callers that check a value against a
//! literal union (`'class'|'interface'|'trait'`) depend on getting it:
//! `strtolower($kind)` over `'Class'|'Interface'|'Trait'` is
//! `'class'|'interface'|'trait'`, not `string`.
//!
//! When the arguments are not all literals the call can still be known
//! to be a refinement of `string`, and a caller that wants a
//! `lowercase-string` or a `non-empty-string` depends on getting that:
//! `strtolower($s)` is a `lowercase-string`, `sprintf('key-%s', $k)` is
//! non-empty, `number_format($x, 2, '.', '')` is a `numeric-string`.
//!
//! The rules live beside [`super::array_func_rules`] and read their
//! arguments through the same [`ArrayFuncArgs`] seam, so the AST walker
//! and the text-driven call resolver both reach them.
//!
//! Folding only happens when it is exact.  The single-byte transforms are
//! byte-oriented in PHP, and the case-mapping ones have been
//! locale-independent ASCII since PHP 8.2, so an argument that is not
//! pure ASCII declines rather than guessing at a Unicode mapping the
//! runtime would not perform.  The `mb_*` ones are folded over UTF-8
//! only, and only for the characters whose mapping PHP and Rust agree on.

use crate::php_type::{PhpType, TypeKind};

use super::array_func_rules::ArrayFuncArgs;

/// How many alternatives a folded union may carry.
///
/// The transforms are applied per alternative, so a wide union costs
/// one pass each and produces a union just as wide.  A subject with
/// more alternatives than this is a data table rather than a set of
/// cases a caller distinguishes, and the union's pairwise absorption is
/// quadratic in its member count.
const MAX_ALTERNATIVES: usize = 32;

/// The type a call to a string builtin produces when its arguments decide
/// more than the stub's `string`, or `None` when they do not.
///
/// Literal arguments fold to the literal result.  Otherwise the call may
/// still be known to be a refinement of `string`: `strtolower()` is a
/// `lowercase-string`, `sprintf('key-%s', $k)` is non-empty.
pub(in crate::type_engine) fn string_func_return_type(
    func_name: &str,
    args: &dyn ArrayFuncArgs,
) -> Option<PhpType> {
    // A fully-qualified call (`\strtolower($s)`) carries the leading
    // separator in its identifier text; the tables below are keyed on the
    // bare name.
    let name = func_name.trim_start_matches('\\').to_ascii_lowercase();
    values::value_type(&name, args)
        .or_else(|| literal_type(&name, args))
        .or_else(|| refined_type(&name, args))
}

/// The literal value a call to a string builtin produces, or `None`
/// when the call is not one of the folded functions or an argument it
/// reads is not a literal.
fn literal_type(name: &str, args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    match name {
        "strlen" if !args.has_arg(1) => return length_type(args, false),
        "mb_strlen" => return length_type(args, true),
        "str_increment" => return step_literals(args, true),
        "str_decrement" => return step_literals(args, false),
        _ => {}
    }

    let folded: Vec<String> = match name {
        "dirname" => {
            let levels = if args.has_arg(1) {
                literal_int(args, 1)?
            } else {
                1
            };
            if !(1..=64).contains(&levels) {
                return None;
            }
            literal_alternatives_any(args, 0)?
                .iter()
                .map(|subject| {
                    let mut path = subject.clone();
                    for _ in 0..levels {
                        let parent = dirname(&path, cfg!(windows));
                        if parent == path {
                            break;
                        }
                        path = parent;
                    }
                    path
                })
                .collect()
        }
        "strtolower" => map_subject(args, |s| s.to_ascii_lowercase())?,
        "strtoupper" => map_subject(args, |s| s.to_ascii_uppercase())?,
        "mb_strtolower" => mb_convert_subject(args, 1, &[MB_CASE_LOWER])?,
        "mb_strtoupper" => mb_convert_subject(args, 1, &[MB_CASE_UPPER])?,
        "mb_convert_case" => {
            let modes = mb_case_modes(args)?;
            mb_convert_subject(args, 2, &modes)?
        }
        "mb_substr" => mb_substr(args)?,
        "ucfirst" => map_subject(args, |s| change_first(s, u8::to_ascii_uppercase))?,
        "lcfirst" => map_subject(args, |s| change_first(s, u8::to_ascii_lowercase))?,
        "ucwords" if !args.has_arg(1) => map_subject(args, |s| ucwords(s, UCWORDS_DELIMITERS))?,
        "ucwords" => {
            let delimiters = single_literal(args, 1)?;
            map_subject(args, |s| ucwords(s, delimiters.as_bytes()))?
        }
        "strrev" => map_subject(args, |s| s.chars().rev().collect())?,
        "trim" | "ltrim" | "rtrim" | "chop" => {
            let (left, right) = match name {
                "trim" => (true, true),
                "ltrim" => (true, false),
                _ => (false, true),
            };
            if !args.has_arg(1) {
                map_subject(args, |s| trim_bytes(s, TRIM_CHARACTERS, left, right))?
            } else {
                let subjects = literal_alternatives(args, 0)?;
                let lists = literal_alternatives(args, 1)?;
                let mut folded = Vec::with_capacity(subjects.len() * lists.len());
                for list in &lists {
                    let characters = trim_character_list(list.as_bytes())?;
                    for subject in &subjects {
                        folded.push(trim_bytes(subject, &characters, left, right));
                    }
                }
                folded
            }
        }
        "str_repeat" => {
            let times = literal_int(args, 1)?;
            // Zero repetitions of anything is empty, whatever the subject.
            if times == 0 && args.has_arg(0) && !args.is_spread(0) {
                return Some(PhpType::literal_string_value(""));
            }
            // A negative count is a fatal error and a huge one is a
            // memory bomb; neither is worth a literal answer.
            if !(0..=1024).contains(&times) {
                return None;
            }
            map_subject(args, |s| s.repeat(times as usize))?
        }
        // `str_replace` also accepts arrays for the search and the
        // replacement, which pair up element-wise; only the plain
        // string-for-string spelling is folded.
        "str_replace" => {
            let search = single_literal(args, 0)?;
            let replace = single_literal(args, 1)?;
            if search.is_empty() {
                return None;
            }
            let subjects = literal_alternatives(args, 2)?;
            subjects
                .iter()
                .map(|s| s.replace(&search, &replace))
                .collect()
        }
        "sprintf" => vec![sprintf_fold(args)?],
        "implode" | "join" => implode_fold(args)?,
        _ => return None,
    };

    Some(union_of_literals(folded))
}

/// The refinement of `string` a call is known to return when its
/// arguments are not all literals.
fn refined_type(name: &str, args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    match name {
        "strtolower" | "mb_strtolower" => case_type(args, Case::Lower),
        "strtoupper" | "mb_strtoupper" => case_type(args, Case::Upper),
        // Recasing one character keeps the case the rest already had.
        "lcfirst" | "mb_lcfirst" => {
            let case = match is_subject(args, PhpType::lowercase_string()) {
                true => Case::Lower,
                false => Case::Unchanged,
            };
            case_type(args, case)
        }
        "ucfirst" | "mb_ucfirst" => {
            let case = match is_subject(args, PhpType::uppercase_string()) {
                true => Case::Upper,
                false => Case::Unchanged,
            };
            case_type(args, case)
        }
        "sprintf" => sprintf_type(args, true),
        "vsprintf" => sprintf_type(args, false),
        "number_format" => number_format_type(args),
        "implode" | "join" => implode_type(args),
        // An increment never produces `'0'`; a decrement can (`'1'`).
        "str_increment" if args.has_arg(0) && !args.is_spread(0) => {
            Some(PhpType::non_falsy_string())
        }
        "str_decrement" if args.has_arg(0) && !args.is_spread(0) => {
            Some(PhpType::non_empty_string())
        }
        _ => None,
    }
}

/// The case a case-mapping call leaves its whole result in.
#[derive(Clone, Copy, PartialEq)]
enum Case {
    Lower,
    Upper,
    Unchanged,
}

/// How far from empty a string is known to be, strongest first.
#[derive(Clone, Copy, PartialEq)]
enum Emptiness {
    Numeric,
    NonFalsy,
    NonEmpty,
    Unknown,
}

/// What the subject of a case mapping is known to be.  Case mapping
/// never changes a string's length, and the case of a numeric string's
/// exponent marker does not change whether it is numeric, so each of
/// these survives the call.
fn subject_emptiness(args: &dyn ArrayFuncArgs) -> Emptiness {
    // A constant's stub value describes the machine the stubs came from;
    // see `names_a_constant`.
    if names_a_constant(args, 0) {
        return Emptiness::Unknown;
    }
    let Some(ty) = args.arg_raw_type(0) else {
        return Emptiness::Unknown;
    };
    if ty.is_subtype_of(&PhpType::numeric_string()) {
        Emptiness::Numeric
    } else if ty.is_subtype_of(&PhpType::non_falsy_string()) {
        Emptiness::NonFalsy
    } else if ty.is_subtype_of(&PhpType::non_empty_string()) {
        Emptiness::NonEmpty
    } else {
        Emptiness::Unknown
    }
}

/// Whether the call's first argument is known to be a `refinement`.
fn is_subject(args: &dyn ArrayFuncArgs, refinement: PhpType) -> bool {
    !names_a_constant(args, 0)
        && args
            .arg_raw_type(0)
            .is_some_and(|ty| ty.is_subtype_of(&refinement))
}

/// `strtolower()` and friends: the case the call imposes, combined with
/// how far from empty its subject was.
fn case_type(args: &dyn ArrayFuncArgs, case: Case) -> Option<PhpType> {
    if !args.has_arg(0) || args.is_spread(0) {
        return None;
    }
    // How far from empty the subject was survives the mapping, as the
    // strongest of the facts that imply non-emptiness.
    let emptiness = match subject_emptiness(args) {
        Emptiness::Numeric => Some(PhpType::numeric_string()),
        Emptiness::NonFalsy => Some(PhpType::non_falsy_string()),
        Emptiness::NonEmpty => Some(PhpType::non_empty_string()),
        Emptiness::Unknown => None,
    };
    // Members in PHPStan's (alphabetical) order, so the intersection reads
    // the way it does there.
    let members = match case {
        Case::Lower => [Some(PhpType::lowercase_string()), emptiness],
        Case::Upper => [emptiness, Some(PhpType::uppercase_string())],
        Case::Unchanged => return emptiness,
    };
    let mut members: Vec<PhpType> = members.into_iter().flatten().collect();
    Some(if members.len() == 1 {
        members.pop().unwrap()
    } else {
        PhpType::intersection(members)
    })
}

/// `sprintf()` / `vsprintf()`: what the format string guarantees about
/// the result, whatever values fill its placeholders.
///
/// `sprintf()` also hands back its value's own type for a format that is
/// a single bare `%s`, and a `numeric-string` for a single numeric
/// conversion.  `vsprintf()` takes its values as an array, so only the
/// format is read.
fn sprintf_type(args: &dyn ArrayFuncArgs, values_follow: bool) -> Option<PhpType> {
    if args.is_spread(0) || args.is_named(0) {
        return None;
    }
    let formats = literal_alternatives_any(args, 0)?;
    let mut answers: Vec<PhpType> = Vec::with_capacity(formats.len());
    for format in &formats {
        let parsed = parse_format(format)?;
        let answer = match parsed.lone_placeholder {
            Some(Placeholder::BareString(position)) if values_follow => {
                sprintf_value_type(args, position)
                    .filter(|value| value.is_subtype_of(&PhpType::string()))?
            }
            Some(Placeholder::Numeric) => PhpType::numeric_string(),
            _ => parsed.emptiness_type()?,
        };
        if !answers.contains(&answer) {
            answers.push(answer);
        }
    }
    match answers.len() {
        1 => answers.pop(),
        // `'%s'|'%d'` over a `string` is `string|numeric-string`, which
        // is just `string`.
        _ => Some(PhpType::union(answers).simplified()),
    }
}

/// The type of the value `sprintf()` reads for its `position`th
/// placeholder (1-based, as PHP's `%1$s` counts).
fn sprintf_value_type(args: &dyn ArrayFuncArgs, position: usize) -> Option<PhpType> {
    // A spread anywhere before the value shifts which argument it is.
    if (0..=position).any(|index| args.is_spread(index) || args.is_named(index)) {
        return None;
    }
    if names_a_constant(args, position) {
        return None;
    }
    args.arg_raw_type(position)
}

/// A format string, reduced to what its result is known to hold.
struct Format {
    /// The shortest result any values can produce, in bytes.
    min_len: usize,
    /// Whether the text outside the placeholders has a character other
    /// than `0`.  The only non-empty falsy string is `"0"`, so such a
    /// character makes the whole result non-falsy.
    has_non_zero_text: bool,
    /// The format when it is nothing but one placeholder.
    lone_placeholder: Option<Placeholder>,
}

/// A format that is a single placeholder and nothing else.
#[derive(Clone, Copy)]
enum Placeholder {
    /// `%s` / `%2$s` with no flags, width, or precision: the result is
    /// the value at this (1-based) position itself.
    BareString(usize),
    /// A decimal, binary, octal or floating-point conversion (`%d`, `%u`,
    /// `%b`, `%o`, `%.2F`, `%e`, …) that no custom padding character can
    /// make non-numeric. Like PHPStan, a float conversion ignores that a
    /// non-finite value prints as `inf` or `nan`.
    Numeric,
    Other,
}

impl Format {
    fn emptiness_type(&self) -> Option<PhpType> {
        if self.min_len >= 2 || self.has_non_zero_text {
            Some(PhpType::non_falsy_string())
        } else if self.min_len == 1 {
            Some(PhpType::non_empty_string())
        } else {
            None
        }
    }
}

/// Parse a `printf`-family format, or `None` when it is malformed (PHP
/// throws a `ValueError` rather than returning a string).
///
/// The grammar is `%[argnum$][flags][width][.precision]specifier`, with
/// `%%` for a literal percent sign.
fn parse_format(format: &str) -> Option<Format> {
    let bytes = format.as_bytes();
    let mut min_len = 0usize;
    let mut has_non_zero_text = false;
    let mut placeholders = 0usize;
    let mut has_text = false;
    let mut last = Placeholder::Other;
    let mut next_position = 1usize;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            has_text = true;
            has_non_zero_text |= bytes[i] != b'0';
            min_len += 1;
            i += 1;
            continue;
        }
        i += 1;
        if bytes.get(i) == Some(&b'%') {
            has_text = true;
            has_non_zero_text = true;
            min_len += 1;
            i += 1;
            continue;
        }

        let start = i;
        let digits = |i: &mut usize| {
            let from = *i;
            while bytes.get(*i).is_some_and(u8::is_ascii_digit) {
                *i += 1;
            }
            format[from..*i].parse::<usize>().ok()
        };
        let mut position = None;
        let argnum = digits(&mut i);
        if argnum.is_some() && bytes.get(i) == Some(&b'$') {
            // `%0$s` is a `ValueError`.
            position = Some(argnum.filter(|&n| n > 0)?);
            i += 1;
        } else {
            i = start;
        }

        let mut custom_padding = false;
        let mut decorated = false;
        while let Some(&flag) = bytes.get(i) {
            match flag {
                b'-' | b'+' | b' ' | b'0' => i += 1,
                b'\'' => {
                    custom_padding = true;
                    i += 2;
                }
                _ => break,
            }
            decorated = true;
        }
        let width = match bytes.get(i) {
            Some(b'*') => {
                i += 1;
                decorated = true;
                0
            }
            _ => digits(&mut i).inspect(|_| decorated = true).unwrap_or(0),
        };
        if bytes.get(i) == Some(&b'.') {
            decorated = true;
            i += 1;
            match bytes.get(i) {
                Some(b'*') => i += 1,
                _ => {
                    digits(&mut i);
                }
            }
        }

        let specifier = *bytes.get(i)?;
        i += 1;
        min_len += match specifier {
            b's' => width,
            // `%c` writes exactly one byte and ignores the width.
            b'c' => 1,
            b'b' | b'd' | b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'h' | b'H' | b'o' | b'u'
            | b'x' | b'X' => width.max(1),
            _ => return None,
        };

        let position = position.unwrap_or_else(|| {
            next_position += 1;
            next_position - 1
        });
        placeholders += 1;
        last = match specifier {
            b's' if !decorated => Placeholder::BareString(position),
            b'd' | b'u' | b'b' | b'o' | b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'h' | b'H'
                if !custom_padding =>
            {
                Placeholder::Numeric
            }
            _ => Placeholder::Other,
        };
    }
    Some(Format {
        min_len,
        has_non_zero_text,
        lone_placeholder: (placeholders == 1 && !has_text).then_some(last),
    })
}

/// `strlen()` and `mb_strlen()`: the length of each alternative of the
/// subject as PHP converts it to a string.  A known value has a known
/// length (`strlen(123)` is `3`); otherwise the length is the range its
/// type allows, `int<1, max>` for a non-empty string or a number.
///
/// `mb_strlen()` counts characters only in UTF-8; in any other encoding a
/// known value still gets the range its emptiness allows.
fn length_type(args: &dyn ArrayFuncArgs, multibyte: bool) -> Option<PhpType> {
    use crate::type_engine::type_ops::int_interval::IntInterval;
    use crate::type_engine::variable::rhs_resolution::php_string_of_type;

    if !args.has_arg(0) || args.is_spread(0) || args.is_named(0) {
        return None;
    }
    let ty = args.arg_raw_type(0)?;
    let mut utf8 = !multibyte || is_utf8_encoding(args, 1);
    if multibyte && args.has_arg(1) && !args.is_spread(1) && !args.is_named(1) {
        // An encoding mbstring does not know is a `ValueError`, so only the
        // known alternatives can return.
        if let Some(names) = literal_alternatives_any(args, 1) {
            let known: Vec<&String> = names
                .iter()
                .filter(|name| values::is_supported_encoding(name))
                .collect();
            if known.is_empty() {
                return Some(PhpType::never());
            }
            utf8 = known.iter().all(|name| {
                name.eq_ignore_ascii_case("UTF-8") || name.eq_ignore_ascii_case("UTF8")
            });
        }
    }
    let read_values = !names_a_constant(args, 0) && utf8;
    let mut lengths: Vec<i64> = Vec::new();
    let mut ranges: Vec<IntInterval> = Vec::new();
    for member in ty.union_members() {
        if let Some(text) = read_values.then(|| php_string_of_type(member)).flatten() {
            let length = if multibyte {
                text.chars().count()
            } else {
                text.len()
            };
            let length = i64::try_from(length).ok()?;
            if !lengths.contains(&length) {
                lengths.push(length);
            }
            continue;
        }
        let (min, max) = if member.is_null() || member.is_false() {
            (Some(0), Some(0))
        } else if member.is_true() {
            (Some(1), Some(1))
        } else if member.is_bool() {
            (Some(0), Some(1))
        } else if member.is_subtype_of(&PhpType::non_empty_string())
            || member.is_int_subtype()
            || member.is_float_subtype()
        {
            (Some(1), None)
        } else {
            (Some(0), None)
        };
        ranges.push(IntInterval { min, max });
    }
    if ranges.is_empty() {
        lengths.sort_unstable();
        return Some(PhpType::union(
            lengths
                .into_iter()
                .map(|length| PhpType::literal_int(length.to_string()))
                .collect(),
        ));
    }
    ranges.extend(lengths.into_iter().map(|length| IntInterval {
        min: Some(length),
        max: Some(length),
    }));
    Some(IntInterval::hull_of(&ranges).into_type())
}

/// `number_format()`: a `numeric-string` when no thousands separator is
/// inserted and the decimal separator is one PHP reads back.
fn number_format_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if (0..4).any(|index| args.is_spread(index) || args.is_named(index)) {
        return None;
    }
    let no_thousands = single_literal_any(args, 3).is_some_and(|s| s.is_empty());
    let plain_decimal = match args.arg_raw_type(2) {
        Some(ty) if ty.is_null() => true,
        _ => single_literal_any(args, 2).is_some_and(|s| s.is_empty() || s == "."),
    };
    (no_thousands && plain_decimal).then(PhpType::numeric_string)
}

/// `implode()` / `join()`: written-out pieces joined by a written-out
/// separator are a `literal-string`, and an array that has entries joins
/// into a string at least as far from empty as each of them.
fn implode_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    use crate::type_engine::variable::rhs_resolution::is_literal_string_type;

    if (0..2).any(|index| args.is_spread(index) || args.is_named(index)) {
        return None;
    }
    let (separator, array) = if args.has_arg(1) {
        (args.arg_raw_type(0)?, args.arg_raw_type(1)?)
    } else {
        (PhpType::literal_string_value(""), args.arg_raw_type(0)?)
    };
    if !array.is_array_like() {
        return None;
    }
    let value = array.iterable_element_type()?;
    let mut joined = Piece {
        literal: is_literal_string_type(&separator),
        lowercase: separator.is_subtype_of(&PhpType::lowercase_string()),
        uppercase: separator.is_subtype_of(&PhpType::uppercase_string()),
        non_empty: true,
        non_falsy: true,
    };
    for member in value.union_members() {
        let piece = Piece::of(member);
        joined.literal &= piece.literal;
        joined.lowercase &= piece.lowercase;
        joined.uppercase &= piece.uppercase;
        joined.non_empty &= piece.non_empty;
        joined.non_falsy &= piece.non_falsy;
    }
    let non_empty_array = array.is_provably_non_empty();
    let mut parts = Vec::new();
    if joined.literal {
        parts.push(PhpType::literal_string());
    }
    if joined.lowercase {
        parts.push(PhpType::lowercase_string());
    }
    if joined.uppercase {
        parts.push(PhpType::uppercase_string());
    }
    if non_empty_array && joined.non_falsy {
        parts.push(PhpType::non_falsy_string());
    } else if non_empty_array && joined.non_empty {
        parts.push(PhpType::non_empty_string());
    }
    match parts.len() {
        0 => None,
        1 => parts.pop(),
        _ => Some(PhpType::intersection(parts)),
    }
}

/// What one `implode()` piece, as PHP converts it to a string, is known
/// to be.
struct Piece {
    literal: bool,
    lowercase: bool,
    uppercase: bool,
    non_empty: bool,
    non_falsy: bool,
}

impl Piece {
    fn of(member: &PhpType) -> Self {
        use crate::type_engine::type_ops::int_interval::IntInterval;
        use crate::type_engine::variable::rhs_resolution::is_literal_string_type;

        // An integer's digits have no case and are never empty; only `0`
        // spells the falsy `'0'`.
        if let Some(interval) = IntInterval::of_int(member) {
            return Self {
                literal: member.as_literal().is_some(),
                lowercase: true,
                uppercase: true,
                non_empty: true,
                non_falsy: interval.min.is_some_and(|min| min > 0)
                    || interval.max.is_some_and(|max| max < 0),
            };
        }
        Self {
            literal: is_literal_string_type(member),
            lowercase: member.is_subtype_of(&PhpType::lowercase_string()),
            uppercase: member.is_subtype_of(&PhpType::uppercase_string()),
            non_empty: member.is_subtype_of(&PhpType::non_empty_string()),
            non_falsy: member.is_subtype_of(&PhpType::non_falsy_string()),
        }
    }
}

/// `implode()` / `join()` of array shapes whose every value is a known
/// scalar, joined by a literal separator: each string the entries can join
/// into, an optional entry counted both present and absent.
fn implode_fold(args: &dyn ArrayFuncArgs) -> Option<Vec<String>> {
    use crate::type_engine::variable::rhs_resolution::php_string_of_type;

    if (0..2).any(|index| args.is_spread(index) || args.is_named(index)) {
        return None;
    }
    let (separator, array) = if args.has_arg(1) {
        (single_literal_any(args, 0)?, args.arg_raw_type(1)?)
    } else {
        (String::new(), args.arg_raw_type(0)?)
    };
    let mut joined: Vec<String> = Vec::new();
    for shape in array.union_members() {
        if !matches!(
            shape.kind(),
            TypeKind::ArrayShape(_) | TypeKind::ListShape(_)
        ) {
            return None;
        }
        // Each partial join, paired with whether it holds a piece yet (an
        // empty piece still takes a separator before the next one).
        let mut partial: Vec<(String, bool)> = vec![(String::new(), false)];
        for entry in shape.shape_entries()? {
            let mut pieces: Vec<String> = Vec::new();
            for value in entry.value_type.union_members() {
                if value.is_bool() && !value.is_true() && !value.is_false() {
                    pieces.extend(["1".to_string(), String::new()]);
                } else {
                    pieces.push(php_string_of_type(value)?);
                }
            }
            let mut next: Vec<(String, bool)> = Vec::new();
            if entry.optional {
                next.clone_from(&partial);
            }
            for (prefix, started) in &partial {
                for piece in &pieces {
                    let candidate = if *started {
                        (format!("{prefix}{separator}{piece}"), true)
                    } else {
                        (piece.clone(), true)
                    };
                    if !next.contains(&candidate) {
                        next.push(candidate);
                    }
                }
            }
            if next.len() > MAX_ALTERNATIVES {
                return None;
            }
            partial = next;
        }
        for (text, _) in partial {
            if !joined.contains(&text) {
                joined.push(text);
            }
        }
    }
    (!joined.is_empty() && joined.len() <= MAX_ALTERNATIVES).then_some(joined)
}

/// PHP removes path segments lexically: `a/./b` keeps `a/.` and
/// `a/../b` keeps `a/..`. `Path::parent` would normalize the dot away.
fn dirname(path: &str, windows: bool) -> String {
    if path.is_empty() {
        return String::new();
    }
    let drive_len = if windows
        && path.as_bytes().get(1) == Some(&b':')
        && path.as_bytes()[0].is_ascii_alphabetic()
    {
        2
    } else {
        0
    };
    let (drive, tail) = path.split_at(drive_len);
    if tail.is_empty() {
        return drive.to_string();
    }
    let separator = |ch| ch == '/' || (windows && ch == '\\');
    let tail = tail.trim_end_matches(separator);
    let parent = tail
        .rfind(separator)
        .map(|end| tail[..end].trim_end_matches(separator));
    let value = match parent {
        None if !tail.is_empty() => ".",
        None | Some("") => {
            if windows {
                "\\"
            } else {
                "/"
            }
        }
        Some(value) => value,
    };
    let result = format!("{drive}{value}");
    // PHP's Windows dirname drops the extended path prefix as well.
    if windows && let Some(short) = result.strip_prefix(r"\\?\") {
        short.to_string()
    } else {
        result
    }
}

/// Apply `transform` to every alternative of the call's first argument.
fn map_subject(
    args: &dyn ArrayFuncArgs,
    transform: impl Fn(&str) -> String,
) -> Option<Vec<String>> {
    let subjects = literal_alternatives(args, 0)?;
    Some(subjects.iter().map(|s| transform(s)).collect())
}

/// The literal string values the argument at `index` can hold.
///
/// A union answers with one entry per member; anything that is not a
/// string literal throughout (a bare `string`, a class, a literal int)
/// declines, because a partial answer would drop the alternatives it
/// could not fold.
fn literal_alternatives(args: &dyn ArrayFuncArgs, index: usize) -> Option<Vec<String>> {
    literal_alternatives_of(args, index, true)
}

/// [`literal_alternatives`] for a rule that reads the string rather than
/// transforming it, so a multibyte one is as good as any.
fn literal_alternatives_any(args: &dyn ArrayFuncArgs, index: usize) -> Option<Vec<String>> {
    literal_alternatives_of(args, index, false)
}

fn literal_alternatives_of(
    args: &dyn ArrayFuncArgs,
    index: usize,
    ascii_only: bool,
) -> Option<Vec<String>> {
    if names_a_constant(args, index) {
        return None;
    }
    let ty = args.arg_raw_type(index)?;
    let mut out = Vec::new();
    collect_literals(&ty, &mut out, ascii_only)?;
    if out.is_empty() || out.len() > MAX_ALTERNATIVES {
        return None;
    }
    Some(out)
}

/// Whether the argument at `index` is written as a bare constant name.
///
/// The stubs give `PHP_VERSION`, `PHP_OS`, `DIRECTORY_SEPARATOR` and
/// friends a literal value, but that value describes the machine the
/// stubs were generated on, not the one the code will run on. Folding a
/// builtin over one would turn an environment-dependent answer into a
/// literal the rest of the engine then treats as certain:
/// `str_replace(DIRECTORY_SEPARATOR, '/', $path)` must not decide what
/// the separator is. The declared type is the honest answer there, so
/// this declines and the call keeps it.
fn names_a_constant(args: &dyn ArrayFuncArgs, index: usize) -> bool {
    // The seam reports integer literals through the same accessor; a
    // constant name never starts with a digit.
    args.arg_atom_text(index)
        .is_some_and(|text| !text.starts_with(|c: char| c.is_ascii_digit()))
}

/// The single literal string the argument at `index` holds, or `None`
/// when it is a union or not a literal.
fn single_literal(args: &dyn ArrayFuncArgs, index: usize) -> Option<String> {
    only(literal_alternatives(args, index)?)
}

/// [`single_literal`] without the ASCII restriction.
fn single_literal_any(args: &dyn ArrayFuncArgs, index: usize) -> Option<String> {
    only(literal_alternatives_any(args, index)?)
}

fn only(mut alternatives: Vec<String>) -> Option<String> {
    match alternatives.len() {
        1 => alternatives.pop(),
        _ => None,
    }
}

/// The literal integer the argument at `index` holds.
fn literal_int(args: &dyn ArrayFuncArgs, index: usize) -> Option<i64> {
    match args.arg_raw_type(index)?.kind() {
        TypeKind::Literal(value) => value.parse_i64(),
        _ => None,
    }
}

/// Append every string literal `ty` can be, declining as a whole if any
/// alternative is something else.
fn collect_literals(ty: &PhpType, out: &mut Vec<String>, ascii_only: bool) -> Option<()> {
    match ty.kind() {
        _ if let Some(value) = ty.as_literal() => {
            let content = value.exact_string_content()?;
            // PHP's string functions are byte-oriented and the case
            // transforms map ASCII only; a multibyte subject would need
            // the runtime's own answer, not ours.
            if ascii_only && !content.is_ascii() {
                return None;
            }
            if !out.iter().any(|seen| seen == content.as_ref()) {
                out.push(content.into_owned());
            }
            Some(())
        }
        TypeKind::Union(members) => {
            for member in members {
                collect_literals(member, out, ascii_only)?;
            }
            Some(())
        }
        _ => None,
    }
}

/// The type describing one folded result, or the union of several.
fn union_of_literals(mut values: Vec<String>) -> PhpType {
    values.dedup();
    let mut members: Vec<PhpType> = Vec::with_capacity(values.len());
    for value in values {
        let ty = PhpType::literal_string_value(value);
        if !members.contains(&ty) {
            members.push(ty);
        }
    }
    match members.len() {
        1 => members.remove(0),
        _ => PhpType::union(members),
    }
}

/// `ucfirst` / `lcfirst`: recase the leading byte, leave the rest alone.
fn change_first(s: &str, recase: fn(&u8) -> u8) -> String {
    let mut bytes = s.as_bytes().to_vec();
    if let Some(first) = bytes.first_mut() {
        *first = recase(first);
    }
    String::from_utf8(bytes).unwrap_or_else(|_| s.to_string())
}

/// The characters `ucwords` treats as word separators by default.
const UCWORDS_DELIMITERS: &[u8] = b" \t\r\n\x0c\x0b";

/// `ucwords`: uppercase the first byte and every byte after one of
/// `delimiters`.
fn ucwords(s: &str, delimiters: &[u8]) -> String {
    let mut bytes = s.as_bytes().to_vec();
    let mut at_word_start = true;
    for byte in &mut bytes {
        if at_word_start {
            *byte = byte.to_ascii_uppercase();
        }
        at_word_start = delimiters.contains(byte);
    }
    String::from_utf8(bytes).unwrap_or_else(|_| s.to_string())
}

/// The characters the `trim` family strips by default.
const TRIM_CHARACTERS: &[u8] = b" \t\n\r\0\x0b";

/// `trim` / `ltrim` / `rtrim` stripping the bytes in `characters`.
fn trim_bytes(s: &str, characters: &[u8], left: bool, right: bool) -> String {
    let strip = |c: char| c.is_ascii() && characters.contains(&(c as u8));
    let trimmed = match left {
        true => s.trim_start_matches(strip),
        false => s,
    };
    match right {
        true => trimmed.trim_end_matches(strip),
        false => trimmed,
    }
    .to_string()
}

/// The bytes a `trim` character list names, expanding `a..z` ranges the
/// way PHP's `php_charmask()` does. `None` for a malformed range, which
/// PHP warns about and reads differently.
fn trim_character_list(list: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < list.len() {
        let c = list[i];
        if list.get(i + 1..i + 3) == Some(b"..") {
            let end = *list.get(i + 3)?;
            if end < c {
                return None;
            }
            out.extend(c..=end);
            i += 4;
        } else {
            out.push(c);
            i += 1;
        }
    }
    Some(out)
}

const MB_CASE_UPPER: i64 = 0;
const MB_CASE_LOWER: i64 = 1;
const MB_CASE_TITLE: i64 = 2;
const MB_CASE_FOLD: i64 = 3;
const MB_CASE_UPPER_SIMPLE: i64 = 4;
const MB_CASE_LOWER_SIMPLE: i64 = 5;
const MB_CASE_TITLE_SIMPLE: i64 = 6;
const MB_CASE_FOLD_SIMPLE: i64 = 7;

const MB_CASE_CONSTANTS: &[(&str, i64)] = &[
    ("MB_CASE_UPPER", MB_CASE_UPPER),
    ("MB_CASE_LOWER", MB_CASE_LOWER),
    ("MB_CASE_TITLE", MB_CASE_TITLE),
    ("MB_CASE_FOLD", MB_CASE_FOLD),
    ("MB_CASE_UPPER_SIMPLE", MB_CASE_UPPER_SIMPLE),
    ("MB_CASE_LOWER_SIMPLE", MB_CASE_LOWER_SIMPLE),
    ("MB_CASE_TITLE_SIMPLE", MB_CASE_TITLE_SIMPLE),
    ("MB_CASE_FOLD_SIMPLE", MB_CASE_FOLD_SIMPLE),
];

/// The `MB_CASE_*` modes `mb_convert_case()`'s second argument can be.
fn mb_case_modes(args: &dyn ArrayFuncArgs) -> Option<Vec<i64>> {
    if args.is_spread(1) || args.is_named(1) {
        return None;
    }
    if let Some(text) = args.arg_atom_text(1) {
        let bare = text.trim_start_matches('\\');
        if let Some(&(_, mode)) = MB_CASE_CONSTANTS.iter().find(|(name, _)| *name == bare) {
            return Some(vec![mode]);
        }
    }
    let ty = args.arg_raw_type(1)?;
    let mut modes = Vec::new();
    for member in ty.union_members() {
        let mode = match member.kind() {
            TypeKind::Literal(value) => value.parse_i64()?,
            TypeKind::Named(name) => {
                MB_CASE_CONSTANTS
                    .iter()
                    .find(|(constant, _)| *constant == name.trim_start_matches('\\'))?
                    .1
            }
            _ => return None,
        };
        if !modes.contains(&mode) {
            modes.push(mode);
        }
    }
    (!modes.is_empty()).then_some(modes)
}

/// Whether the encoding argument at `index` is absent or names UTF-8, the
/// only encoding the multibyte folds model.
fn is_utf8_encoding(args: &dyn ArrayFuncArgs, index: usize) -> bool {
    if !args.has_arg(index) {
        return true;
    }
    if args.is_spread(index) {
        return false;
    }
    if args.arg_raw_type(index).is_some_and(|ty| ty.is_null()) {
        return true;
    }
    single_literal(args, index).is_some_and(|encoding| {
        encoding.eq_ignore_ascii_case("UTF-8") || encoding.eq_ignore_ascii_case("UTF8")
    })
}

/// `mb_strtolower()`, `mb_strtoupper()` and `mb_convert_case()`: every
/// subject converted in every one of `modes`.
fn mb_convert_subject(
    args: &dyn ArrayFuncArgs,
    encoding_index: usize,
    modes: &[i64],
) -> Option<Vec<String>> {
    if !is_utf8_encoding(args, encoding_index) {
        return None;
    }
    let subjects = literal_alternatives_any(args, 0)?;
    let mut out = Vec::with_capacity(subjects.len() * modes.len());
    for subject in &subjects {
        for &mode in modes {
            out.push(mb_convert_case(subject, mode)?);
        }
    }
    Some(out)
}

/// Whether every case mapping mbstring offers (full and simple, upper,
/// lower, title and fold) agrees with Rust's full mapping for `c`.
///
/// That holds for ASCII and the Latin-1 and Latin Extended-A letters
/// apart from the few whose full mapping is more than one character or
/// whose fold differs from their lowercase.
fn has_plain_case_mapping(c: char) -> bool {
    c.is_ascii() || (('\u{C0}'..='\u{17F}').contains(&c) && !matches!(c, 'ß' | 'İ' | 'ŉ' | 'ſ'))
}

/// `mb_convert_case($s, $mode)` for UTF-8, or `None` when the string holds
/// characters this does not map exactly the way mbstring does.
///
/// Full upper- and lowercasing use Unicode's mapping for any character but
/// the Greek sigma, whose final form PHP only picks since 8.3. The other
/// modes are only folded over characters whose every mapping agrees.
fn mb_convert_case(s: &str, mode: i64) -> Option<String> {
    let plain = s.chars().all(has_plain_case_mapping);
    match mode {
        MB_CASE_UPPER | MB_CASE_LOWER if !s.contains(['Σ', 'σ', 'ς']) => Some(match mode {
            MB_CASE_UPPER => s.to_uppercase(),
            _ => s.to_lowercase(),
        }),
        MB_CASE_UPPER_SIMPLE if plain => Some(s.to_uppercase()),
        MB_CASE_LOWER_SIMPLE | MB_CASE_FOLD | MB_CASE_FOLD_SIMPLE if plain => {
            Some(s.to_lowercase())
        }
        MB_CASE_TITLE | MB_CASE_TITLE_SIMPLE if plain => {
            // A cased letter after a cased letter is inside a word and is
            // lowercased; any other is titlecased. A case-ignorable
            // character (an apostrophe, a period) does not end the word.
            let mut out = String::with_capacity(s.len());
            let mut in_word = false;
            for c in s.chars() {
                if in_word {
                    out.extend(c.to_lowercase());
                } else {
                    out.extend(c.to_uppercase());
                }
                if !matches!(c, '\'' | '.' | ':' | '^' | '`') {
                    in_word = c.is_lowercase() || c.is_uppercase();
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// `mb_substr()` of literal strings with a literal start and length,
/// counting characters.
fn mb_substr(args: &dyn ArrayFuncArgs) -> Option<Vec<String>> {
    if (0..4).any(|index| args.is_spread(index) || args.is_named(index)) {
        return None;
    }
    if !is_utf8_encoding(args, 3) {
        return None;
    }
    let start = literal_int(args, 1)?;
    let length = match args.arg_raw_type(2) {
        _ if !args.has_arg(2) => None,
        Some(ty) if ty.is_null() => None,
        _ => Some(literal_int(args, 2)?),
    };
    let subjects = literal_alternatives_any(args, 0)?;
    Some(
        subjects
            .iter()
            .map(|subject| {
                let chars: Vec<char> = subject.chars().collect();
                let count = chars.len() as i64;
                let from = if start < 0 {
                    (count + start).max(0)
                } else {
                    start.min(count)
                };
                let to = match length {
                    None => count,
                    Some(length) if length < 0 => (count + length).max(from),
                    Some(length) => from.saturating_add(length).min(count),
                };
                chars[from as usize..to as usize].iter().collect()
            })
            .collect(),
    )
}

/// `sprintf()` whose format and values are all single literals and whose
/// placeholders are plain `%s` and `%d`: the string it prints.
fn sprintf_fold(args: &dyn ArrayFuncArgs) -> Option<String> {
    if args.is_spread(0) || args.is_named(0) {
        return None;
    }
    let format = single_literal_any(args, 0)?;
    let bytes = format.as_bytes();
    let mut out = String::with_capacity(format.len());
    let mut next_position = 1usize;
    let mut text_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        out.push_str(&format[text_start..i]);
        i += 1;
        if bytes.get(i) == Some(&b'%') {
            out.push('%');
            i += 1;
            text_start = i;
            continue;
        }
        let digits_start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        let position = if i > digits_start {
            if bytes.get(i) != Some(&b'$') {
                return None;
            }
            let position = format[digits_start..i].parse::<usize>().ok()?;
            i += 1;
            (position > 0).then_some(position)?
        } else {
            next_position += 1;
            next_position - 1
        };
        let specifier = *bytes.get(i)?;
        i += 1;
        text_start = i;
        let value = sprintf_value_type(args, position)?;
        let literal = value.as_literal()?;
        match specifier {
            b's' => match literal.exact_string_content() {
                Some(content) => out.push_str(&content),
                None => out.push_str(&literal.parse_i64()?.to_string()),
            },
            b'd' => out.push_str(&literal.parse_i64()?.to_string()),
            _ => return None,
        }
    }
    out.push_str(&format[text_start..]);
    Some(out)
}

/// PHP's alphanumeric string increment (`'a'` → `'b'`, `'Az'` → `'Ba'`,
/// `'zz'` → `'aaa'`), or `None` for a string with any other character,
/// whose increment PHP deprecates.
pub(in crate::type_engine) fn increment_alphanumeric(s: &str) -> Option<String> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let mut bytes = s.as_bytes().to_vec();
    for i in (0..bytes.len()).rev() {
        let (wrapped, carry) = match bytes[i] {
            b'z' => (b'a', b'a'),
            b'Z' => (b'A', b'A'),
            b'9' => (b'0', b'1'),
            byte => (byte + 1, 0),
        };
        bytes[i] = wrapped;
        if carry == 0 {
            break;
        }
        // The carry ran off the front: prepend the first value of the
        // kind of character it came from.
        if i == 0 {
            bytes.insert(0, carry);
        }
    }
    String::from_utf8(bytes).ok()
}

/// What `str_decrement()` gives a string: `Some(None)` when PHP throws
/// for it (not alphanumeric, or nothing left to decrement, like `'a'` or
/// `'0'`), and `None` for a leading-zero string such as `'01'`, which
/// this does not try to answer.
fn decrement_alphanumeric(s: &str) -> Option<Option<String>> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Some(None);
    }
    if s.len() > 1 && s.starts_with('0') {
        return None;
    }
    let mut bytes = s.as_bytes().to_vec();
    let mut carry = true;
    for byte in bytes.iter_mut().rev() {
        (*byte, carry) = match *byte {
            b'0' => (b'9', true),
            b'a' => (b'z', true),
            b'A' => (b'Z', true),
            other => (other - 1, false),
        };
        if !carry {
            break;
        }
    }
    // A borrow off the front, or a first digit that reached zero, drops
    // the first character: `'aa'` is `'z'` and `'10'` is `'9'`.
    if carry || (bytes.len() > 1 && bytes[0] == b'0') {
        if bytes.len() == 1 {
            return Some(None);
        }
        bytes.remove(0);
    }
    Some(String::from_utf8(bytes).ok())
}

/// `str_increment()` and `str_decrement()` over literal subjects.  An
/// alternative PHP throws for contributes nothing, so a call where every
/// one throws is `never`.
fn step_literals(args: &dyn ArrayFuncArgs, increment: bool) -> Option<PhpType> {
    let mut folded = Vec::new();
    // Not ASCII-only: a multibyte subject is one PHP throws for.
    for subject in literal_alternatives_any(args, 0)? {
        let stepped = if increment {
            increment_alphanumeric(&subject)
        } else {
            decrement_alphanumeric(&subject)?
        };
        folded.extend(stepped);
    }
    Some(match folded.is_empty() {
        true => PhpType::never(),
        false => union_of_literals(folded),
    })
}

/// A string builtin must resolve to the global declaration, even when a
/// namespaced function or import shares its short name.
pub(in crate::type_engine) fn is_builtin_string_function(
    name: &str,
    loader: crate::types::FunctionLoader<'_>,
    offset: u32,
) -> bool {
    has_string_func_rule(name) && calls_global_function(name, loader, offset)
}

/// Whether a call written as `name` at `offset` reaches the global function
/// of that name rather than a namespaced function or import sharing it.
pub(in crate::type_engine) fn calls_global_function(
    name: &str,
    loader: crate::types::FunctionLoader<'_>,
    offset: u32,
) -> bool {
    loader
        .and_then(|loader| loader(name, offset))
        .is_none_or(|function| {
            function.namespace.as_deref().is_none_or(str::is_empty)
                && function
                    .name
                    .eq_ignore_ascii_case(name.trim_start_matches('\\'))
        })
}

/// Whether `func_name` is one of the string builtins
/// [`string_func_return_type`] has a rule for.
///
/// Callers use this to skip building an argument adapter for the calls
/// that could never be refined.
pub(in crate::type_engine) fn has_string_func_rule(func_name: &str) -> bool {
    let name = func_name.trim_start_matches('\\').to_ascii_lowercase();
    values::VALUE_FUNCTIONS.contains(&name.as_str())
        || matches!(
            name.as_str(),
            "strtolower"
                | "dirname"
                | "mb_strtolower"
                | "strtoupper"
                | "mb_strtoupper"
                | "mb_convert_case"
                | "mb_substr"
                | "ucfirst"
                | "lcfirst"
                | "ucwords"
                | "strrev"
                | "trim"
                | "ltrim"
                | "rtrim"
                | "chop"
                | "str_repeat"
                | "str_replace"
                | "strlen"
                | "mb_strlen"
                | "str_increment"
                | "str_decrement"
                | "mb_lcfirst"
                | "mb_ucfirst"
                | "sprintf"
                | "vsprintf"
                | "number_format"
                | "implode"
                | "join"
        )
}

#[path = "string_func_rules_values.rs"]
mod values;

#[cfg(test)]
#[path = "string_func_rules_tests.rs"]
mod tests;
