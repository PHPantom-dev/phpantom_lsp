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

use tower_lsp_server::gen_lsp_types::Position;

// Re-export the canonical position-to-byte-offset helper so callers can
// import it from this module rather than from `text_position` directly.
pub(crate) use crate::text_position::position_to_byte_offset;

/// Returns `true` if the given position is inside a `/** … */` docblock.
///
/// Scans backwards from the cursor to find the nearest `/**` that has not
/// been closed by a matching `*/` before the cursor position.
pub fn is_inside_docblock(content: &str, position: Position) -> bool {
    is_offset_inside_docblock(content, position_to_byte_offset(content, position))
}

/// [`is_inside_docblock`] for a byte offset.
pub(crate) fn is_offset_inside_docblock(content: &str, byte_offset: usize) -> bool {
    let before_cursor = content
        .get(..byte_offset.min(content.len()))
        .unwrap_or(content);

    let Some(open_pos) = before_cursor.rfind("/**") else {
        return false;
    };

    // A `*/` between the `/**` and the cursor means the docblock is
    // already closed, so the cursor is not inside it.
    let after_open = &before_cursor[open_pos + 3..];
    !after_open.contains("*/")
}

/// Returns `true` if the given position is inside a `//` line comment or
/// a `/* … */` block comment that is **not** a `/** … */` docblock.
///
/// Uses a forward scan from the start of the file (see [`lexical_state_at`])
/// so comment markers inside string literals are ignored.
pub fn is_inside_non_doc_comment(content: &str, position: Position) -> bool {
    let target = position_to_byte_offset(content, position);
    matches!(
        lexical_state_at(content.as_bytes(), target).state,
        Lexical::LineComment | Lexical::BlockComment
    )
}

/// Where the `/** … */` docblock that `byte_offset` sits in opens, from
/// the forward scan of [`lexical_state_at`], so a `/**` inside a string
/// literal is not taken for one.
pub(crate) fn docblock_start_at(content: &str, byte_offset: usize) -> Option<usize> {
    let at = lexical_state_at(content.as_bytes(), byte_offset);
    (at.state == Lexical::Docblock).then_some(at.opened_at)
}

/// Classification of the string context at a given cursor position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
/// suppressed, [`StringContext::SimpleInterpolation`] or
/// [`StringContext::BraceInterpolation`] when inside a PHP interpolation
/// expression, and [`StringContext::NotInString`] when the cursor is in
/// normal code.
pub fn classify_string_context(content: &str, position: Position) -> StringContext {
    let target = position_to_byte_offset(content, position);
    let bytes = content.as_bytes();
    let at = lexical_state_at(bytes, target);
    match at.state {
        Lexical::SingleString | Lexical::Nowdoc => StringContext::InStringLiteral,
        Lexical::DoubleString | Lexical::Heredoc => {
            if at.brace_depth > 0 {
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

/// What the source at an offset sits in.
#[derive(PartialEq, Clone, Copy)]
enum Lexical {
    Code,
    SingleString,
    DoubleString,
    LineComment,
    BlockComment,
    Docblock,
    Heredoc,
    Nowdoc,
}

/// What the forward scan of [`lexical_state_at`] finds at an offset.
struct LexicalAt {
    state: Lexical,
    /// Where the string or comment the offset sits in opens.
    opened_at: usize,
    /// The depth of the `{$…}` interpolation braces open there: zero in
    /// the body of a double-quoted string or heredoc proper and greater
    /// inside a complex interpolation expression, whose own quotes do not
    /// end the string.
    brace_depth: u32,
}

/// The [`Lexical`] state at byte `target` of `bytes`, from a forward scan
/// of the file up to it.
fn lexical_state_at(bytes: &[u8], target: usize) -> LexicalAt {
    let len = bytes.len();
    let mut i = 0;
    let mut state = Lexical::Code;
    let mut opened_at = 0;
    let mut heredoc_label: Vec<u8> = Vec::new();
    let mut brace_depth: u32 = 0;

    while i < len {
        if i >= target {
            return LexicalAt {
                state,
                opened_at,
                brace_depth,
            };
        }

        match state {
            Lexical::Code => {
                opened_at = i;
                if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
                    state = Lexical::LineComment;
                    i += 2;
                } else if bytes[i] == b'#' && bytes.get(i + 1) != Some(&b'[') {
                    // `#[` opens an attribute rather than a comment.
                    state = Lexical::LineComment;
                    i += 1;
                } else if bytes[i] == b'/'
                    && i + 2 < len
                    && bytes[i + 1] == b'*'
                    && bytes[i + 2] == b'*'
                    && (i + 3 >= len || bytes[i + 3] != b'*')
                {
                    // `/**` but not `/***` — that's a docblock
                    state = Lexical::Docblock;
                    i += 3;
                } else if bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
                    state = Lexical::BlockComment;
                    i += 2;
                } else if bytes[i] == b'\'' {
                    state = Lexical::SingleString;
                    i += 1;
                } else if bytes[i] == b'"' {
                    state = Lexical::DoubleString;
                    brace_depth = 0;
                    i += 1;
                } else if bytes[i] == b'<'
                    && i + 2 < len
                    && bytes[i + 1] == b'<'
                    && bytes[i + 2] == b'<'
                {
                    // Possible heredoc / nowdoc
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
                            state = Lexical::Nowdoc;
                        } else {
                            state = Lexical::Heredoc;
                            brace_depth = 0;
                        }
                    }
                } else {
                    i += 1;
                }
            }
            Lexical::LineComment => {
                if bytes[i] == b'\n' {
                    state = Lexical::Code;
                }
                i += 1;
            }
            Lexical::BlockComment | Lexical::Docblock => {
                if bytes[i] == b'*' && i + 1 < len && bytes[i + 1] == b'/' {
                    state = Lexical::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Lexical::SingleString => {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'\'' {
                    state = Lexical::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            Lexical::DoubleString => {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                } else if bytes[i] == b'"' && brace_depth == 0 {
                    state = Lexical::Code;
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
            Lexical::Heredoc | Lexical::Nowdoc => {
                let interpolates = state == Lexical::Heredoc;
                if interpolates
                    && bytes[i] == b'{'
                    && ((i + 1 < len && bytes[i + 1] == b'$') || brace_depth > 0)
                {
                    brace_depth += 1;
                    i += 1;
                } else if interpolates && bytes[i] == b'}' && brace_depth > 0 {
                    brace_depth -= 1;
                    i += 1;
                } else if bytes[i] == b'\n' {
                    // Look for the closing label at the start of a line.
                    i += 1;
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
                            state = Lexical::Code;
                            brace_depth = 0;
                            continue;
                        }
                    }
                    // Not the closing label — the whitespace scanned past
                    // above is part of the body, so no rewind is needed.
                } else {
                    i += 1;
                }
            }
        }
    }

    // Cursor at or past end of file
    LexicalAt {
        state,
        opened_at,
        brace_depth,
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
