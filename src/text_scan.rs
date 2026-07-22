//! Shared low-level byte/char scanning primitives for skipping over PHP
//! string literals and comments while searching source text for other
//! syntax (matching delimiters, call boundaries, etc.).
//!
//! These are intentionally simple, non-heredoc-aware scanners. The
//! classmap scanner (`classmap_scanner.rs`) has its own single-pass,
//! `memchr`-driven state machine for the same job because it also has to
//! track heredoc/nowdoc bodies and is a hot path over entire files; it is
//! kept separate rather than routed through here.

/// Skip past a string literal starting at `pos` (which must point to the
/// opening quote). Returns the position after the closing quote.
pub(crate) fn skip_string_forward(bytes: &[u8], pos: usize) -> usize {
    let quote = bytes[pos];
    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 1; // skip escaped char
        } else if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Skip past a line comment (`//…`) starting at `pos`. Returns the
/// position of the newline (or end of input).
pub(crate) fn skip_line_comment(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

/// Skip past a block comment (`/* … */`) starting at `pos`. Returns the
/// position after the closing `*/` (or end of input).
pub(crate) fn skip_block_comment(bytes: &[u8], pos: usize) -> usize {
    let mut i = pos + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    i
}

/// Find the matching closing delimiter for an opening delimiter at
/// `open_pos`, respecting string literal nesting (but not comments).
///
/// `open` and `close` are the delimiter bytes (e.g. `b'('` / `b')'` or
/// `b'{'` / `b'}'`).
pub(crate) fn find_matching_delimiter_forward(
    text: &str,
    open_pos: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let bytes = text.as_bytes();
    if open_pos >= bytes.len() || bytes[open_pos] != open {
        return None;
    }

    let mut depth = 1i32;
    let mut pos = open_pos + 1;

    while pos < bytes.len() && depth > 0 {
        match bytes[pos] {
            b if b == open => depth += 1,
            b if b == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(pos);
                }
            }
            b'\'' | b'"' => {
                let quote = bytes[pos];
                pos += 1;
                while pos < bytes.len() {
                    if bytes[pos] == b'\\' {
                        pos += 1;
                    } else if bytes[pos] == quote {
                        break;
                    }
                    pos += 1;
                }
            }
            _ => {}
        }
        pos += 1;
    }

    None
}

/// Skip backward past a string literal ending at position `end` (which
/// points to the closing quote character `q`). Returns the position of
/// the opening quote, or 0 if not found.
pub(crate) fn skip_string_backward(chars: &[char], end: usize, q: char) -> usize {
    if end == 0 {
        return 0;
    }
    let mut j = end - 1;
    while j > 0 {
        if chars[j] == q {
            // Check it's not escaped — count preceding backslashes.
            let mut backslashes = 0u32;
            let mut k = j;
            while k > 0 && chars[k - 1] == '\\' {
                backslashes += 1;
                k -= 1;
            }
            if backslashes.is_multiple_of(2) {
                // Not escaped — this is the opening quote.
                return j;
            }
        }
        j -= 1;
    }
    0
}
