//! Extraction of type names from PHPStan return-type diagnostic
//! messages (`return.void` and `return.type`).

use crate::php_type::PhpType;

/// Message fragment that identifies a `return.void` diagnostic.
pub(super) const RETURN_VOID_MSG_SUFFIX: &str = "but should not return anything.";

/// Message fragment that identifies a `return.empty` diagnostic.
pub(super) const RETURN_EMPTY_MSG_FRAGMENT: &str = "but empty return statement found.";

/// Extract the actual return type from a `return.void` diagnostic
/// message.
///
/// Message format:
/// `{desc} with return type void returns {actual} but should not return anything.`
///
/// Returns the `{actual}` type as a `PhpType`, or `None` if the
/// message doesn't match.
pub(super) fn extract_actual_type(message: &str) -> Option<PhpType> {
    let marker = " returns ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    let end = rest.find(" but should not return anything.")?;
    let actual = rest[..end].trim();
    if actual.is_empty() {
        return None;
    }
    Some(PhpType::parse(actual))
}

/// Extract the actual return type from a `return.type` diagnostic
/// message.
///
/// Message format:
/// `{desc} should return {expected} but returns {actual}.`
///
/// Returns the `{actual}` type as a `PhpType`, or `None` if the
/// message doesn't match.
pub(super) fn extract_return_type_actual(message: &str) -> Option<PhpType> {
    let marker = " but returns ";
    let start = message.find(marker)? + marker.len();
    let rest = &message[start..];
    // Strip the trailing period.
    let actual = rest.strip_suffix('.')?.trim();
    if actual.is_empty() {
        return None;
    }
    Some(PhpType::parse(actual))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "message_parse_tests.rs"]
mod tests;
