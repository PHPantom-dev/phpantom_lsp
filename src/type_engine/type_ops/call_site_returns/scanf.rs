//! `sscanf()` and `fscanf()` without output variables: a list with one entry
//! per conversion in a literal format, each `null` when the input ran out
//! before it. Mirrors PHPStan's `SscanfFunctionDynamicReturnTypeExtension`.
//!
//! The conversions follow PHP's `ext/standard/scanf.c`. A string conversion
//! (`%s`, `%c`, `%[…]`) that reads nothing stops the scan instead of
//! assigning, so the strings it does assign are never empty.

use crate::php_type::{PhpType, ShapeEntry};

use super::{CallArgs, literal_strings};

/// `stream` is true for `fscanf()`, which also returns `false` at the end of
/// the file.
pub(super) fn return_type(args: &CallArgs<'_>, stream: bool) -> Option<PhpType> {
    // With output variables the call returns how many it assigned, which
    // the declared conditional already says.
    if args.args.len() > 2 {
        return None;
    }
    let formats = literal_strings(&args.resolve(&args.text("$format")?)?)?;
    let mut results: Vec<PhpType> = Vec::new();
    for format in &formats {
        let shape = PhpType::array_shape(
            conversions(format)?
                .into_iter()
                .map(|ty| ShapeEntry {
                    key: None,
                    optional: false,
                    value_type: PhpType::union(vec![ty, PhpType::null()]),
                })
                .collect(),
        );
        if !results.contains(&shape) {
            results.push(shape);
        }
    }
    if results.is_empty() {
        return None;
    }
    if stream {
        results.push(PhpType::false_());
    }
    results.push(PhpType::null());
    Some(PhpType::union(results))
}

/// The type each assigning conversion in `format` produces, in order, or
/// `None` for a format this does not read (positional `%n$` conversions, an
/// unknown conversion character, an unterminated `%[`).
fn conversions(format: &str) -> Option<Vec<PhpType>> {
    let bytes = format.as_bytes();
    let mut types = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        if bytes.get(i) == Some(&b'%') {
            i += 1;
            continue;
        }
        let suppressed = bytes.get(i) == Some(&b'*');
        if suppressed {
            i += 1;
        }
        let digits = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if bytes.get(i) == Some(&b'$') && i > digits {
            return None;
        }
        // Size modifiers are accepted and ignored.
        while matches!(bytes.get(i), Some(b'l' | b'L' | b'h')) {
            i += 1;
        }
        let ty = match bytes.get(i)? {
            b'd' | b'i' | b'o' | b'x' | b'X' | b'u' | b'n' => PhpType::int(),
            b'f' | b'e' | b'E' | b'g' => PhpType::float(),
            b's' | b'c' => PhpType::non_empty_string(),
            b'[' => {
                i += 1;
                // A `]` straight after the `[` (or `[^`) is part of the set.
                if bytes.get(i) == Some(&b'^') {
                    i += 1;
                }
                if bytes.get(i) == Some(&b']') {
                    i += 1;
                }
                i += bytes[i..].iter().position(|&b| b == b']')?;
                PhpType::non_empty_string()
            }
            _ => return None,
        };
        i += 1;
        if !suppressed {
            types.push(ty);
        }
    }
    Some(types)
}
