//! Eloquent accessor detection and property name extraction.
//!
//! This module handles both legacy (`getXAttribute()`) and modern
//! (Laravel 9+ `Attribute` cast) accessor patterns, mapping method
//! signatures to virtual property names and types.

use crate::php_type::PhpType;
use crate::types::{ClassInfo, MethodInfo};

use super::helpers::camel_to_snake;

/// The fully-qualified name of the `Attribute` cast class used by
/// Laravel 9+ modern accessors/mutators.
const ATTRIBUTE_CAST_FQN: &str = "Illuminate\\Database\\Eloquent\\Casts\\Attribute";

/// Determine whether a method is a legacy Eloquent accessor.
///
/// Legacy accessors follow the `getXAttribute()` naming convention where
/// `X` starts with an uppercase letter.  For example,
/// `getFullNameAttribute()` is a legacy accessor that produces a virtual
/// property `$full_name`.
pub(super) fn is_legacy_accessor(method: &MethodInfo) -> bool {
    let name = &method.name;
    if !name.starts_with("get") || !name.ends_with("Attribute") {
        return false;
    }
    // Must have at least one character between "get" and "Attribute".
    // "getAttribute" itself (len 12) is a real Eloquent method, not an accessor.
    let middle = &name[3..name.len() - 9]; // strip "get" (3) and "Attribute" (9)
    if middle.is_empty() {
        return false;
    }
    // The first character of the middle portion must be uppercase.
    middle.starts_with(|c: char| c.is_uppercase())
}

pub(super) fn is_legacy_mutator(method: &MethodInfo) -> bool {
    let name = &method.name;
    if !name.starts_with("set") || !name.ends_with("Attribute") {
        return false;
    }
    let middle = &name[3..name.len() - 9];
    !middle.is_empty() && middle.starts_with(|c: char| c.is_uppercase())
}

/// Extract the virtual property name from a legacy accessor method name.
///
/// Strips `get` prefix and `Attribute` suffix, then converts the
/// remaining CamelCase portion to snake_case.
///
/// `getFullNameAttribute` → `full_name`
/// `getNameAttribute` → `name`
pub(super) fn legacy_accessor_property_name(method_name: &str) -> String {
    let middle = &method_name[3..method_name.len() - 9];
    camel_to_snake(middle)
}

pub(super) fn legacy_mutator_property_name(method_name: &str) -> String {
    let middle = &method_name[3..method_name.len() - 9];
    camel_to_snake(middle)
}

/// Determine whether a method is a modern Eloquent accessor (Laravel 9+).
///
/// Modern accessors are methods that return
/// `Illuminate\Database\Eloquent\Casts\Attribute`.  The method name
/// is in camelCase and the virtual property name is the snake_case
/// equivalent.  For example, `fullName(): Attribute` produces
/// `$full_name`.
pub(super) fn is_modern_accessor(method: &MethodInfo) -> bool {
    match method.return_type.as_ref() {
        Some(rt) => {
            if let Some(base) = rt.base_name() {
                base == ATTRIBUTE_CAST_FQN || base == "Attribute"
            } else {
                false
            }
        }
        None => false,
    }
}

/// Extract the get-type from a modern accessor's return type.
///
/// Given a return type like `Attribute<string>` or
/// `Attribute<string, never>`, returns the first generic argument
/// (`string` in both examples).  Falls back to `PhpType::mixed()` when
/// no generic parameter is present.
pub(super) fn extract_modern_accessor_type(method: &MethodInfo) -> PhpType {
    if let Some(rt) = method.return_type.as_ref()
        && let PhpType::Generic(_, args) = rt
        && let Some(first) = args.first()
        && !matches!(first, PhpType::Named(s) | PhpType::Raw(s) if s.is_empty())
    {
        return first.clone();
    }
    PhpType::mixed()
}

/// Check whether a method on `class` with the given `method_name` is
/// actually a legacy or modern Eloquent accessor.
///
/// Go-to-definition uses this after `accessor_method_candidates` finds a
/// matching method, to avoid jumping to a relationship or other method
/// that happens to share the camelCase name.  For example,
/// `masterRecipe()` returning `BelongsToMany` is NOT an accessor even
/// though `snake_to_camel("master_recipe")` produces `"masterRecipe"`.
pub(crate) fn is_accessor_or_mutator_method(class: &ClassInfo, method_name: &str) -> bool {
    let method = match class.get_method(method_name) {
        Some(m) => m,
        None => return false,
    };
    is_legacy_accessor(method) || is_legacy_mutator(method) || is_modern_accessor(method)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "accessors_tests.rs"]
mod tests;
