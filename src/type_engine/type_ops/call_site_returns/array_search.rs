//! `array_search()` over an array shape: the keys whose values the needle
//! may equal, and `false` unless one of them is certain to match first.
//! Mirrors PHPStan's `ArraySearchFunctionDynamicReturnTypeExtension`.
//!
//! The entries are searched in order, as PHP does. A needle that provably
//! equals a required entry ends the search there, a needle that provably
//! differs from an entry skips its key, and anything undecided keeps the key
//! and searches on.

use crate::php_type::{LiteralValue, PhpType, is_decimal_int_array_key, runtime_shape_keys};

use super::CallArgs;

pub(super) fn return_type(args: &CallArgs<'_>) -> Option<PhpType> {
    let haystack = args.resolve(&args.text("$haystack")?)?;
    let needle = args.resolve(&args.text("$needle")?)?;
    let strict = match args.text("$strict") {
        None => false,
        Some(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" => true,
            "false" => false,
            _ => return None,
        },
    };
    let mut results: Vec<PhpType> = Vec::new();
    let mut push = |ty: PhpType| {
        if !results.contains(&ty) {
            results.push(ty);
        }
    };
    for member in haystack.strip_benevolence().union_members() {
        let Some(parts) = member.shape_parts().filter(|parts| parts.tail.is_none()) else {
            // A generic array is left to the declared `TKey|false`, unless
            // a strict search is for a value of a type none of the values
            // has.
            let value = member.extract_value_type(false)?;
            if !(strict && compare(&needle, value, true) == Some(false)) {
                return None;
            }
            push(PhpType::false_());
            continue;
        };
        let keys = runtime_shape_keys(parts.entries)?;
        // A class-constant key is stored as its spelling, not its value.
        if keys.iter().any(|key| key.contains("::")) {
            return None;
        }
        let mut found = false;
        for (entry, key) in parts.entries.iter().zip(&keys) {
            let matches = compare(&needle, &entry.value_type, strict);
            if matches == Some(false) {
                continue;
            }
            push(if is_decimal_int_array_key(key) {
                PhpType::literal_int(key.as_str())
            } else {
                PhpType::literal_string_value(key)
            });
            if matches == Some(true) && !entry.optional {
                found = true;
                break;
            }
        }
        if !found {
            push(PhpType::false_());
        }
    }
    if results.is_empty() {
        return None;
    }
    Some(PhpType::union(results))
}

/// A scalar a type is exactly.
#[derive(PartialEq)]
enum Scalar {
    Int(i64),
    Str(String),
    Bool(bool),
    Null,
}

impl Scalar {
    fn of(ty: &PhpType) -> Option<Self> {
        if ty.is_null() {
            return Some(Self::Null);
        }
        if ty.is_true() {
            return Some(Self::Bool(true));
        }
        if ty.is_false() {
            return Some(Self::Bool(false));
        }
        match ty.as_literal()? {
            LiteralValue::Int(raw) => Some(Self::Int(crate::php_type::parse_php_int_literal(raw)?)),
            literal @ LiteralValue::String(_) => {
                Some(Self::Str(literal.string_content()?.into_owned()))
            }
            LiteralValue::Float(_) => None,
        }
    }
}

/// Whether `needle` equals `value`: `Some(true)` when it always does,
/// `Some(false)` when it never does, `None` when that is not decided.
fn compare(needle: &PhpType, value: &PhpType, strict: bool) -> Option<bool> {
    if let (Some(a), Some(b)) = (Scalar::of(needle), Scalar::of(value)) {
        if a == b {
            return Some(true);
        }
        if strict {
            return Some(false);
        }
        // Loosely, two different values of one type only differ for
        // integers and for strings neither of which is numeric.
        return match (&a, &b) {
            (Scalar::Int(_), Scalar::Int(_)) => Some(false),
            (Scalar::Str(x), Scalar::Str(y)) if !is_numeric(x) && !is_numeric(y) => Some(false),
            _ => None,
        };
    }
    if strict {
        let (a, b) = (kinds(needle)?, kinds(value)?);
        if a & b == 0 {
            return Some(false);
        }
    }
    None
}

/// Whether PHP may read `s` as a number. Rust also accepts `inf` and `NaN`,
/// which only leaves such a comparison undecided.
fn is_numeric(s: &str) -> bool {
    const WHITESPACE: [char; 6] = [' ', '\t', '\n', '\r', '\x0b', '\x0c'];
    s.trim_matches(WHITESPACE).parse::<f64>().is_ok()
}

const INT: u8 = 1;
const FLOAT: u8 = 2;
const STRING: u8 = 4;
const BOOL: u8 = 8;
const NULL: u8 = 16;
const ARRAY: u8 = 32;
const OBJECT: u8 = 64;

/// The runtime types `ty` may be, or `None` when that is not known.
fn kinds(ty: &PhpType) -> Option<u8> {
    let mut kinds = 0;
    for member in ty.union_members() {
        if member.is_mixed() {
            return None;
        }
        kinds |= if member.is_null() {
            NULL
        } else if member.is_bool() || member.is_true() || member.is_false() {
            BOOL
        } else if member.is_int_subtype() {
            INT
        } else if member.is_float_subtype() {
            FLOAT
        } else if member.is_string_subtype() {
            STRING
        } else if member.is_array_like() {
            ARRAY
        } else if member.is_object_like() {
            OBJECT
        } else {
            return None;
        };
    }
    Some(kinds)
}
