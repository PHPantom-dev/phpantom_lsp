//! `date_create()`, `date_create_immutable()` and their `_from_format`
//! siblings over literal arguments: the date object when PHP accepts the
//! string, `false` when it rejects it. Mirrors PHPStan's
//! `DateTimeCreateDynamicReturnTypeExtension` and
//! `DateTimeDynamicReturnTypeExtension`, which run the call at analysis
//! time.
//!
//! PHP's free-form parser (`timelib`'s `strtotime()` grammar) is far too
//! large to port, so `date_create()` only recognises the spellings that are
//! certainly valid and leaves every other string undecided. The
//! `_from_format` variants follow `timelib_parse_from_format()` field by
//! field for the numeric and textual-name fields, which is enough to prove
//! a string either matches or does not; a format using any other field is
//! undecided.

use crate::php_type::PhpType;

use super::{CallArgs, literal_strings, without_false};

/// How many format and datetime pairs a call may try before it is left to
/// the declared type.
const MAX_PAIRS: usize = 16;

pub(super) fn return_type(
    args: &CallArgs<'_>,
    declared: &PhpType,
    from_format: bool,
) -> Option<PhpType> {
    let object = without_false(declared)?;
    let datetime = match args.text("$datetime") {
        Some(text) => args.resolve(&text)?,
        // `date_create()` defaults to `'now'`.
        None if !from_format => PhpType::literal_string_value("now"),
        None => return None,
    };
    let format = if from_format {
        Some(args.resolve(&args.text("$format")?)?)
    } else {
        None
    };
    created(object, format.as_ref(), &datetime)
}

/// What creating a date from `datetime` gives: `object` when PHP accepts
/// every string it may be, `false` when it accepts none, and both when it
/// accepts some. `format` is the `createFromFormat()` format, `None` for
/// the free-form parser.
///
/// `None` when an argument is not a literal or whether PHP accepts it is
/// undecided.
pub(crate) fn created(
    object: PhpType,
    format: Option<&PhpType>,
    datetime: &PhpType,
) -> Option<PhpType> {
    let datetimes = literal_strings(datetime)?;
    let formats = match format {
        Some(format) => Some(literal_strings(format)?),
        None => None,
    };
    let pairs = datetimes.len() * formats.as_ref().map_or(1, Vec::len);
    if pairs == 0 || pairs > MAX_PAIRS {
        return None;
    }
    let (mut valid, mut invalid) = (false, false);
    for datetime in &datetimes {
        let outcomes: Vec<Option<bool>> = match &formats {
            Some(formats) => formats
                .iter()
                .map(|format| parse_from_format(format.as_bytes(), datetime.as_bytes()))
                .collect(),
            None => vec![certainly_valid(datetime).then_some(true)],
        };
        for outcome in outcomes {
            match outcome? {
                true => valid = true,
                false => invalid = true,
            }
        }
    }
    Some(match (valid, invalid) {
        (true, false) => object,
        (false, true) => PhpType::false_(),
        _ => PhpType::union(vec![object, PhpType::false_()]),
    })
}

/// A spelling of a date PHP's free-form parser certainly accepts.
#[derive(Debug, PartialEq)]
enum Spelling {
    /// `now`, `today` and the other days and times named relative to now.
    Keyword,
    /// A signed or unsigned count of a unit: `+2 days`, `-1 week`.
    Relative,
    /// An ISO 8601 date in `year`, with an optional time and zone.
    Iso { year: u32 },
}

const KEYWORDS: [&str; 6] = ["now", "today", "tomorrow", "yesterday", "midnight", "noon"];

const UNITS: [&str; 10] = [
    "sec",
    "second",
    "min",
    "minute",
    "hour",
    "day",
    "week",
    "fortnight",
    "month",
    "year",
];

/// Whether `datetime` is one of the spellings `date_create()` certainly
/// accepts. The empty string is the current time there.
fn certainly_valid(datetime: &str) -> bool {
    datetime.trim().is_empty() || recognise(datetime).is_some()
}

/// Which spelling `datetime` is, when it is one this recognises: a keyword,
/// a relative offset, or an ISO 8601 date with an optional time and a `Z`,
/// numeric, `UTC` or `GMT` zone.
fn recognise(datetime: &str) -> Option<Spelling> {
    let s = datetime.trim();
    if KEYWORDS
        .iter()
        .any(|keyword| s.eq_ignore_ascii_case(keyword))
    {
        return Some(Spelling::Keyword);
    }
    if let Some((count, unit)) = s.split_once(' ') {
        let count = count.strip_prefix(['+', '-']).unwrap_or(count);
        let unit = unit.strip_suffix(['s', 'S']).unwrap_or(unit);
        if !count.is_empty()
            && count.bytes().all(|b| b.is_ascii_digit())
            && UNITS.iter().any(|u| unit.eq_ignore_ascii_case(u))
        {
            return Some(Spelling::Relative);
        }
    }
    let s = ["UTC", "GMT"]
        .iter()
        .find_map(|zone| {
            let cut = s.len().checked_sub(zone.len() + 1)?;
            (s.as_bytes()[cut] == b' ' && s[cut + 1..].eq_ignore_ascii_case(zone))
                .then(|| &s[..cut])
        })
        .unwrap_or(s);
    let b = s.as_bytes();
    let digits = |from: usize, count: usize| {
        b.len() >= from + count && b[from..from + count].iter().all(u8::is_ascii_digit)
    };
    let number = |from: usize, count: usize| -> u32 {
        std::str::from_utf8(&b[from..from + count])
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(u32::MAX)
    };
    // YYYY-MM-DD
    if !(digits(0, 4)
        && b.get(4) == Some(&b'-')
        && digits(5, 2)
        && b.get(7) == Some(&b'-')
        && digits(8, 2))
    {
        return None;
    }
    if !(1..=12).contains(&number(5, 2)) || !(1..=31).contains(&number(8, 2)) {
        return None;
    }
    let iso = Some(Spelling::Iso { year: number(0, 4) });
    let mut i = 10;
    if i == b.len() {
        return iso;
    }
    // [T ]HH:MM[:SS[.fraction]]
    if !matches!(b[i], b'T' | b't' | b' ') {
        return None;
    }
    i += 1;
    if !(digits(i, 2) && b.get(i + 2) == Some(&b':') && digits(i + 3, 2)) {
        return None;
    }
    if number(i, 2) > 23 || number(i + 3, 2) > 59 {
        return None;
    }
    i += 5;
    if b.get(i) == Some(&b':') {
        if !digits(i + 1, 2) || number(i + 1, 2) > 59 {
            return None;
        }
        i += 3;
        if b.get(i) == Some(&b'.') {
            let start = i + 1;
            i = start;
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            if i == start {
                return None;
            }
        }
    }
    // Z, or a ±HH:MM / ±HHMM offset.
    let zoned = match b.get(i) {
        None => true,
        Some(b'Z' | b'z') => i + 1 == b.len(),
        Some(b'+' | b'-') => {
            let rest = &b[i + 1..];
            (rest.len() == 5 && digits(i + 1, 2) && rest[2] == b':' && digits(i + 4, 2))
                || (rest.len() == 4 && digits(i + 1, 4))
        }
        _ => false,
    };
    if zoned { iso } else { None }
}

/// `strtotime($datetime, $baseTimestamp)` over a literal: an `int` for a
/// string PHP certainly accepts, positive when it is now or a date from
/// 1971 on, and `false` for the empty string. Any other string is
/// undecided.
pub(super) fn strtotime(datetime: &PhpType, has_base: bool) -> Option<PhpType> {
    let mut results: Vec<PhpType> = Vec::new();
    for datetime in literal_strings(datetime)? {
        let ty = if datetime.is_empty() {
            PhpType::false_()
        } else {
            match recognise(&datetime)? {
                // An offset can reach back before 1970, and so can anything
                // relative to a timestamp the caller chose.
                Spelling::Keyword if !has_base => PhpType::int_range("1", "max"),
                Spelling::Iso { year } if year > 1970 => PhpType::int_range("1", "max"),
                _ => PhpType::int(),
            }
        };
        if !results.contains(&ty) {
            results.push(ty);
        }
    }
    (!results.is_empty()).then(|| PhpType::union(results))
}

/// Run `timelib_parse_from_format()` over `datetime`: `Some(true)` when PHP
/// accepts it, `Some(false)` when it reports an error (which makes the call
/// return `false`), and `None` when the format uses a field this port does
/// not read.
fn parse_from_format(format: &[u8], datetime: &[u8]) -> Option<bool> {
    let mut f = 0;
    let mut p = 0;
    let mut allow_trailing = false;
    let mut have_hour = false;
    while f < format.len() {
        let c = format[f];
        f += 1;
        let ok = match c {
            b'd' | b'j' | b'm' | b'n' | b'y' | b'g' | b'h' | b'G' | b'H' => {
                if matches!(c, b'g' | b'h' | b'G' | b'H') {
                    have_hour = true;
                }
                number(datetime, &mut p, 2, 1)
            }
            // A signed year is read by rules this port does not follow.
            b'Y' if matches!(datetime.get(p), Some(b'-' | b'+')) => return None,
            b'Y' => number(datetime, &mut p, 4, 1),
            b'z' => number(datetime, &mut p, 3, 1),
            b'i' | b's' => number(datetime, &mut p, 2, 2),
            b'v' => number(datetime, &mut p, 3, 1),
            b'u' => number(datetime, &mut p, 6, 1),
            b'M' | b'F' => word(datetime, &mut p, &MONTHS),
            b'D' | b'l' => word(datetime, &mut p, &DAYS),
            // An English ordinal suffix is skipped when it is there.
            b'S' => {
                let suffix = datetime.get(p..p + 2).unwrap_or_default();
                if ["st", "nd", "rd", "th"]
                    .iter()
                    .any(|s| suffix.eq_ignore_ascii_case(s.as_bytes()))
                {
                    p += 2;
                }
                true
            }
            b'A' | b'a' => {
                if !have_hour {
                    return Some(false);
                }
                meridian(datetime, &mut p)
            }
            b'!' | b'|' => true,
            b'+' => {
                allow_trailing = true;
                true
            }
            b'#' => {
                let matched = datetime.get(p).is_some_and(|b| b";:/.,-()".contains(b));
                p += 1;
                matched
            }
            b'?' => {
                p += 1;
                p <= datetime.len()
            }
            b'\\' => {
                let &literal = format.get(f)?;
                f += 1;
                let matched = datetime.get(p) == Some(&literal);
                p += 1;
                matched
            }
            // Separators and spaces match themselves.
            b' ' | b'-' | b'/' | b'.' | b',' | b':' | b';' | b'(' | b')' | b'_' => {
                let matched = datetime.get(p) == Some(&c);
                p += 1;
                matched
            }
            _ => return None,
        };
        if !ok {
            return Some(false);
        }
    }
    Some(p == datetime.len() || (p < datetime.len() && allow_trailing))
}

/// Read a number of at most `max` digits, needing at least `min`. A field
/// that does not start with a digit is an error (`TIMELIB_CHECK_NUMBER`).
fn number(datetime: &[u8], p: &mut usize, max: usize, min: usize) -> bool {
    let start = *p;
    while *p < datetime.len() && *p - start < max && datetime[*p].is_ascii_digit() {
        *p += 1;
    }
    *p - start >= min.max(1)
}

const MONTHS: [&str; 24] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "jan",
    "feb",
    "mar",
    "apr",
    "may",
    "jun",
    "jul",
    "aug",
    "sep",
    "oct",
    "nov",
    "dec",
];

const DAYS: [&str; 14] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "mon",
    "tue",
    "wed",
    "thu",
    "fri",
    "sat",
    "sun",
];

/// Read a run of letters naming one of `names`, case-insensitively.
fn word(datetime: &[u8], p: &mut usize, names: &[&str]) -> bool {
    let start = *p;
    while *p < datetime.len() && datetime[*p].is_ascii_alphabetic() {
        *p += 1;
    }
    let read = &datetime[start..*p];
    names
        .iter()
        .any(|name| read.eq_ignore_ascii_case(name.as_bytes()))
}

/// Read `am`, `pm`, `a.m.` or `p.m.`, in any case.
fn meridian(datetime: &[u8], p: &mut usize) -> bool {
    let rest = &datetime[*p..];
    for spelling in ["a.m.", "p.m.", "am", "pm"] {
        if rest.len() >= spelling.len()
            && rest[..spelling.len()].eq_ignore_ascii_case(spelling.as_bytes())
        {
            *p += spelling.len();
            return true;
        }
    }
    false
}

#[cfg(test)]
#[path = "date_create_tests.rs"]
mod tests;
