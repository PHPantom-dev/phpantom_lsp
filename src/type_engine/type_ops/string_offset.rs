//! The value an offset read of a string yields.
//!
//! `$s[$i]` reads one byte of `$s` as a one-character string, counting a
//! negative offset back from the end. A literal string and an offset whose
//! range is known pin that down to the characters it can land on, and an
//! offset past either end reads `''` (with a warning) rather than failing.
//! Anything less known is assumed to read a character, the way an offset
//! read of a generic array is assumed to find its value: a
//! `non-empty-string`, lowercase or uppercase when the string is.

use crate::php_type::{LiteralValue, PhpType, TypeKind};

use super::int_interval::IntInterval;

/// How many offsets a read may land on before its characters are no longer
/// listed one by one.
const MAX_LISTED_OFFSETS: i64 = 256;

/// The value `base[offset]` reads, for a `base` that is a string (one union
/// member at a time). `offset` is the offset's type, or `None` when it is
/// not known.
pub(crate) fn string_offset_read(base: &PhpType, offset: Option<&PhpType>) -> PhpType {
    if let Some(offset) = offset
        && let Some(literal) = base.as_literal()
        && let Some(read) = literal_string_offset_read(literal, offset)
    {
        return read;
    }
    one_character_of(base)
}

/// The characters of the literal string `literal` that `offset` can land
/// on, plus `''` when it can fall outside the string. `None` when the
/// string's bytes or the offset's range are not known, or there are too
/// many offsets to list.
fn literal_string_offset_read(literal: &LiteralValue, offset: &PhpType) -> Option<PhpType> {
    let content = literal.exact_string_content()?;
    // A byte of a multi-byte character is not a string a type can spell.
    if !content.is_ascii() {
        return None;
    }
    let bytes = content.as_bytes();
    let len = i64::try_from(bytes.len()).ok()?;
    let in_string = IntInterval {
        min: Some(-len),
        max: Some(len - 1),
    };
    let mut values: Vec<PhpType> = Vec::new();
    let mut push = |value: PhpType| {
        if !values.contains(&value) {
            values.push(value);
        }
    };
    let members: Vec<&PhpType> = match offset.kind() {
        TypeKind::Union(members) => members.iter().collect(),
        _ => vec![offset],
    };
    for member in members {
        let interval = IntInterval::of_int(member)?;
        let Some(inside) = interval.intersect(in_string) else {
            push(PhpType::literal_string_value(""));
            continue;
        };
        let (min, max) = (inside.min?, inside.max?);
        if max - min >= MAX_LISTED_OFFSETS {
            return None;
        }
        for index in min..=max {
            let at = if index < 0 { index + len } else { index };
            let byte = bytes[usize::try_from(at).ok()?];
            push(PhpType::literal_string_value((byte as char).to_string()));
        }
        if inside != interval {
            push(PhpType::literal_string_value(""));
        }
    }
    Some(PhpType::union(values))
}

/// One character of a string of type `base`: never empty, and keeping the
/// letter case `base` promises.
fn one_character_of(base: &PhpType) -> PhpType {
    let non_empty = PhpType::parse("non-empty-string");
    for case in ["lowercase-string", "uppercase-string"] {
        if base.is_subtype_of(&PhpType::parse(case)) {
            return PhpType::intersection(vec![PhpType::parse(case), non_empty]);
        }
    }
    non_empty
}
