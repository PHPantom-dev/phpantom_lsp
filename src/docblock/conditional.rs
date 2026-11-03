//! PHPStan conditional return type parsing.
//!
//! This submodule handles annotations like:
//! ```text
//! @return ($abstract is class-string<TClass> ? TClass
//!           : ($abstract is null ? \Illuminate\Foundation\Application : mixed))
//! ```
//!
//! The main entry point is [`extract_conditional_return_type`], which
//! returns a type holding
//! [`TypeKind::Conditional`](crate::php_type::TypeKind::Conditional) nodes
//! that downstream code can evaluate at call-sites by matching the actual
//! argument types against the declared conditions.

use super::tag_kind::TagKind;

use crate::php_type::PhpType;

use super::parser::{DocblockInfo, collapse_newlines, parse_docblock_for_tags};

// ─── Public API ─────────────────────────────────────────────────────────────

/// Extract a PHPStan conditional return type from a `@return` tag.
///
/// Handles annotations like:
/// ```text
/// @return ($abstract is class-string<TClass> ? TClass
///           : ($abstract is null ? \Illuminate\Foundation\Application : mixed))
/// ```
///
/// A conditional need not be the whole type: one inside a union
/// (`false|($output is 'ARRAY_A' ? … : …)`), an intersection or a generic
/// argument is decided by the call's arguments all the same, so the whole
/// type is returned.
///
/// Returns `None` if the `@return` tag is missing or holds no conditional.
pub fn extract_conditional_return_type(docblock: &str) -> Option<PhpType> {
    extract_conditional_return_type_from_info(&parse_docblock_for_tags(docblock)?)
}

/// Like [`extract_conditional_return_type`], but operates on a pre-parsed [`DocblockInfo`].
pub fn extract_conditional_return_type_from_info(info: &DocblockInfo) -> Option<PhpType> {
    let raw = extract_raw_return_content_from_info(info)?;
    let trimmed = raw.trim();
    // Every conditional opens with `(`, so a type without one has none.
    if !trimmed.contains('(') {
        return None;
    }
    let parsed = PhpType::parse(trimmed);
    parsed.contains_conditional().then_some(parsed)
}

// ─── Internals ──────────────────────────────────────────────────────────────

/// Extract the raw content after `@return` from a pre-parsed docblock.
///
/// The parser already handles `/** */` stripping, leading `*` removal,
/// and multi-line tag continuation.  The value is returned with internal
/// `\n` normalised to spaces so that downstream parsers see a single-line
/// string.
fn extract_raw_return_content_from_info(info: &DocblockInfo) -> Option<String> {
    let tag = info.first_tag_by_kind_vendor_first(TagKind::Return)?;

    let desc = tag.description.trim();
    if desc.is_empty() {
        return None;
    }

    Some(collapse_newlines(desc))
}
