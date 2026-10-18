//! Plain-text parsing of the PHP literals a model's `$casts` property and
//! `casts()` method are written with.
//!
//! The cast entries are read from the source text of the array literal
//! rather than its AST, so `'decimal:2'`, `Address::class.':nullable'`
//! and `AsEnumCollection::of(Status::class)` all reduce to the cast string
//! Eloquent stores for them.

use crate::text_scan::{split_top_level_arrow, split_top_level_commas};
use crate::util::strip_fqn_prefix;

/// The trimmed, non-empty entries of a PHP array literal written as `[…]`.
///
/// Commas inside parentheses, brackets, braces, or quotes do not split an
/// entry, so `AsCollection::using(Custom::class, Item::class)` stays one
/// value. Text that is not an array literal yields nothing.
pub(super) fn array_literal_entries(text: &str) -> impl Iterator<Item = &str> {
    let inner = text
        .trim()
        .strip_prefix('[')
        .map(|s| s.strip_suffix(']').unwrap_or(s));
    inner
        .into_iter()
        .flat_map(split_top_level_commas)
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
}

/// Parse key-value pairs from a PHP array literal text.
///
/// Accepts text starting with `[` and extracts `'key' => 'value'` pairs.
/// Both single-quoted and double-quoted strings are supported for keys
/// and values.  Handles multi-line arrays and trailing commas.
///
/// Returns a list of `(key, value)` string pairs.
pub(super) fn parse_casts_array(text: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    for segment in array_literal_entries(text) {
        let Some((key_part, value_part)) = split_top_level_arrow(segment) else {
            continue;
        };
        let key = extract_string_literal(key_part.trim());
        let value = extract_cast_value(value_part.trim());

        if let (Some(k), Some(v)) = (key, value)
            && !k.is_empty()
            && !v.is_empty()
        {
            results.push((k, v));
        }
    }
    results
}

/// Extract the string content from a PHP string literal.
///
/// Strips surrounding quotes (single or double) and returns the inner
/// text.  Returns `None` if the text is not a quoted string.
///
/// Also handles:
/// - `SomeCast::class` — returns `"SomeCast"`
/// - `Address::class.':argument'` — strips the concatenated argument
///   suffix and returns `"Address"`
pub(super) fn extract_string_literal(text: &str) -> Option<String> {
    let t = text.trim();
    if ((t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"')))
        && t.len() >= 2
    {
        return Some(t[1..t.len() - 1].to_string());
    }
    // For class-string cast values like `SomeCast::class` or
    // `SomeCast::class.':argument'`, extract the class name.
    // The concatenation dot may have surrounding whitespace, so
    // look for `::class` and take everything before it.
    if let Some(class_pos) = t.find("::class") {
        let before = t[..class_pos].trim();
        let name = strip_fqn_prefix(before);
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// The cast type string a `$casts` / `casts()` value names.
///
/// Accepts the forms Eloquent evaluates before it stores a cast:
/// - a quoted string (`'boolean'`, `'decimal:2'`)
/// - `SomeCast::class`, including a concatenated argument
///   (`Address::class.':nullable'`, `AsEnumCollection::class.':'.Status::class`)
/// - a static call whose return value is that string
///   (`AsEnumCollection::of(Status::class)`, `AsCollection::using(Custom::class, Item::class)`,
///   `AsBinary::uuid()`)
///
/// A call is stored with its method kept (`AsEnumCollection::of:Status`) so
/// the type resolver can tell `of()` from `using()` after the class name has
/// been resolved. Arguments that are not a class constant or a string literal
/// are dropped: a variable there is not knowable statically.
fn extract_cast_value(text: &str) -> Option<String> {
    let t = text.trim().trim_end_matches(';').trim();
    if let Some(value) = extract_string_literal_quoted(t) {
        return Some(value);
    }
    if let Some(value) = extract_static_cast_call(t) {
        return Some(value);
    }
    extract_class_const_cast(t)
}

fn extract_string_literal_quoted(text: &str) -> Option<String> {
    if ((text.starts_with('\'') && text.ends_with('\''))
        || (text.starts_with('"') && text.ends_with('"')))
        && text.len() >= 2
    {
        return Some(text[1..text.len() - 1].to_string());
    }
    None
}

/// `Class::method(args)` as the cast string the method returns.
///
/// `::class` is a constant, not a call, and is left for
/// [`extract_class_const_cast`].
fn extract_static_cast_call(text: &str) -> Option<String> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open || !text[close + 1..].trim().is_empty() {
        return None;
    }
    let head = text[..open].trim();
    let sep = head.rfind("::")?;
    let class = strip_fqn_prefix(head[..sep].trim());
    let method = head[sep + 2..].trim();
    if method == "class" || !is_php_ident(method) || !is_php_class_name(class) {
        return None;
    }
    let args = parse_cast_call_args(&text[open + 1..close]);
    Some(super::casts::format_cast_reference(
        class,
        Some(method),
        &args,
    ))
}

fn parse_cast_call_args(inner: &str) -> Vec<String> {
    if inner.trim().is_empty() {
        return Vec::new();
    }
    split_top_level_commas(inner)
        .into_iter()
        .map(str::trim)
        .filter(|arg| !arg.is_empty())
        .map(|arg| extract_cast_call_arg(arg).unwrap_or_default())
        .collect()
}

fn extract_cast_call_arg(arg: &str) -> Option<String> {
    let arg = strip_named_argument(arg.trim());
    if let Some(value) = extract_string_literal_quoted(arg) {
        return Some(value);
    }
    if let Some(class_pos) = arg.find("::class") {
        let name = strip_fqn_prefix(arg[..class_pos].trim());
        if is_php_class_name(name) {
            return Some(name.to_string());
        }
    }
    None
}

/// `name: value` with a single colon, not the `::` of a class constant.
fn strip_named_argument(arg: &str) -> &str {
    if let Some(idx) = arg.find(':')
        && !arg[..idx].contains(':')
        && arg.as_bytes().get(idx + 1) != Some(&b':')
    {
        let name = arg[..idx].trim();
        if is_php_ident(name) {
            return arg[idx + 1..].trim();
        }
    }
    arg
}

/// `Class::class`, plus any `.'…'` / `.Other::class` concatenations after it.
///
/// `Address::class.':nullable'` is `Address:nullable`.
/// `AsEnumCollection::class.':'.Status::class` is `AsEnumCollection:Status`.
fn extract_class_const_cast(text: &str) -> Option<String> {
    let class_pos = text.find("::class")?;
    let name = strip_fqn_prefix(text[..class_pos].trim());
    if !is_php_class_name(name) {
        return None;
    }
    let mut result = name.to_string();
    let mut rest = text[class_pos + "::class".len()..].trim();
    while let Some(stripped) = rest.strip_prefix('.') {
        rest = stripped.trim();
        if let Some(quoted) = leading_quoted(rest) {
            if let Some(value) = extract_string_literal_quoted(quoted) {
                result.push_str(&value);
            }
            rest = rest[quoted.len()..].trim();
            continue;
        }
        let Some(next) = rest.find("::class") else {
            break;
        };
        let other = strip_fqn_prefix(rest[..next].trim());
        if !is_php_class_name(other) {
            break;
        }
        result.push_str(other);
        rest = rest[next + "::class".len()..].trim();
    }
    Some(result)
}

/// The quoted literal `rest` starts with, if it starts with one.
fn leading_quoted(rest: &str) -> Option<&str> {
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let mut chars = rest.char_indices().skip(1);
    while let Some((idx, c)) = chars.next() {
        if c == '\\' {
            chars.next();
            continue;
        }
        if c == quote {
            return Some(&rest[..=idx]);
        }
    }
    None
}

fn is_php_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_php_class_name(name: &str) -> bool {
    let name = name.strip_prefix('\\').unwrap_or(name);
    !name.is_empty()
        && !name.ends_with('\\')
        && !name.contains("\\\\")
        && name
            .split('\\')
            .all(|segment| !segment.is_empty() && is_php_ident(segment))
}
