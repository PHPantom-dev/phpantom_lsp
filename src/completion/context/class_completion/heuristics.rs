//! Naming heuristics for [`super::ClassNameContext::likely_mismatch`].
//!
//! These demote (but never exclude) unloaded classes whose actual kind
//! cannot be verified, based on common PHP naming conventions
//! (`AbstractFoo`, `FooInterface`, `IFoo`, `FooTrait`, …).

/// Heuristic: the short name looks like it could be an attribute class.
///
/// Returns `true` when the name contains "Attribute" as a substring
/// (case-insensitive), or is one of the well-known built-in attributes
/// (`Override`, `Deprecated`, `SensitiveParameter`, etc.).
pub(super) fn likely_attribute_name(short_name: &str) -> bool {
    let lower = short_name.to_lowercase();
    if lower.contains("attribute") {
        return true;
    }
    // Well-known PHP built-in attributes that don't have "Attribute"
    // in their name.
    matches!(
        short_name,
        "Override"
            | "Deprecated"
            | "SensitiveParameter"
            | "AllowDynamicProperties"
            | "ReturnTypeWillChange"
    )
}

/// Heuristic: names that look like interfaces (`IFoo`, `FooInterface`).
pub(super) fn likely_interface_name(name: &str) -> bool {
    if name.starts_with('I') && name.len() > 1 {
        let second = name.chars().nth(1).unwrap();
        if second.is_uppercase() {
            return true;
        }
    }
    if name.ends_with("Interface") {
        return true;
    }
    false
}

/// Heuristic: names that positively look like non-interface types.
///
/// Used to demote unlikely interface candidates in `Implements` and
/// `ExtendsInterface` contexts. Only returns `true` when the name
/// matches a known non-interface naming pattern (Abstract*, *Abstract,
/// Base[A-Z]*). Names that don't match any pattern are left alone
/// (returns `false`).
pub(super) fn likely_non_interface_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("abstract") || lower.ends_with("abstract") {
        return true;
    }
    // `Base[A-Z]` prefix — e.g. `BaseController`, `BaseModel`.
    if name.starts_with("Base") && name.len() >= 5 {
        let fifth = name.as_bytes()[4];
        if fifth.is_ascii_uppercase() {
            return true;
        }
    }
    false
}

/// Heuristic: names that look like they cannot be instantiated.
///
/// Combines interface-like names, abstract-like names, and trait-like
/// names. Used to demote (but not exclude) class index/stub items in
/// `new` context.
pub(super) fn likely_non_instantiable(name: &str) -> bool {
    if likely_interface_name(name) {
        return true;
    }
    if name.starts_with("Abstract") {
        return true;
    }
    // `Base[A-Z]` prefix — e.g. `BaseController`, `BaseModel`.
    // `Baseline`, `Based`, etc. are NOT matched (5th char is lowercase).
    if name.starts_with("Base") && name.len() >= 5 {
        let fifth = name.as_bytes()[4];
        if fifth.is_ascii_uppercase() {
            return true;
        }
    }
    if name.ends_with("Abstract") || name.ends_with("Trait") {
        return true;
    }
    false
}
