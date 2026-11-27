//! String builtins whose literal arguments decide a non-string answer.
//!
//! `version_compare()` over literal versions is one known ordering or one
//! known yes/no, and the multibyte functions that take an encoding name
//! either accept the name or throw a `ValueError` (PHP 8), so a literal
//! encoding decides whether the call returns at all.

use crate::php_type::PhpType;

use super::super::array_func_rules::ArrayFuncArgs;
use super::literal_alternatives_any;

/// The type a call to one of these builtins produces, or `None` when it is
/// not one of them or the arguments it reads are not literals.
pub(super) fn value_type(name: &str, args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    match name {
        "version_compare" => version_compare_type(args),
        "mb_http_output" | "mb_internal_encoding" => {
            encoding_setter_type(args, is_supported_encoding)
        }
        "mb_regex_encoding" => encoding_setter_type(args, is_regex_encoding),
        "mb_encoding_aliases" => {
            encoding_decides(args, 0, || PhpType::parse("list<non-falsy-string>"))
        }
        "mb_chr" => encoding_decides(args, 1, || PhpType::parse("string|false")),
        "mb_ord" => {
            // An empty string has no first character to read: PHP 8 throws.
            let empty = positional(args, 0)
                && literal_alternatives_any(args, 0)
                    .is_some_and(|s| s.iter().all(String::is_empty));
            if empty {
                return Some(PhpType::never());
            }
            encoding_decides(args, 1, || PhpType::parse("int|false"))
        }
        _ => None,
    }
}

/// The functions [`value_type`] has a rule for.
pub(super) const VALUE_FUNCTIONS: &[&str] = &[
    "version_compare",
    "mb_http_output",
    "mb_internal_encoding",
    "mb_regex_encoding",
    "mb_encoding_aliases",
    "mb_chr",
    "mb_ord",
];

/// Whether the argument at `index` is written positionally, so the seam's
/// slot is the parameter's.
fn positional(args: &dyn ArrayFuncArgs, index: usize) -> bool {
    args.has_arg(index) && !args.is_spread(index) && !args.is_named(index)
}

/// `version_compare()` over literal versions (and a literal operator).
fn version_compare_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if !positional(args, 0) || !positional(args, 1) {
        return None;
    }
    let left = literal_alternatives_any(args, 0)?;
    let right = literal_alternatives_any(args, 1)?;
    if !left.iter().chain(&right).all(|v| is_plain_version(v)) {
        return None;
    }
    let operators = if args.has_arg(2) {
        if !positional(args, 2) {
            return None;
        }
        Some(literal_alternatives_any(args, 2)?)
    } else {
        None
    };
    let mut results: Vec<PhpType> = Vec::new();
    let mut push = |ty: PhpType| {
        if !results.contains(&ty) {
            results.push(ty);
        }
    };
    for l in &left {
        for r in &right {
            let order = version_compare(l, r);
            match &operators {
                None => push(PhpType::literal_int(order.to_string())),
                Some(operators) => {
                    for op in operators {
                        // An unknown operator is a `ValueError`.
                        if let Some(answer) = apply_operator(op, order) {
                            push(if answer {
                                PhpType::true_()
                            } else {
                                PhpType::false_()
                            });
                        }
                    }
                }
            }
        }
    }
    Some(match results.len() {
        0 => PhpType::never(),
        _ => PhpType::union(results),
    })
}

fn apply_operator(op: &str, order: i32) -> Option<bool> {
    Some(match op {
        "<" | "lt" => order < 0,
        "<=" | "le" => order <= 0,
        ">" | "gt" => order > 0,
        ">=" | "ge" => order >= 0,
        "==" | "eq" => order == 0,
        "!=" | "<>" | "ne" => order != 0,
        _ => return None,
    })
}

/// Whether `version` is runs of letters and digits joined by single
/// separators (`.`, `-`, `_`, `+`), the form every real version takes.
/// Stray punctuation and empty parts are where `php_version_compare()`
/// gets idiosyncratic, so those are left unfolded.
pub(super) fn is_plain_version(version: &str) -> bool {
    let bytes = version.as_bytes();
    let is_sep = |b: u8| matches!(b, b'.' | b'-' | b'_' | b'+');
    bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || is_sep(b))
        && !bytes.windows(2).any(|w| is_sep(w[0]) && is_sep(w[1]))
}

/// PHP's `version_compare()` without an operator: `-1`, `0` or `1`.
///
/// A port of `php_version_compare()` from `ext/standard/versioning.c`.
pub(super) fn version_compare(v1: &str, v2: &str) -> i32 {
    if v1.is_empty() || v2.is_empty() {
        return match (v1.is_empty(), v2.is_empty()) {
            (true, true) => 0,
            (false, _) => 1,
            _ => -1,
        };
    }
    let c1 = if v1.starts_with('#') {
        v1.to_string()
    } else {
        canonicalize_version(v1)
    };
    let c2 = if v2.starts_with('#') {
        v2.to_string()
    } else {
        canonicalize_version(v2)
    };
    compare_canonical(c1.as_bytes(), c2.as_bytes())
}

fn compare_canonical(ver1: &[u8], ver2: &[u8]) -> i32 {
    // The C walks two cursors and nulls out each `.` it splits on; `n1`
    // and `n2` stand in for its "a `.` was found" pointers, which start out
    // non-null.
    let next_dot = |ver: &[u8], from: usize| {
        ver[from..]
            .iter()
            .position(|&b| b == b'.')
            .map(|i| from + i)
    };
    let (mut p1, mut p2) = (0usize, 0usize);
    let (mut n1, mut n2): (Option<usize>, Option<usize>) = (Some(0), Some(0));
    let mut compare = 0;
    while p1 < ver1.len() && p2 < ver2.len() && n1.is_some() && n2.is_some() {
        n1 = next_dot(ver1, p1);
        n2 = next_dot(ver2, p2);
        let part1 = &ver1[p1..n1.unwrap_or(ver1.len())];
        let part2 = &ver2[p2..n2.unwrap_or(ver2.len())];
        let d1 = part1.first().is_some_and(u8::is_ascii_digit);
        let d2 = part2.first().is_some_and(u8::is_ascii_digit);
        compare = match (d1, d2) {
            (true, true) => strtol(part1).cmp(&strtol(part2)) as i32,
            (false, false) => compare_special_forms(part1, part2),
            (true, false) => compare_special_forms(b"#N#", part2),
            (false, true) => compare_special_forms(part1, b"#N#"),
        };
        if compare != 0 {
            break;
        }
        if let Some(dot) = n1 {
            p1 = dot + 1;
        }
        if let Some(dot) = n2 {
            p2 = dot + 1;
        }
    }
    if compare == 0 {
        let rest1 = &ver1[p1.min(ver1.len())..];
        let rest2 = &ver2[p2.min(ver2.len())..];
        if n1.is_some() {
            compare = if rest1.first().is_some_and(u8::is_ascii_digit) {
                1
            } else {
                version_compare(&String::from_utf8_lossy(rest1), "#N#")
            };
        } else if n2.is_some() {
            compare = if rest2.first().is_some_and(u8::is_ascii_digit) {
                -1
            } else {
                version_compare("#N#", &String::from_utf8_lossy(rest2))
            };
        }
    }
    compare
}

/// C's `strtol()` over a run of leading digits, saturating like it does.
fn strtol(part: &[u8]) -> i64 {
    part.iter()
        .take_while(|b| b.is_ascii_digit())
        .fold(0i64, |acc, b| {
            acc.saturating_mul(10).saturating_add(i64::from(b - b'0'))
        })
}

fn compare_special_forms(form1: &[u8], form2: &[u8]) -> i32 {
    const FORMS: [(&[u8], i32); 10] = [
        (b"dev", 0),
        (b"alpha", 1),
        (b"a", 1),
        (b"beta", 2),
        (b"b", 2),
        (b"RC", 3),
        (b"rc", 3),
        (b"#", 4),
        (b"pl", 5),
        (b"p", 5),
    ];
    let order = |form: &[u8]| {
        FORMS
            .iter()
            .find(|(name, _)| form.starts_with(name))
            .map_or(-1, |&(_, order)| order)
    };
    (order(form1) - order(form2)).signum()
}

/// `php_canonicalize_version()`: `-`, `_` and `+` become `.`, a `.` goes
/// between a digit and a non-digit, and any other punctuation collapses
/// into a single `.`.
fn canonicalize_version(version: &str) -> String {
    let bytes = version.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() * 2);
    out.push(bytes[0]);
    let mut lp = bytes[0];
    let is_dig = |b: u8| b.is_ascii_digit();
    let is_ndig = |b: u8| !b.is_ascii_digit() && b != b'.';
    for &p in &bytes[1..] {
        let last = *out.last().unwrap_or(&0);
        if matches!(p, b'-' | b'_' | b'+') {
            if last != b'.' {
                out.push(b'.');
            }
        } else if (is_ndig(lp) && is_dig(p)) || (is_dig(lp) && is_ndig(p)) {
            if last != b'.' {
                out.push(b'.');
            }
            out.push(p);
        } else if !p.is_ascii_alphanumeric() {
            if last != b'.' {
                out.push(b'.');
            }
        } else {
            out.push(p);
        }
        lp = p;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `mb_http_output()`, `mb_internal_encoding()` and `mb_regex_encoding()`
/// handed an encoding: `true` once the setting took, and a `ValueError`
/// for a name the function does not accept. Without an argument the stub
/// patch's getter branch applies.
fn encoding_setter_type(args: &dyn ArrayFuncArgs, supported: fn(&str) -> bool) -> Option<PhpType> {
    if !positional(args, 0) || args.arg_raw_type(0)?.is_null() {
        return None;
    }
    let names = literal_alternatives_any(args, 0)?;
    Some(if names.iter().any(|name| supported(name)) {
        PhpType::true_()
    } else {
        PhpType::never()
    })
}

/// A function whose encoding argument at `index` must name a supported
/// encoding: `never` when no literal alternative does, else `None` so the
/// declared return stands.
fn encoding_decides(
    args: &dyn ArrayFuncArgs,
    index: usize,
    accepted: impl FnOnce() -> PhpType,
) -> Option<PhpType> {
    if !positional(args, index) {
        return None;
    }
    let names = literal_alternatives_any(args, index)?;
    Some(if names.iter().any(|name| is_supported_encoding(name)) {
        accepted()
    } else {
        PhpType::never()
    })
}

pub(super) fn is_supported_encoding(name: &str) -> bool {
    contains_ignore_case(SUPPORTED_ENCODINGS, name)
}

/// Whether mbstring reads every byte as one character in `name`, so a
/// string's length in it is its length in bytes.
pub(super) fn is_single_byte_encoding(name: &str) -> bool {
    contains_ignore_case(SINGLE_BYTE_ENCODINGS, name)
}

fn is_regex_encoding(name: &str) -> bool {
    contains_ignore_case(REGEX_ENCODINGS, name)
}

fn contains_ignore_case(table: &[&str], name: &str) -> bool {
    table
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

/// Every name `mb_list_encodings()` and `mb_encoding_aliases()` report on
/// PHP 8.5, upper-cased. mbstring matches names case-insensitively.
const SUPPORTED_ENCODINGS: &[&str] = &[
    "7BIT",
    "8BIT",
    "ANSI_X3.4-1968",
    "ANSI_X3.4-1986",
    "ARABIC",
    "ARMSCII-8",
    "ARMSCII8",
    "ASCII",
    "BASE64",
    "BIG-5",
    "BIG-FIVE",
    "BIGFIVE",
    "BINARY",
    "BYTE2BE",
    "BYTE2LE",
    "BYTE4BE",
    "BYTE4LE",
    "CN-BIG5",
    "CN-GB",
    "CP-1251",
    "CP-1254",
    "CP-850",
    "CP-866",
    "CP-936",
    "CP1251",
    "CP1252",
    "CP1254",
    "CP367",
    "CP50220",
    "CP50220-RAW",
    "CP50220RAW",
    "CP50221",
    "CP50222",
    "CP51932",
    "CP850",
    "CP866",
    "CP932",
    "CP936",
    "CP949",
    "CP950",
    "CSASCII",
    "CYRILLIC",
    "EUC",
    "EUC-CN",
    "EUC-JP",
    "EUC-JP-2004",
    "EUC-KR",
    "EUC-TW",
    "EUCCN",
    "EUCJP",
    "EUCJP-MS",
    "EUCJP-OPEN",
    "EUCJP-WIN",
    "EUCKR",
    "EUCTW",
    "EUC_CN",
    "EUC_JP",
    "EUC_JP-2004",
    "EUC_KR",
    "EUC_TW",
    "GB-18030",
    "GB-18030-2000",
    "GB18030",
    "GB18030-2022",
    "GB2312",
    "GBK",
    "GREEK",
    "HEBREW",
    "HTML",
    "HTML-ENTITIES",
    "HZ",
    "IBM-367",
    "IBM-850",
    "IBM-866",
    "IBM367",
    "IBM850",
    "IBM866",
    "ISO-10646-UCS-2",
    "ISO-10646-UCS-4",
    "ISO-2022-JP",
    "ISO-2022-JP-2004",
    "ISO-2022-JP-KDDI",
    "ISO-2022-JP-MOBILE#KDDI",
    "ISO-2022-JP-MS",
    "ISO-2022-KR",
    "ISO-8859-1",
    "ISO-8859-10",
    "ISO-8859-13",
    "ISO-8859-14",
    "ISO-8859-15",
    "ISO-8859-16",
    "ISO-8859-2",
    "ISO-8859-3",
    "ISO-8859-4",
    "ISO-8859-5",
    "ISO-8859-6",
    "ISO-8859-7",
    "ISO-8859-8",
    "ISO-8859-9",
    "ISO-IR-6",
    "ISO2022JPMS",
    "ISO646-US",
    "ISO8859-1",
    "ISO8859-10",
    "ISO8859-13",
    "ISO8859-14",
    "ISO8859-15",
    "ISO8859-16",
    "ISO8859-2",
    "ISO8859-3",
    "ISO8859-4",
    "ISO8859-5",
    "ISO8859-6",
    "ISO8859-7",
    "ISO8859-8",
    "ISO8859-9",
    "ISO_646.IRV:1991",
    "JIS",
    "JIS-MS",
    "KOI8-R",
    "KOI8-U",
    "KOI8R",
    "KOI8U",
    "LATIN1",
    "LATIN2",
    "LATIN3",
    "LATIN4",
    "LATIN5",
    "LATIN6",
    "LATIN8",
    "MACJAPANESE",
    "MS932",
    "MS_KANJI",
    "MUTF-7",
    "QPRINT",
    "QUOTED-PRINTABLE",
    "SHIFT-JIS",
    "SHIFT_JIS-2004",
    "SHIFT_JIS-IMODE",
    "SHIFT_JIS-KDDI",
    "SHIFT_JIS-SOFTBANK",
    "SJIS",
    "SJIS-2004",
    "SJIS-DOCOMO",
    "SJIS-KDDI",
    "SJIS-MAC",
    "SJIS-MOBILE#DOCOMO",
    "SJIS-MOBILE#KDDI",
    "SJIS-MOBILE#SOFTBANK",
    "SJIS-MS",
    "SJIS-OPEN",
    "SJIS-SOFTBANK",
    "SJIS-WIN",
    "SJIS2004",
    "UCS-2",
    "UCS-2BE",
    "UCS-2LE",
    "UCS-4",
    "UCS-4BE",
    "UCS-4LE",
    "UCS2",
    "UCS4",
    "UHC",
    "UNICODE",
    "US",
    "US-ASCII",
    "UTF-16",
    "UTF-16BE",
    "UTF-16LE",
    "UTF-32",
    "UTF-32BE",
    "UTF-32LE",
    "UTF-7",
    "UTF-8",
    "UTF-8-DOCOMO",
    "UTF-8-KDDI",
    "UTF-8-MOBILE#DOCOMO",
    "UTF-8-MOBILE#KDDI",
    "UTF-8-MOBILE#KDDI-A",
    "UTF-8-MOBILE#KDDI-B",
    "UTF-8-MOBILE#SOFTBANK",
    "UTF-8-SOFTBANK",
    "UTF16",
    "UTF32",
    "UTF7",
    "UTF7-IMAP",
    "UTF8",
    "UTF8-DOCOMO",
    "UTF8-KDDI",
    "UTF8-SOFTBANK",
    "UUENCODE",
    "WINDOWS-1251",
    "WINDOWS-1252",
    "WINDOWS-1254",
    "WINDOWS-31J",
    "X-EUC-CN",
    "X-EUC-JP",
    "X-EUC-KR",
    "X-EUC-TW",
    "X-MAC-JAPANESE",
    "X-SJIS",
    "X-SJIS-EMOJI-DOCOMO",
    "X-SJIS-EMOJI-KDDI",
    "X-SJIS-EMOJI-SOFTBANK",
];

/// The names in [`SUPPORTED_ENCODINGS`] of encodings that decode one
/// character per byte, whatever the byte (measured on PHP 8.5). The
/// transfer encodings (`HTML-ENTITIES`, `QUOTED-PRINTABLE`, `UUENCODE`,
/// `BASE64`) are left out: they decode an escape to one character.
const SINGLE_BYTE_ENCODINGS: &[&str] = &[
    "7BIT",
    "8BIT",
    "ANSI_X3.4-1968",
    "ANSI_X3.4-1986",
    "ARABIC",
    "ARMSCII-8",
    "ARMSCII8",
    "ASCII",
    "BINARY",
    "CP-1251",
    "CP-1254",
    "CP-850",
    "CP-866",
    "CP1251",
    "CP1252",
    "CP1254",
    "CP367",
    "CP850",
    "CP866",
    "CSASCII",
    "CYRILLIC",
    "GREEK",
    "HEBREW",
    "IBM-367",
    "IBM-850",
    "IBM-866",
    "IBM367",
    "IBM850",
    "IBM866",
    "ISO-8859-1",
    "ISO-8859-10",
    "ISO-8859-13",
    "ISO-8859-14",
    "ISO-8859-15",
    "ISO-8859-16",
    "ISO-8859-2",
    "ISO-8859-3",
    "ISO-8859-4",
    "ISO-8859-5",
    "ISO-8859-6",
    "ISO-8859-7",
    "ISO-8859-8",
    "ISO-8859-9",
    "ISO-IR-6",
    "ISO646-US",
    "ISO8859-1",
    "ISO8859-10",
    "ISO8859-13",
    "ISO8859-14",
    "ISO8859-15",
    "ISO8859-16",
    "ISO8859-2",
    "ISO8859-3",
    "ISO8859-4",
    "ISO8859-5",
    "ISO8859-6",
    "ISO8859-7",
    "ISO8859-8",
    "ISO8859-9",
    "ISO_646.IRV:1991",
    "KOI8-R",
    "KOI8-U",
    "KOI8R",
    "KOI8U",
    "LATIN1",
    "LATIN2",
    "LATIN3",
    "LATIN4",
    "LATIN5",
    "LATIN6",
    "LATIN8",
    "US",
    "US-ASCII",
    "WINDOWS-1251",
    "WINDOWS-1252",
    "WINDOWS-1254",
];

/// The subset of [`SUPPORTED_ENCODINGS`] that `mb_regex_encoding()`
/// accepts (the encodings Oniguruma can match in).
const REGEX_ENCODINGS: &[&str] = &[
    "ASCII",
    "BIG-5",
    "BIG-FIVE",
    "BIGFIVE",
    "CN-BIG5",
    "CP932",
    "EUC-CN",
    "EUC-JP",
    "EUC-KR",
    "EUC-TW",
    "EUCCN",
    "EUCJP",
    "EUCJP-WIN",
    "EUCKR",
    "EUCTW",
    "EUC_CN",
    "EUC_KR",
    "EUC_TW",
    "GB2312",
    "ISO-8859-1",
    "ISO-8859-10",
    "ISO-8859-13",
    "ISO-8859-14",
    "ISO-8859-15",
    "ISO-8859-16",
    "ISO-8859-2",
    "ISO-8859-3",
    "ISO-8859-4",
    "ISO-8859-5",
    "ISO-8859-6",
    "ISO-8859-7",
    "ISO-8859-8",
    "ISO-8859-9",
    "ISO8859-1",
    "ISO8859-10",
    "ISO8859-13",
    "ISO8859-14",
    "ISO8859-15",
    "ISO8859-16",
    "ISO8859-2",
    "ISO8859-3",
    "ISO8859-4",
    "ISO8859-5",
    "ISO8859-6",
    "ISO8859-7",
    "ISO8859-8",
    "ISO8859-9",
    "KOI8-R",
    "KOI8R",
    "MS932",
    "SJIS",
    "SJIS-WIN",
    "UCS-4",
    "UCS-4LE",
    "US-ASCII",
    "UTF-16",
    "UTF-16BE",
    "UTF-16LE",
    "UTF-32",
    "UTF-32BE",
    "UTF-32LE",
    "UTF-8",
    "UTF8",
    "WINDOWS-31J",
    "X-EUC-JP",
];
