//! Extraction of the native PHP `#[Attribute]` target bitmask.
//!
//! This is unrelated to Laravel's model attributes (`#[Connection]`,
//! `#[Table]`, etc., handled in
//! [`crate::virtual_members::laravel::model_extraction`]) — it is PHP's own
//! `Attribute::TARGET_*` mechanism for restricting where a user-defined
//! attribute class may be applied.

use mago_span::HasSpan;
use mago_syntax::cst::attribute::AttributeList;
use mago_syntax::cst::sequence::Sequence;

use crate::atom::last_segment;
use crate::types::attribute_target;

/// Extract the PHP attribute target bitmask from a class's attribute lists.
///
/// Scans for `#[\Attribute]` or `#[\Attribute(flags)]` and returns the
/// target bitmask.  Returns `0` when the class is not an attribute class.
///
/// Recognises these patterns:
/// - `#[Attribute]` / `#[\Attribute]` → `TARGET_ALL` (default)
/// - `#[Attribute(Attribute::TARGET_CLASS)]` → `TARGET_CLASS`
/// - `#[Attribute(Attribute::TARGET_CLASS | Attribute::TARGET_METHOD)]` → bitwise OR
/// - `#[Attribute(TARGET_CLASS | TARGET_METHOD)]` → short-form constants
/// - Numeric literals (e.g. `#[Attribute(1)]`, `#[Attribute(63)]`)
pub(super) fn extract_attribute_targets(
    attribute_lists: &Sequence<'_, AttributeList<'_>>,
    content: &str,
) -> u8 {
    for attr_list in attribute_lists.iter() {
        for attr in attr_list.attributes.iter() {
            let short = last_segment(attr.name.value());
            if short != b"Attribute" {
                continue;
            }

            // `#[\Attribute]` without arguments → TARGET_ALL.
            let Some(arg_list) = attr.argument_list.as_ref() else {
                return attribute_target::TARGET_ALL;
            };

            // No arguments inside parentheses → TARGET_ALL.
            let Some(first_arg) = arg_list.arguments.first() else {
                return attribute_target::TARGET_ALL;
            };

            // Extract the raw text of the first argument and parse
            // the target flags from it.
            let span = first_arg.span();
            let start = span.start.offset as usize;
            let end = span.end.offset as usize;
            let Some(text) = content.get(start..end) else {
                return attribute_target::TARGET_ALL;
            };

            return parse_attribute_target_flags(text);
        }
    }

    0
}

/// Parse a target-flag expression from the argument to `#[\Attribute(…)]`.
///
/// Handles `|`-separated lists of `Attribute::TARGET_*` or bare
/// `TARGET_*` constants, as well as plain integer literals.
fn parse_attribute_target_flags(text: &str) -> u8 {
    let text = text.trim();

    // Try plain integer literal first.
    if let Ok(n) = text.parse::<u8>() {
        return n;
    }

    let mut flags: u8 = 0;
    for part in text.split('|') {
        let part = part.trim();
        // Strip optional `Attribute::` or `self::` prefix.
        let constant = part
            .strip_prefix("Attribute::")
            .or_else(|| part.strip_prefix("\\Attribute::"))
            .or_else(|| part.strip_prefix("self::"))
            .unwrap_or(part);

        flags |= match constant {
            "TARGET_CLASS" => attribute_target::TARGET_CLASS,
            "TARGET_FUNCTION" => attribute_target::TARGET_FUNCTION,
            "TARGET_METHOD" => attribute_target::TARGET_METHOD,
            "TARGET_PROPERTY" => attribute_target::TARGET_PROPERTY,
            "TARGET_CLASS_CONSTANT" => attribute_target::TARGET_CLASS_CONSTANT,
            "TARGET_PARAMETER" => attribute_target::TARGET_PARAMETER,
            "TARGET_ALL" => attribute_target::TARGET_ALL,
            _ => {
                // Unrecognised constant — try parsing as an integer.
                constant.trim().parse::<u8>().unwrap_or_default()
            }
        };
    }

    // If we matched the `#[Attribute]` name but couldn't parse any
    // flags, default to TARGET_ALL.
    if flags == 0 {
        attribute_target::TARGET_ALL
    } else {
        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_class_qualified() {
        assert_eq!(
            parse_attribute_target_flags("\\Attribute::TARGET_CLASS"),
            attribute_target::TARGET_CLASS,
        );
    }

    #[test]
    fn parse_target_class_unqualified() {
        assert_eq!(
            parse_attribute_target_flags("Attribute::TARGET_CLASS"),
            attribute_target::TARGET_CLASS,
        );
    }

    #[test]
    fn parse_target_method_self() {
        assert_eq!(
            parse_attribute_target_flags("self::TARGET_METHOD"),
            attribute_target::TARGET_METHOD,
        );
    }

    #[test]
    fn parse_target_bare_constant() {
        assert_eq!(
            parse_attribute_target_flags("TARGET_PROPERTY"),
            attribute_target::TARGET_PROPERTY,
        );
    }

    #[test]
    fn parse_target_numeric_literal() {
        assert_eq!(parse_attribute_target_flags("1"), 1);
        assert_eq!(parse_attribute_target_flags("63"), 63);
    }

    #[test]
    fn parse_target_bitwise_or() {
        let expected = attribute_target::TARGET_CLASS | attribute_target::TARGET_METHOD;
        assert_eq!(
            parse_attribute_target_flags("\\Attribute::TARGET_CLASS | \\Attribute::TARGET_METHOD"),
            expected,
        );
    }

    #[test]
    fn parse_target_all() {
        assert_eq!(
            parse_attribute_target_flags("Attribute::TARGET_ALL"),
            attribute_target::TARGET_ALL,
        );
    }

    #[test]
    fn parse_target_unrecognised_defaults_to_all() {
        // Completely unrecognisable text falls back to TARGET_ALL
        // because the class IS marked with #[Attribute(...)].
        assert_eq!(
            parse_attribute_target_flags("SOME_UNKNOWN_CONST"),
            attribute_target::TARGET_ALL,
        );
    }

    #[test]
    fn parse_target_mixed_qualified_and_bare() {
        let expected = attribute_target::TARGET_FUNCTION | attribute_target::TARGET_PARAMETER;
        assert_eq!(
            parse_attribute_target_flags("Attribute::TARGET_FUNCTION | TARGET_PARAMETER"),
            expected,
        );
    }

    #[test]
    fn parse_target_whitespace_handling() {
        assert_eq!(
            parse_attribute_target_flags("  Attribute::TARGET_CLASS  "),
            attribute_target::TARGET_CLASS,
        );
    }
}
