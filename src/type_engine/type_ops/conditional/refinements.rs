//! Deciding a condition that names a refinement of its category: an integer
//! range, a refined string, an array's key and value types, and the
//! keywords that cut across categories (`scalar`, `numeric`, `empty`).
//!
//! Each answers the same three ways the parent module does: `Some(true)`
//! when every value of the argument's type satisfies the condition,
//! `Some(false)` when none does, and `None` when some may and some may not.

use crate::class_lookup::ResolvedClassLoader;
use crate::php_type::{
    LiteralValue, PhpType, TypeKind, array_like_key_value, parse_range_bound, refined_int_to_range,
    shape_key_type,
};

use super::arg_form::type_category;

/// The condition keywords that name values from more than one category,
/// which the category comparison would otherwise misread as a class.
pub(super) fn cross_category_keyword(condition: &PhpType) -> Option<&'static str> {
    let name = match condition.kind() {
        TypeKind::Named(name) => name.as_str(),
        // The type parser has no node for `empty`.
        TypeKind::Raw(text) => text,
        _ => return None,
    };
    match name {
        "scalar" => Some("scalar"),
        "numeric" => Some("numeric"),
        "empty" => Some("empty"),
        _ => None,
    }
}

/// Decide `is scalar`, `is numeric` or `is empty` against a single
/// (non-union) argument type.
pub(super) fn keyword_condition_result(arg: &PhpType, keyword: &str) -> Option<bool> {
    match keyword {
        "scalar" => match type_category(arg) {
            Some("string" | "int" | "float" | "bool") => Some(true),
            Some(_) => Some(false),
            None => {
                is_named_any(arg, &["scalar", "numeric", "number", "array-key"]).then_some(true)
            }
        },
        "numeric" => match type_category(arg) {
            Some("int" | "float") => Some(true),
            Some("string") => {
                if arg.is_subtype_of(&PhpType::numeric()) {
                    Some(true)
                } else if arg.is_string_literal() {
                    Some(false)
                } else {
                    None
                }
            }
            Some(_) => Some(false),
            None => is_named_any(arg, &["numeric", "number"]).then_some(true),
        },
        "empty" => value_truthiness(arg).map(|truthy| !truthy),
        _ => None,
    }
}

/// Whether `ty` is one of the keywords in `names`.
fn is_named_any(ty: &PhpType, names: &[&str]) -> bool {
    matches!(ty.kind(), TypeKind::Named(name) if names.contains(&name.as_str()))
}

/// Whether every value of `ty` is truthy, or none of them is, reading an
/// integer range and an intersection of string refinements by what they
/// hold.
fn value_truthiness(ty: &PhpType) -> Option<bool> {
    if let Some((lo, hi)) = int_bounds(ty) {
        return if (lo, hi) == (0, 0) {
            Some(false)
        } else if lo > 0 || hi < 0 {
            Some(true)
        } else {
            None
        };
    }
    match ty.kind() {
        // Each member narrows the same value, so the one that settles it
        // settles it for all.
        TypeKind::Intersection(members) => members.iter().find_map(value_truthiness),
        // A resource is always truthy, open or closed.
        TypeKind::Named(name)
            if matches!(
                name.as_str(),
                "resource" | "closed-resource" | "open-resource"
            ) =>
        {
            Some(true)
        }
        _ => ty.truthiness(),
    }
}

/// The inclusive bounds of the integers `ty` holds, for an integer literal,
/// range or refinement. `None` for anything else, and for `non-zero-int`,
/// which no single range describes.
fn int_bounds(ty: &PhpType) -> Option<(i64, i64)> {
    match ty.kind() {
        TypeKind::Literal(value) => value.parse_i64().map(|n| (n, n)),
        TypeKind::IntRange(min, max) => Some((parse_range_bound(min), parse_range_bound(max))),
        TypeKind::Named(name) => {
            let lower = name.to_ascii_lowercase();
            if matches!(lower.as_str(), "int" | "integer") {
                return Some((i64::MIN, i64::MAX));
            }
            let (min, max) = refined_int_to_range(&lower)?;
            Some((parse_range_bound(min), parse_range_bound(max)))
        }
        _ => None,
    }
}

/// Decide an integer condition (`is int<min, 0>`, `is positive-int`)
/// against an integer argument, by the ranges the two cover.
pub(super) fn int_condition_result(arg: &PhpType, condition: &PhpType) -> Option<bool> {
    if arg.is_subtype_of(condition) {
        return Some(true);
    }
    let (arg_lo, arg_hi) = int_bounds(arg)?;
    let (cond_lo, cond_hi) = int_bounds(condition)?;
    if cond_lo <= arg_lo && arg_hi <= cond_hi {
        Some(true)
    } else if arg_hi < cond_lo || cond_hi < arg_lo {
        Some(false)
    } else {
        None
    }
}

/// The string refinements a literal is checked against exactly: whether a
/// literal is one is read off its value alone.
///
/// `class-string` and `callable-string` are not among them, since whether a
/// literal names a class or a function depends on the code base.
///
/// Any two of them share a value (`'1'` is every one), so their
/// intersection is never empty.
pub(in crate::type_engine) fn is_value_refinement(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(name) => matches!(
            name.to_ascii_lowercase().as_str(),
            "non-empty-string"
                | "non-falsy-string"
                | "truthy-string"
                | "numeric-string"
                | "lowercase-string"
                | "uppercase-string"
                | "non-empty-lowercase-string"
                | "non-empty-uppercase-string"
                | "literal-string"
                | "non-empty-literal-string"
        ),
        TypeKind::Intersection(members) => members.iter().all(is_value_refinement),
        _ => false,
    }
}

/// Decide a string condition (`is non-falsy-string`, `is lowercase-string`)
/// against a string argument.
///
/// A literal argument is checked against the refinement's rule outright.
/// A refined argument holds the condition when its refinements imply it,
/// and otherwise may still hold a value that satisfies it.
pub(super) fn string_condition_result(arg: &PhpType, condition: &PhpType) -> Option<bool> {
    if condition.is_string_type() || arg.is_subtype_of(condition) {
        return Some(true);
    }
    if arg.is_string_literal() && is_value_refinement(condition) {
        return Some(false);
    }
    None
}

/// Whether no value of `arg` (an integer or a string type) can be the
/// literal `value`: an integer range that leaves it out, or a string
/// refinement it breaks (`''` is never a `non-empty-string`).
pub(super) fn excludes_literal(arg: &PhpType, value: &LiteralValue) -> bool {
    if let Some(n) = value.parse_i64() {
        return int_bounds(arg).is_some_and(|(lo, hi)| n < lo || hi < n);
    }
    if !matches!(value, LiteralValue::String(_)) {
        return false;
    }
    match arg.kind() {
        TypeKind::Intersection(members) => members.iter().any(|m| excludes_literal(m, value)),
        _ => is_value_refinement(arg) && !PhpType::literal(value.clone()).is_subtype_of(arg),
    }
}

/// Decide an `is array<K, V>` (or `list<V>`, `T[]`) condition against an
/// array argument, by its key and value types.
///
/// A shape is judged entry by entry, so one holding both an integer and a
/// string key is neither proven nor refuted by `array<int, mixed>`. An
/// empty array satisfies any of them. `None` when the condition names no
/// key or value type, for the caller to fall back on.
pub(super) fn array_condition_result(
    arg: &PhpType,
    condition: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Option<bool>> {
    let (cond_key, cond_value) = match condition.kind() {
        TypeKind::Generic(generic) => {
            let (key, value) = array_like_key_value(generic)?;
            (key, value.clone())
        }
        TypeKind::Array(value) => (PhpType::int(), value.clone()),
        _ => return None,
    };
    let part = |arg_part: &PhpType, cond_part: &PhpType| {
        if cond_part.is_mixed() || cond_part.is_array_key() || is_int_or_string(cond_part) {
            Some(true)
        } else {
            super::type_condition_result(arg_part, cond_part, class_loader)
        }
    };
    let entry =
        |key: &PhpType, value: &PhpType| both(part(key, &cond_key), part(value, &cond_value));

    if let Some(shape) = arg.shape_parts() {
        let mut results: Vec<Option<bool>> = Vec::with_capacity(shape.entries.len() + 1);
        let mut position = 0_i64;
        for e in shape.entries {
            let key = match e.key.as_deref() {
                Some(key) => shape_key_type(key),
                None => {
                    let key = PhpType::literal_int(position.to_string());
                    position += 1;
                    key
                }
            };
            results.push(entry(&key, &e.value_type));
        }
        if let Some((tail_key, tail_value)) = shape.tail {
            results.push(entry(tail_key, tail_value));
        }
        return Some(if results.iter().all(|r| *r == Some(true)) {
            Some(true)
        } else if results.iter().all(|r| *r == Some(false)) {
            Some(false)
        } else {
            None
        });
    }
    let (arg_key, arg_value) = match arg.kind() {
        TypeKind::Generic(generic) => {
            let (key, value) = array_like_key_value(generic)?;
            (key, value.clone())
        }
        TypeKind::Array(value) => (PhpType::int(), value.clone()),
        TypeKind::Named(name) if crate::php_type::is_list_name(name) => {
            (PhpType::int(), PhpType::mixed())
        }
        TypeKind::Named(_) => (
            PhpType::union(vec![PhpType::int(), PhpType::string()]),
            PhpType::mixed(),
        ),
        _ => return Some(None),
    };
    Some(entry(&arg_key, &arg_value))
}

/// `int|string`, the spelling `array-key` is normalised to.
fn is_int_or_string(ty: &PhpType) -> bool {
    matches!(ty.kind(), TypeKind::Union(members)
        if members.len() == 2 && members.iter().any(|m| m.is_int()) && members.iter().any(|m| m.is_string_type()))
}

/// Both of two checks: refuted by either, held only when both hold.
fn both(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}
