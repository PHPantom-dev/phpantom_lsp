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
//! Folding only happens when it is exact.  Every transform here is
//! byte-oriented in PHP, and the case-mapping ones have been
//! locale-independent ASCII since PHP 8.2, so an argument that is not
//! pure ASCII declines rather than guessing at a Unicode mapping the
//! runtime would not perform.

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
    literal_type(&name, args).or_else(|| refined_type(&name, args))
}

/// The literal value a call to a string builtin produces, or `None`
/// when the call is not one of the folded functions or an argument it
/// reads is not a literal.
fn literal_type(name: &str, args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    // The length is an `int`, so it is folded apart from the transforms.
    // On the ASCII input that gets this far bytes and characters agree.
    if matches!(name, "strlen" | "mb_strlen") && !args.has_arg(1) {
        let mut lengths: Vec<PhpType> = Vec::new();
        for subject in literal_alternatives(args, 0)? {
            let length = PhpType::literal_int(subject.len().to_string());
            if !lengths.contains(&length) {
                lengths.push(length);
            }
        }
        return Some(match lengths.len() {
            1 => lengths.remove(0),
            _ => PhpType::union(lengths),
        });
    }

    let folded: Vec<String> = match name {
        // `mb_*` agrees with the ASCII mapping on ASCII input, which is
        // the only input that gets this far.
        "strtolower" | "mb_strtolower" => map_subject(args, |s| s.to_ascii_lowercase())?,
        "strtoupper" | "mb_strtoupper" => map_subject(args, |s| s.to_ascii_uppercase())?,
        "ucfirst" => map_subject(args, |s| change_first(s, u8::to_ascii_uppercase))?,
        "lcfirst" => map_subject(args, |s| change_first(s, u8::to_ascii_lowercase))?,
        "ucwords" if !args.has_arg(1) => map_subject(args, ucwords)?,
        "strrev" => map_subject(args, |s| s.chars().rev().collect())?,
        // The two-argument spellings take a character list that replaces
        // the default set, so only the default is folded here.
        "trim" if !args.has_arg(1) => map_subject(args, |s| trim_default(s, true, true))?,
        "ltrim" if !args.has_arg(1) => map_subject(args, |s| trim_default(s, true, false))?,
        "rtrim" | "chop" if !args.has_arg(1) => {
            map_subject(args, |s| trim_default(s, false, true))?
        }
        "str_repeat" => {
            let times = literal_int(args, 1)?;
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
    let emptiness = subject_emptiness(args);
    let non_empty = emptiness != Emptiness::Unknown;
    let cased = match (case, non_empty) {
        (Case::Lower, false) => Some(PhpType::lowercase_string()),
        (Case::Lower, true) => Some(PhpType::non_empty_lowercase_string()),
        (Case::Upper, false) => Some(PhpType::uppercase_string()),
        (Case::Upper, true) => Some(PhpType::non_empty_uppercase_string()),
        (Case::Unchanged, _) => None,
    };
    // The `non-empty-*case-string` keywords carry non-emptiness but not
    // the stronger facts, which ride along as an intersection.
    let stronger = match emptiness {
        Emptiness::Numeric => Some(PhpType::numeric_string()),
        Emptiness::NonFalsy => Some(PhpType::non_falsy_string()),
        Emptiness::NonEmpty => Some(PhpType::non_empty_string()),
        Emptiness::Unknown => None,
    };
    match (cased, stronger) {
        (Some(cased), Some(stronger)) if emptiness != Emptiness::NonEmpty => {
            Some(PhpType::intersection(vec![cased, stronger]))
        }
        (Some(cased), _) => Some(cased),
        (None, stronger) => stronger,
    }
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
    let literal = is_literal_string_type(&separator) && is_literal_string_type(&value);
    let emptiness = if !array.is_provably_non_empty() {
        None
    } else if value.is_subtype_of(&PhpType::non_falsy_string()) {
        Some(PhpType::non_falsy_string())
    } else if value.is_subtype_of(&PhpType::non_empty_string()) {
        Some(PhpType::non_empty_string())
    } else {
        None
    };
    match (literal, emptiness) {
        (false, emptiness) => emptiness,
        (true, None) => Some(PhpType::literal_string()),
        (true, Some(emptiness)) if emptiness == PhpType::non_empty_string() => {
            Some(PhpType::non_empty_literal_string())
        }
        (true, Some(emptiness)) => Some(PhpType::intersection(vec![
            PhpType::literal_string(),
            emptiness,
        ])),
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
        TypeKind::Literal(value) => {
            let content = value.string_content()?;
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

/// `ucwords` with its default delimiter set.
fn ucwords(s: &str) -> String {
    let mut bytes = s.as_bytes().to_vec();
    let mut at_word_start = true;
    for byte in &mut bytes {
        if at_word_start {
            *byte = byte.to_ascii_uppercase();
        }
        at_word_start = UCWORDS_DELIMITERS.contains(byte);
    }
    String::from_utf8(bytes).unwrap_or_else(|_| s.to_string())
}

/// The characters the `trim` family strips by default.
const TRIM_CHARACTERS: &[char] = &[' ', '\t', '\n', '\r', '\0', '\x0b'];

/// `trim` / `ltrim` / `rtrim` with their default character list.
fn trim_default(s: &str, left: bool, right: bool) -> String {
    let trimmed = match left {
        true => s.trim_start_matches(TRIM_CHARACTERS),
        false => s,
    };
    match right {
        true => trimmed.trim_end_matches(TRIM_CHARACTERS),
        false => trimmed,
    }
    .to_string()
}

/// Whether `func_name` is one of the string builtins
/// [`string_func_return_type`] has a rule for.
///
/// Callers use this to skip building an argument adapter for the calls
/// that could never be refined.
pub(in crate::type_engine) fn has_string_func_rule(func_name: &str) -> bool {
    matches!(
        func_name
            .trim_start_matches('\\')
            .to_ascii_lowercase()
            .as_str(),
        "strtolower"
            | "mb_strtolower"
            | "strtoupper"
            | "mb_strtoupper"
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
            | "mb_lcfirst"
            | "mb_ucfirst"
            | "sprintf"
            | "vsprintf"
            | "number_format"
            | "implode"
            | "join"
    )
}

#[cfg(test)]
#[path = "string_func_rules_tests.rs"]
mod tests;
