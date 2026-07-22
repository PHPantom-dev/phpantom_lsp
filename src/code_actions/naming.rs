//! Shared identifier-casing helpers for code action name generation.
//!
//! The extract/generate refactorings each need to turn an arbitrary
//! source fragment into a valid PHP identifier in a particular case
//! convention (camelCase for variables, PascalCase for accessor method
//! names, SCREAMING_SNAKE_CASE for constants). These transforms used to
//! be copied into each handler; they live here so a fix to one applies
//! to all.

/// Convert a string to camelCase, starting with a lowercase letter.
///
/// Snake_case input is folded on underscores; otherwise only the first
/// character is lowercased. Empty input yields `"variable"`.
pub(crate) fn to_camel_case(s: &str) -> String {
    if s.is_empty() {
        return "variable".to_string();
    }

    // If it contains underscores, treat as snake_case
    if s.contains('_') {
        return snake_to_camel(s);
    }

    // Just lowercase the first character
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    let mut result = first.to_lowercase().to_string();
    result.extend(chars);
    result
}

/// Convert `snake_case` to `camelCase`.
///
/// Empty input (or input consisting only of underscores) yields
/// `"variable"`.
pub(crate) fn snake_to_camel(s: &str) -> String {
    let parts: Vec<&str> = s.split('_').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return "variable".to_string();
    }

    let mut result = parts[0].to_lowercase();
    for part in &parts[1..] {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            result.push_str(&chars.as_str().to_lowercase());
        }
    }
    result
}

/// Convert a string to PascalCase.
///
/// `name` → `Name`, `first_name` → `FirstName`, `firstName` → `FirstName`.
pub(crate) fn to_pascal_case(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }

    // If it contains underscores, treat as snake_case.
    if name.contains('_') {
        return name
            .split('_')
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(c) => {
                        let upper: String = c.to_uppercase().collect();
                        format!("{}{}", upper, chars.as_str())
                    }
                    None => String::new(),
                }
            })
            .collect();
    }

    // Simple case: just capitalize the first letter.
    capitalise(name)
}

/// Convert a string to SCREAMING_SNAKE_CASE.
///
/// Non-alphanumeric characters become underscores. Consecutive
/// underscores are collapsed, and leading/trailing underscores trimmed.
pub(crate) fn string_to_screaming_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_uppercase());
        } else if (ch == '_' || ch == '-' || ch == ' ' || ch == '/' || ch == '.')
            && !result.ends_with('_')
        {
            result.push('_');
        }
        // Skip other characters (e.g. special symbols).
    }
    // Trim trailing underscore
    while result.ends_with('_') {
        result.pop();
    }
    // Trim leading underscore
    while result.starts_with('_') {
        result.remove(0);
    }
    result
}

/// Capitalise the first character of a string (ASCII-aware, Unicode-safe).
pub(crate) fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            format!("{}{}", upper, chars.as_str())
        }
        None => String::new(),
    }
}
