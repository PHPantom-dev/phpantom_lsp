//! Reading a type off the text of one operand expression.
//!
//! These helpers answer from the source text alone, without resolving
//! anything: whether an expression is a single operand, where its
//! top-level `??` / `?:` / `.` operators split it, and what a cast
//! prefix names. The parent module resolves the operands they hand back.

use crate::atom::atom;
use crate::php_type::PhpType;
use crate::text_scan::{ScanStep, scan_top_level};

/// The type a cast expression produces, or `None` when the text is not a
/// cast.
///
/// A cast names its own result whatever its operand turns out to be, which is
/// what makes it readable from the source text alone: `(string) $customer->id`
/// is a `string` without resolving the property. `(array)` promises an array
/// but says nothing about its contents, so it stays bare.
///
/// Only a cast applied to a single operand answers. A cast that is one side of
/// a larger expression (`(int) $a / 2`, `(int) $a === $b`) has the operator's
/// result type, not the cast's, so those are left to the caller's other paths
/// rather than answered wrongly.
pub(in crate::type_engine::call_resolution) fn resolve_cast_type(text: &str) -> Option<PhpType> {
    let (keyword, operand) = text.strip_prefix('(')?.split_once(')')?;
    if !operand_is_single(operand.trim()) {
        return None;
    }
    let name = match keyword.trim().to_ascii_lowercase().as_str() {
        "string" | "binary" => "string",
        "int" | "integer" => "int",
        "float" | "double" | "real" => "float",
        "bool" | "boolean" => "bool",
        "array" => "array",
        "object" => "object",
        _ => return None,
    };
    Some(PhpType::named(atom(name)))
}

/// The type a `??`/`?:` result carries given what each side resolved to.
///
/// Whichever side resolved when only one did is the answer, since the
/// other operand contributed nothing to know about; when both did and
/// they differ, the result could be either, so the answer is their union.
pub(super) fn join_operand_types(left: Option<PhpType>, right: Option<PhpType>) -> Option<PhpType> {
    match (left, right) {
        (Some(l), Some(r)) if l == r => Some(l),
        (Some(l), Some(r)) => Some(PhpType::union(vec![l, r])),
        (Some(l), None) => Some(l),
        (None, Some(r)) => Some(r),
        (None, None) => None,
    }
}

/// Whether `text` joins its parts with a top-level `.` concatenation.
pub(super) fn contains_top_level_concat(text: &str) -> bool {
    scan_top_level(text.as_bytes(), |bytes, i| match bytes[i] {
        b'?' if bytes[i..].starts_with(b"?->") => ScanStep::Skip(3),
        b'-' if bytes[i..].starts_with(b"->") => ScanStep::Skip(2),
        // `...` (spread/variadic) is not concatenation.
        b'.' if bytes[i..].starts_with(b"...") => ScanStep::Skip(3),
        b'.' => ScanStep::Stop,
        _ => ScanStep::Skip(1),
    })
    .is_some()
}

/// Split `text` at the first top-level null-coalescing operator (`??`).
///
/// `??` is right-associative, so splitting at the *first* one leaves the
/// rest of a `$a ?? $b ?? $c` chain in the right operand for the caller to
/// resolve the same way. The assignment form `??=` is not an expression
/// operator and is left alone.
pub(super) fn split_top_level_coalesce(text: &str) -> Option<(&str, &str)> {
    let at = scan_top_level(text.as_bytes(), |bytes, i| {
        if !bytes[i..].starts_with(b"??") {
            ScanStep::Skip(1)
        } else if bytes[i..].starts_with(b"??=") {
            ScanStep::Abort
        } else {
            ScanStep::Stop
        }
    })?;
    let left = text[..at].trim();
    let right = text[at + 2..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

/// Split `text` at a top-level elvis operator (`?:`), stepping over the
/// nullsafe `?->` (which is not this).
///
/// Returns the trimmed left and right operand texts, or `None` when no
/// top-level `?:` is found — including a full ternary (`$a ? $b : $c`),
/// which is left to the caller's other paths.
pub(super) fn split_top_level_elvis(text: &str) -> Option<(&str, &str)> {
    let at = scan_top_level(text.as_bytes(), |bytes, i| {
        if bytes[i] == b'?'
            && !bytes[i..].starts_with(b"?->")
            && text[i + 1..].trim_start().starts_with(':')
        {
            ScanStep::Stop
        } else {
            ScanStep::Skip(1)
        }
    })?;
    let rest = text[at + 1..].trim_start().strip_prefix(':')?;
    Some((text[..at].trim_end(), rest.trim_start()))
}

/// Whether `operand` is one expression rather than several joined by an
/// operator.
///
/// A variable, property or method chain, array index, call, or literal counts
/// as one; anything carrying a binary operator at the top level (outside its
/// own brackets, quotes and parentheses) does not. `->` and `?->` are chain
/// links, not operators.
fn operand_is_single(operand: &str) -> bool {
    if operand.is_empty() {
        return false;
    }
    scan_top_level(operand.as_bytes(), |bytes, i| match bytes[i] {
        // `->` and `?->` continue the chain, so they are stepped over
        // whole; a bare `-` or `?` is subtraction or a ternary.
        b'-' if bytes[i..].starts_with(b"->") => ScanStep::Skip(2),
        b'?' if bytes[i..].starts_with(b"?->") => ScanStep::Skip(3),
        b'-' | b'?' => ScanStep::Stop,
        b'.' | b'+' | b'*' | b'/' | b'%' | b'<' | b'>' | b'=' | b'!' | b'&' | b'|' | b'^'
        | b',' | b' ' | b'\t' | b'\n' => ScanStep::Stop,
        _ => ScanStep::Skip(1),
    })
    .is_none()
}

#[cfg(test)]
mod cast_tests {
    use super::resolve_cast_type;

    fn cast(text: &str) -> Option<String> {
        resolve_cast_type(text).map(|ty| ty.to_string())
    }

    #[test]
    fn a_cast_names_its_result_type() {
        assert_eq!(cast("(string) $value").as_deref(), Some("string"));
        assert_eq!(cast("(int)$value").as_deref(), Some("int"));
        assert_eq!(cast("(bool) $flag").as_deref(), Some("bool"));
        assert_eq!(cast("(float) $n").as_deref(), Some("float"));
        assert_eq!(cast("(array) $thing").as_deref(), Some("array"));
        assert_eq!(cast("(object) $thing").as_deref(), Some("object"));
    }

    #[test]
    fn the_aliases_php_accepts_read_the_same() {
        assert_eq!(cast("(integer) $n").as_deref(), Some("int"));
        assert_eq!(cast("(boolean) $b").as_deref(), Some("bool"));
        assert_eq!(cast("(double) $n").as_deref(), Some("float"));
        assert_eq!(cast("(binary) $s").as_deref(), Some("string"));
    }

    #[test]
    fn a_chain_or_index_operand_still_counts_as_one() {
        assert_eq!(
            cast("(string) $order->customer->id").as_deref(),
            Some("string")
        );
        assert_eq!(cast("(string) $row['name']").as_deref(), Some("string"));
        assert_eq!(cast("(string) $order?->total()").as_deref(), Some("string"));
        assert_eq!(cast("(string) $row['a b']").as_deref(), Some("string"));
    }

    #[test]
    fn a_cast_inside_a_larger_expression_is_left_alone() {
        assert_eq!(cast("(int) $a / 2"), None);
        assert_eq!(cast("(int) $a === $b"), None);
        assert_eq!(cast("(string) $a . $b"), None);
        assert_eq!(cast("(int) $a - 1"), None);
        assert_eq!(cast("(bool) $a && $b"), None);
    }

    #[test]
    fn text_that_is_not_a_cast_answers_nothing() {
        assert_eq!(cast("$value"), None);
        assert_eq!(cast("($value)"), None);
        assert_eq!(cast("(string)"), None);
        assert_eq!(cast("(new Order())->total()"), None);
        assert_eq!(cast("strlen($value)"), None);
    }
}
