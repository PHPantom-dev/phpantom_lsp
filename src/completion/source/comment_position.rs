//! Comment, docblock, and string position detection.
//!
//! This module provides utilities to determine whether a given cursor
//! position falls inside a comment, docblock, or string literal.  These
//! are used early in the completion pipeline to decide whether to suppress
//! normal completions (inside `//` / `/* */` / string literals) or to
//! switch to PHPDoc tag completion (inside `/** */`).
//!
//! The functions here are pure — they take `(content, Position)` and
//! return a result without any shared state.

use tower_lsp::lsp_types::Position;

// Re-export the canonical position-to-byte-offset helper so that existing
// `use super::comment_position::position_to_byte_offset` imports continue
// to work without modification.
pub(crate) use crate::text_position::position_to_byte_offset;

/// Returns `true` if the given position is inside a `/** … */` docblock.
///
/// Scans backwards from the cursor to find the nearest `/**` that has not
/// been closed by a matching `*/` before the cursor position.
pub fn is_inside_docblock(content: &str, position: Position) -> bool {
    // Convert position to byte offset for easier scanning
    let byte_offset = position_to_byte_offset(content, position);

    let before_cursor = &content[..byte_offset.min(content.len())];

    // Find the last `/**` before the cursor
    let Some(open_pos) = before_cursor.rfind("/**") else {
        return false;
    };

    // Check if there is a `*/` between the `/**` and the cursor
    // (which would mean the docblock is closed)
    let after_open = &before_cursor[open_pos + 3..];
    !after_open.contains("*/")
}

/// Returns `true` if the given position is inside a `//` line comment or
/// a `/* … */` block comment that is **not** a `/** … */` docblock.
///
/// Uses a forward state-machine scan from the start of the file to
/// correctly handle comments inside string literals (which are ignored).
pub fn is_inside_non_doc_comment(content: &str, position: Position) -> bool {
    let target = position_to_byte_offset(content, position);
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // Scanner states
    #[derive(PartialEq)]
    enum State {
        Code,
        SingleString,
        DoubleString,
        LineComment,
        BlockComment,
        Docblock,
        Heredoc,
    }

    let mut state = State::Code;
    // For heredoc/nowdoc we track the closing label
    let mut heredoc_label: Vec<u8> = Vec::new();

    while i < len {
        if i >= target {
            return state == State::LineComment || state == State::BlockComment;
        }

        match state {
            State::Code => {
                if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
                    state = State::LineComment;
                    i += 2;
                } else if bytes[i] == b'/'
                    && i + 2 < len
                    && bytes[i + 1] == b'*'
                    && bytes[i + 2] == b'*'
                    && (i + 3 >= len || bytes[i + 3] != b'*')
                {
                    // `/**` but not `/***` — that's a docblock
                    state = State::Docblock;
                    i += 3;
                } else if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
                    state = State::BlockComment;
                    i += 2;
                } else if bytes[i] == b'\'' {
                    state = State::SingleString;
                    i += 1;
                } else if bytes[i] == b'"' {
                    state = State::DoubleString;
                    i += 1;
                } else if bytes[i] == b'<'
                    && i + 2 < len
                    && bytes[i + 1] == b'<'
                    && bytes[i + 2] == b'<'
                {
                    // Possible heredoc / nowdoc
                    i += 3;
                    // Skip optional whitespace
                    while i < len && bytes[i] == b' ' {
                        i += 1;
                    }
                    let is_nowdoc = i < len && bytes[i] == b'\'';
                    if is_nowdoc {
                        i += 1; // skip opening quote
                    }
                    heredoc_label.clear();
                    while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                        heredoc_label.push(bytes[i]);
                        i += 1;
                    }
                    if !heredoc_label.is_empty() {
                        if is_nowdoc && i < len && bytes[i] == b'\'' {
                            i += 1; // skip closing quote
                        }
                        state = State::Heredoc;
                    }
                } else {
                    i += 1;
                }
            }
            State::LineComment => {
                if bytes[i] == b'\n' {
                    state = State::Code;
                }
                i += 1;
            }
            State::BlockComment => {
                if bytes[i] == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                    state = State::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            State::Docblock => {
                if bytes[i] == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                    state = State::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            State::SingleString => {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2; // skip escaped char
                } else if bytes[i] == b'\'' {
                    state = State::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            State::DoubleString => {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2; // skip escaped char
                } else if bytes[i] == b'"' {
                    state = State::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            State::Heredoc => {
                // Look for the closing label at the start of a line
                if bytes[i] == b'\n' {
                    i += 1;
                    // Skip optional whitespace before the label
                    let line_start = i;
                    while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                        i += 1;
                    }
                    if i + heredoc_label.len() <= len
                        && &bytes[i..i + heredoc_label.len()] == heredoc_label.as_slice()
                    {
                        let after_label = i + heredoc_label.len();
                        if after_label >= len
                            || bytes[after_label] == b';'
                            || bytes[after_label] == b'\n'
                            || bytes[after_label] == b'\r'
                        {
                            i = after_label;
                            state = State::Code;
                            continue;
                        }
                    }
                    // Not the closing label — rewind to just after the newline
                    // to avoid skipping content, but we already advanced past
                    // whitespace which is fine (it's part of the heredoc body).
                    let _ = line_start; // consumed above
                } else {
                    i += 1;
                }
            }
        }
    }

    // Cursor is at or past end of file
    state == State::LineComment || state == State::BlockComment
}

/// Classification of the string context at a given cursor position.
#[derive(Debug, PartialEq, Eq)]
pub enum StringContext {
    /// The cursor is not inside any string literal.
    NotInString,
    /// The cursor is inside a string literal where interpolation is not
    /// possible (single-quoted string, nowdoc) or where the cursor is at
    /// a position that is not an interpolation site (plain text inside a
    /// double-quoted string or heredoc). Completion should be suppressed.
    InStringLiteral,
    /// The cursor is inside a simple interpolation context without braces
    /// (`"$var->"`, `"$var"`). PHP only allows property access here (no
    /// method calls, no chaining), so completion should filter to
    /// properties only.
    SimpleInterpolation,
    /// The cursor is inside a `{$…}` brace interpolation context where
    /// full PHP expressions are allowed (`"{$user->getName()}"`).
    /// Completion should proceed normally with no filtering.
    BraceInterpolation,
}

/// Classifies whether the cursor is inside a string and, if so, whether
/// it is at an interpolation site where completion should still fire.
///
/// Returns [`StringContext::InStringLiteral`] when completion should be
/// suppressed, [`StringContext::InInterpolation`] when inside a PHP
/// interpolation expression, and [`StringContext::NotInString`] when the
/// cursor is in normal code.
pub fn classify_string_context(content: &str, position: Position) -> StringContext {
    let target = position_to_byte_offset(content, position);
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    #[derive(PartialEq, Clone, Copy)]
    enum State {
        Code,
        SingleString,
        DoubleString,
        LineComment,
        BlockComment,
        Docblock,
        Heredoc,
        Nowdoc,
    }

    let mut state = State::Code;
    let mut heredoc_label: Vec<u8> = Vec::new();
    // Brace depth for `{$...}` interpolation inside double-quoted strings
    // and heredocs. Zero means we are in the string body proper; > 0 means
    // we are inside a `{$…}` complex interpolation expression.
    let mut brace_depth: u32 = 0;

    while i < len {
        if i >= target {
            return match state {
                State::SingleString | State::Nowdoc => StringContext::InStringLiteral,
                State::DoubleString | State::Heredoc => {
                    if brace_depth > 0 {
                        StringContext::BraceInterpolation
                    } else if is_simple_interpolation_site(bytes, target) {
                        StringContext::SimpleInterpolation
                    } else {
                        StringContext::InStringLiteral
                    }
                }
                _ => StringContext::NotInString,
            };
        }

        match state {
            State::Code => {
                if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
                    state = State::LineComment;
                    i += 2;
                } else if bytes[i] == b'/'
                    && i + 2 < len
                    && bytes[i + 1] == b'*'
                    && bytes[i + 2] == b'*'
                    && (i + 3 >= len || bytes[i + 3] != b'*')
                {
                    state = State::Docblock;
                    i += 3;
                } else if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
                    state = State::BlockComment;
                    i += 2;
                } else if bytes[i] == b'\'' {
                    state = State::SingleString;
                    i += 1;
                } else if bytes[i] == b'"' {
                    state = State::DoubleString;
                    brace_depth = 0;
                    i += 1;
                } else if bytes[i] == b'<'
                    && i + 2 < len
                    && bytes[i + 1] == b'<'
                    && bytes[i + 2] == b'<'
                {
                    i += 3;
                    while i < len && bytes[i] == b' ' {
                        i += 1;
                    }
                    let is_nowdoc = i < len && bytes[i] == b'\'';
                    if is_nowdoc {
                        i += 1;
                    }
                    heredoc_label.clear();
                    while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                        heredoc_label.push(bytes[i]);
                        i += 1;
                    }
                    if !heredoc_label.is_empty() {
                        if is_nowdoc {
                            if i < len && bytes[i] == b'\'' {
                                i += 1;
                            }
                            state = State::Nowdoc;
                        } else {
                            state = State::Heredoc;
                            brace_depth = 0;
                        }
                    }
                } else {
                    i += 1;
                }
            }
            State::LineComment => {
                if bytes[i] == b'\n' {
                    state = State::Code;
                }
                i += 1;
            }
            State::BlockComment | State::Docblock => {
                if bytes[i] == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                    state = State::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            State::SingleString => {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'\'' {
                    state = State::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            State::DoubleString => {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'"' && brace_depth == 0 {
                    state = State::Code;
                    i += 1;
                } else if bytes[i] == b'{'
                    && ((i + 1 < len && bytes[i + 1] == b'$') || brace_depth > 0)
                {
                    brace_depth += 1;
                    i += 1;
                } else if bytes[i] == b'}' && brace_depth > 0 {
                    brace_depth -= 1;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            State::Heredoc => {
                if bytes[i] == b'{' && ((i + 1 < len && bytes[i + 1] == b'$') || brace_depth > 0) {
                    brace_depth += 1;
                    i += 1;
                } else if bytes[i] == b'}' && brace_depth > 0 {
                    brace_depth -= 1;
                    i += 1;
                } else if bytes[i] == b'\n' {
                    i += 1;
                    let line_start = i;
                    while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                        i += 1;
                    }
                    if i + heredoc_label.len() <= len
                        && &bytes[i..i + heredoc_label.len()] == heredoc_label.as_slice()
                    {
                        let after_label = i + heredoc_label.len();
                        if after_label >= len
                            || bytes[after_label] == b';'
                            || bytes[after_label] == b'\n'
                            || bytes[after_label] == b'\r'
                        {
                            i = after_label;
                            state = State::Code;
                            brace_depth = 0;
                            continue;
                        }
                    }
                    let _ = line_start;
                } else {
                    i += 1;
                }
            }
            State::Nowdoc => {
                if bytes[i] == b'\n' {
                    i += 1;
                    let line_start = i;
                    while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
                        i += 1;
                    }
                    if i + heredoc_label.len() <= len
                        && &bytes[i..i + heredoc_label.len()] == heredoc_label.as_slice()
                    {
                        let after_label = i + heredoc_label.len();
                        if after_label >= len
                            || bytes[after_label] == b';'
                            || bytes[after_label] == b'\n'
                            || bytes[after_label] == b'\r'
                        {
                            i = after_label;
                            state = State::Code;
                            continue;
                        }
                    }
                    let _ = line_start;
                } else {
                    i += 1;
                }
            }
        }
    }

    // Cursor at or past end of file
    match state {
        State::SingleString | State::Nowdoc => StringContext::InStringLiteral,
        State::DoubleString | State::Heredoc => {
            if brace_depth > 0 {
                StringContext::BraceInterpolation
            } else if is_simple_interpolation_site(bytes, target) {
                StringContext::SimpleInterpolation
            } else {
                StringContext::InStringLiteral
            }
        }
        _ => StringContext::NotInString,
    }
}

/// Checks whether the bytes immediately before `target` look like a PHP
/// interpolation site inside a double-quoted string or heredoc.
///
/// Recognised patterns (cursor shown as `|`):
/// - `$identifier->|`  or `$identifier?->|` — member access
/// - `$identifier|` — partially typed variable name
/// - `$|` — bare dollar, user is starting an interpolation
///
/// All of these are valid interpolation starts in PHP double-quoted
/// strings, so completion should be allowed rather than suppressed.
fn is_simple_interpolation_site(bytes: &[u8], target: usize) -> bool {
    let mut pos = target;

    // ── Case 1: member access `$identifier->` / `$identifier?->` ────
    // Check `?->` first so the longer operator is not partially matched.
    let has_arrow =
        if pos >= 3 && bytes[pos - 3] == b'?' && bytes[pos - 2] == b'-' && bytes[pos - 1] == b'>' {
            pos -= 3;
            true
        } else if pos >= 2 && bytes[pos - 2] == b'-' && bytes[pos - 1] == b'>' {
            pos -= 2;
            true
        } else {
            false
        };

    // Walk back over the identifier `[a-zA-Z0-9_]*`.
    let before_ident = pos;
    while pos > 0 && (bytes[pos - 1].is_ascii_alphanumeric() || bytes[pos - 1] == b'_') {
        pos -= 1;
    }
    let ident_len = before_ident - pos;

    // If we consumed an arrow, the identifier must be non-empty and
    // preceded by `$`.
    if has_arrow {
        return ident_len > 0 && pos > 0 && bytes[pos - 1] == b'$';
    }

    // ── Case 2: bare `$` or `$partialName` (variable interpolation) ─
    // The identifier may be empty (just `$`) or partially typed.
    pos > 0 && bytes[pos - 1] == b'$'
}
