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

/// Convert a byte slice to a string slice.
///
/// PHP source is always valid UTF-8 (mago guarantees this after lexing),
/// so this is a safe unchecked conversion on the hot path.
#[inline]
pub fn bytes_to_str(bytes: &[u8]) -> &str {
    // SAFETY: mago lexer only produces valid UTF-8 identifier bytes
    unsafe { std::str::from_utf8_unchecked(bytes) }
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
