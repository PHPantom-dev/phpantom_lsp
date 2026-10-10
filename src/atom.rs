//! Interned string type for symbol names.
//!
//! This module provides [`Atom`], a globally-interned string type backed by
//! [`ustr::Ustr`]. Every distinct string is stored in memory exactly once;
//! equality checks are pointer comparisons and hash lookups use identity
//! hashing (the pointer value itself, no re-hashing of string content).
//!
//! # When to use `Atom`
//!
//! Use `Atom` for values that are:
//! - Compared frequently (class names, method names, property names)
//! - Used as HashMap keys on hot paths
//! - Copied/cloned millions of times during analysis
//!
//! Do **not** use `Atom` for:
//! - Long free-text strings (descriptions, docblock bodies)
//! - Strings that are constructed once and never compared
//! - Temporary/intermediate strings during parsing

use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::BuildHasherDefault;

use ustr::IdentityHasher;

/// A globally-interned string. Cloning is free (Copy type), equality is
/// a pointer comparison, and hashing uses identity hashing.
pub type Atom = ustr::Ustr;

/// A high-performance `HashMap` using `Atom` as the key.
///
/// Uses identity hashing (the pre-computed hash stored inside `Ustr`)
/// instead of re-hashing string content on every lookup.
pub type AtomMap<V> = HashMap<Atom, V, BuildHasherDefault<IdentityHasher>>;

/// A high-performance `HashSet` using `Atom` as the key.
pub type AtomSet = HashSet<Atom, BuildHasherDefault<IdentityHasher>>;

/// Convert a byte slice borrowed from PHP *source text* to a string slice.
///
/// Every PHP file is read as a `String`, so any byte slice the lexer hands
/// back that points into the source (identifiers, keywords, operators, and
/// the `raw` token text of a literal) is a slice of valid UTF-8 on a
/// character boundary. This covers those inputs only.
///
/// It does **not** cover the *decoded* value of a string literal
/// ([`mago_syntax::cst::LiteralString::value`]): mago resolves `"\x8b"` to
/// the single byte `0x8b`, which no UTF-8 string can hold. Use
/// [`literal_bytes_to_str`] for those.
#[inline]
pub fn bytes_to_str(bytes: &[u8]) -> &str {
    // SAFETY: the caller passes a slice of the source text, which is UTF-8
    unsafe { std::str::from_utf8_unchecked(bytes) }
}

/// Convert the decoded value of a PHP string literal to a string slice,
/// returning `None` when it is not valid UTF-8.
///
/// Unlike [`bytes_to_str`], the input here is arbitrary bytes rather than
/// source text: an escape such as `"\x8b"` or `"\212"` decodes to a byte
/// that cannot appear in a `str`. Callers treat `None` as "this literal's
/// value is not something I can read" and fall back or bail out.
#[inline]
pub fn literal_bytes_to_str(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes).ok()
}

/// Intern a byte slice as an [`Atom`], treating it as UTF-8.
#[inline]
pub fn atom_bytes(bytes: &[u8]) -> Atom {
    atom(bytes_to_str(bytes))
}

/// Intern a byte slice as a lowercase [`Atom`].
#[inline]
pub fn ascii_lowercase_atom_bytes(bytes: &[u8]) -> Atom {
    ascii_lowercase_atom(bytes_to_str(bytes))
}

/// Get the last segment of a namespace-separated byte slice.
///
/// WORKAROUND(mago 1.29): `Identifier::last_segment()` uses `position`
/// (first match) instead of `rposition` (last match), returning incorrect
/// results for qualified/fully-qualified names. Remove this helper and
/// switch back to `.last_segment()` once mago fixes the bug.
/// See: https://github.com/carthage-software/mago/issues/XXXX
#[inline]
pub fn last_segment(bytes: &[u8]) -> &[u8] {
    match bytes.iter().rposition(|b| *b == b'\\') {
        Some(pos) => &bytes[pos + 1..],
        None => bytes,
    }
}

/// Intern a string, returning an [`Atom`].
///
/// If the string has been seen before, returns the existing interned
/// instance (pointer comparison will match). If not, stores a new copy
/// in the global intern table.
///
/// # Examples
///
/// ```
/// use phpantom_lsp::atom::atom;
///
/// let a = atom("App\\Models\\User");
/// let b = atom("App\\Models\\User");
/// assert_eq!(a, b); // pointer comparison
/// ```
#[inline]
pub fn atom(s: &str) -> Atom {
    ustr::ustr(s)
}

/// The [`Atom`] for `s` if it has already been interned, without
/// interning it otherwise.
#[inline]
pub fn existing_atom(s: &str) -> Option<Atom> {
    ustr::existing_ustr(s)
}

/// Intern a string after lowercasing ASCII characters.
///
/// PHP class and function names are case-insensitive (but not constant
/// names). This helper avoids a heap allocation for strings up to 256
/// bytes by using a stack buffer.
#[inline]
pub fn ascii_lowercase_atom(s: &str) -> Atom {
    let bytes = s.as_bytes();

    // Fast path: already lowercase
    if !bytes.iter().any(u8::is_ascii_uppercase) {
        return atom(s);
    }

    // Stack buffer for short strings (covers virtually all PHP symbol names)
    const STACK_BUF_SIZE: usize = 256;
    if s.len() <= STACK_BUF_SIZE {
        let mut buf = [0u8; STACK_BUF_SIZE];
        for (i, &b) in bytes.iter().enumerate() {
            buf[i] = b.to_ascii_lowercase();
        }
        // SAFETY: ASCII lowercasing of valid UTF-8 produces valid UTF-8
        return atom(unsafe { std::str::from_utf8_unchecked(&buf[..s.len()]) });
    }

    atom(&s.to_ascii_lowercase())
}

/// Intern a PHP constant name, lowercasing only the namespace portion.
///
/// PHP constants are case-sensitive, but namespace lookups are
/// case-insensitive. For `"App\\Constants\\MY_CONST"`, this returns an
/// atom with `"app\\constants\\MY_CONST"`.
#[inline]
pub fn ascii_lowercase_constant_name_atom(name: &str) -> Atom {
    if let Some(last_slash_idx) = name.rfind('\\') {
        let (namespace, const_part) = name.split_at(last_slash_idx);
        let const_name = &const_part[1..]; // skip the backslash

        const STACK_BUF_SIZE: usize = 256;
        if name.len() <= STACK_BUF_SIZE {
            let mut buf = [0u8; STACK_BUF_SIZE];
            let mut index = 0;

            for byte in namespace.bytes() {
                buf[index] = byte.to_ascii_lowercase();
                index += 1;
            }

            buf[index] = b'\\';
            index += 1;

            let const_bytes = const_name.as_bytes();
            buf[index..index + const_bytes.len()].copy_from_slice(const_bytes);
            index += const_bytes.len();

            // SAFETY: ASCII lowercasing of valid UTF-8 produces valid UTF-8
            return atom(unsafe { std::str::from_utf8_unchecked(&buf[..index]) });
        }

        let mut result = namespace.to_ascii_lowercase();
        result.push('\\');
        result.push_str(const_name);
        atom(&result)
    } else {
        // No namespace — constant name is case-sensitive, return as-is
        atom(name)
    }
}
