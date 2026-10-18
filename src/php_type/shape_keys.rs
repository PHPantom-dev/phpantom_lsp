//! Array keys as PHP stores them, and the keys a shape entry is recorded
//! under.

use super::*;

/// Whether PHP stores this string as an integer array key.
///
/// Only canonical decimal spellings are coerced: `"8"` and `"-8"` become
/// integer keys, while `"+8"`, `"08"`, decimal fractions, and values outside
/// the platform integer range remain strings.
pub(crate) fn is_decimal_int_array_key(content: &str) -> bool {
    !content.starts_with('+')
        && content
            .parse::<i64>()
            .is_ok_and(|parsed| parsed.to_string() == content)
}

/// Whether `isset($string[$key])` can hold for this string key: PHP only
/// sets a string's integer offsets, and accepts any integer-numeric
/// spelling of one (`"1"`, `"01"`, `" 1"`, `"+1"`), but not a fraction
/// (`"1.0"`), a trailing non-digit (`"1x"`), or a word.
pub(crate) fn may_be_set_string_offset(key: &str) -> bool {
    let trimmed = key.trim_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Whether reading `$string[$key]` can yield a character.  A key that does
/// not begin with an integer names no offset at all: the read throws, and
/// under `??` it falls back to the default.  A leading-numeric one (`"1x"`)
/// still reads the character at that integer, with a warning.
pub(crate) fn may_read_string_offset(key: &str) -> bool {
    let trimmed = key.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    digits.bytes().next().is_some_and(|b| b.is_ascii_digit())
}

/// The shape key an entry written under `Foo::class` is recorded with: the
/// constant's spelling, which is how a docblock writes the same key and
/// which keeps it a `class-string<Foo>` when the keys are read back.
pub(crate) fn class_name_shape_key(class: &str) -> String {
    format!("{class}::class")
}

/// The class a shape key spelled `Foo::class` is the name of.
pub(crate) fn class_name_key(key: &str) -> Option<&str> {
    let (class, constant) = key.rsplit_once("::")?;
    (constant.eq_ignore_ascii_case("class") && is_class_name_spelling(class)).then_some(class)
}

/// Whether `text` is spelled like a class name, qualified or not.
fn is_class_name_spelling(text: &str) -> bool {
    let text = text.strip_prefix('\\').unwrap_or(text);
    !text.is_empty()
        && text.split('\\').all(|part| {
            part.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_' || !c.is_ascii())
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii())
        })
}

/// The class a shape key spelled `Foo::CONSTANT` reads its constant from,
/// and the constant's name.
pub(crate) fn class_constant_key(key: &str) -> Option<(&str, &str)> {
    let (class, constant) = key.rsplit_once("::")?;
    (is_class_name_spelling(class) && is_class_name_spelling(constant) && !constant.contains('\\'))
        .then_some((class, constant))
}

/// The runtime array key each shape entry occupies, in order.
///
/// A positional entry takes the next free integer index, mirroring the
/// literal or the sequence of appends it was tracked from, and an explicit
/// integer key moves that cursor past itself. Returns `None` once an entry
/// that may be absent leaves a later positional entry's index in doubt,
/// since pairing two shapes up is only decidable when both sides' keys are.
pub(crate) fn runtime_shape_keys(entries: &[ShapeEntry]) -> Option<Vec<String>> {
    let mut next: i64 = 0;
    let mut shifted = false;
    let mut keys = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry.key.as_deref() {
            None => {
                if shifted {
                    return None;
                }
                keys.push(next.to_string());
                next = next.checked_add(1)?;
                shifted |= entry.optional;
            }
            Some(key) => {
                if let Some(index) = canonical_int_key(key) {
                    next = next.max(index.checked_add(1)?);
                    shifted |= entry.optional;
                }
                keys.push(key.to_string());
            }
        }
    }
    Some(keys)
}

/// The type of the array key a runtime shape key stands for: the integer or
/// string literal it spells.
///
/// A class-constant key is stored as its spelling, which says nothing about
/// the key it evaluates to: `Foo::class` is the class's name, and any other
/// constant is only known to be some `array-key`.
pub(crate) fn shape_key_type(key: &str) -> PhpType {
    if is_canonical_int_key(key) {
        PhpType::literal_int(key)
    } else if let Some(class) = class_name_key(key) {
        PhpType::class_string(Some(PhpType::named(atom(class))))
    } else if class_constant_key(key).is_some() {
        PhpType::named(atom("array-key"))
    } else {
        PhpType::literal_string_value(key)
    }
}
