//! Byte-level cursor predicates for override completion: which member
//! declaration slot, if any, the cursor is typing in.

use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Position, Range};

use crate::class_lookup::find_class_at_offset;
use crate::signature_render::detect_class_indent;
use crate::text_position::{offset_to_position, position_to_offset};
use crate::text_scan::{
    is_ident_byte, keyword_ends_at, scan_ident_backward, skip_block_comment, skip_line_comment,
};
use crate::types::ClassInfo;

/// Extract the partial method name and its LSP range at the cursor.
pub(crate) fn extract_method_name_partial(
    content: &str,
    position: Position,
) -> Option<(String, Range)> {
    let offset = position_to_offset(content, position) as usize;
    if offset > content.len() {
        return None;
    }
    let bytes = content.as_bytes();
    let mut start = offset;
    while start > 0 {
        let b = bytes[start - 1];
        if b.is_ascii_alphanumeric() || b == b'_' {
            start -= 1;
        } else {
            break;
        }
    }
    let partial = content[start..offset].to_string();

    let start_pos = offset_to_position(content, start);
    let end_pos = position;
    Some((
        partial,
        Range {
            start: start_pos,
            end: end_pos,
        },
    ))
}

/// Whether the cursor is after the `function` keyword (not `const`/`case`).
pub(crate) fn is_after_function_keyword(content: &str, position: Position) -> bool {
    after_keyword(content, position, "function")
}

/// Whether the cursor is after the `const` keyword (class constant name).
pub(crate) fn is_after_const_keyword(content: &str, position: Position) -> bool {
    let bytes = content.as_bytes();
    let cursor = (position_to_offset(content, position) as usize).min(bytes.len());
    let mut i = scan_ident_backward(bytes, cursor);
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if keyword_ends_at(bytes, i, b"const") {
        return !preceded_by_use_keyword_bytes(bytes, i - "const".len());
    }
    has_const_keyword_before_name(bytes, i)
}

fn after_keyword(content: &str, position: Position, keyword: &str) -> bool {
    let bytes = content.as_bytes();
    let cursor = (position_to_offset(content, position) as usize).min(bytes.len());
    let mut i = scan_ident_backward(bytes, cursor);
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    keyword_ends_at(bytes, i, keyword.as_bytes())
}

fn preceded_by_use_keyword_bytes(bytes: &[u8], keyword_start: usize) -> bool {
    let mut before = keyword_start;
    while before > 0 && bytes[before - 1].is_ascii_whitespace() {
        before -= 1;
    }
    keyword_ends_at(bytes, before, b"use")
}

fn has_const_keyword_before_name(bytes: &[u8], pos: usize) -> bool {
    let mut line_start = pos;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    // Find the standalone `const` keyword governing this line (skipping
    // `use const` imports).
    let Some(const_end) = bytes[line_start..pos]
        .windows(b"const".len())
        .enumerate()
        .find_map(|(idx, window)| {
            if window != b"const" {
                return None;
            }
            let start = line_start + idx;
            let end = start + b"const".len();
            let is_word = (start == 0 || !is_ident_byte(bytes[start - 1]))
                && (end >= bytes.len() || !is_ident_byte(bytes[end]));
            (is_word && !preceded_by_use_keyword_bytes(bytes, start)).then_some(end)
        })
    else {
        return false;
    };
    // A declarator *name* follows `const` or a top-level `,`; the cursor is
    // in the initializer once a top-level `=` is seen (until the next
    // comma). `const MAX = PHP_INT_M|` is a value position, not a name.
    !in_const_initializer(bytes, const_end, pos)
}

/// Whether the byte range `from..pos` (starting just after a `const`
/// keyword) ends inside a declarator initializer rather than a name slot.
fn in_const_initializer(bytes: &[u8], from: usize, pos: usize) -> bool {
    let mut depth = 0i32;
    let mut in_value = false;
    for &b in &bytes[from..pos] {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => in_value = false,
            b'=' if depth == 0 => in_value = true,
            _ => {}
        }
    }
    in_value
}

/// Whether the cursor sits at the start of a new member declaration in the
/// class-like body whose opening brace is at `body_start`.
///
/// Scans forward from the brace with a PHP-aware lexer so that comments,
/// docblocks, attributes, strings, and heredocs before the cursor are
/// skipped rather than mistaken for code.  A backwards scan cannot do
/// this: `// closes with }` and `/** @var int */` both end on bytes that
/// look like ordinary code from behind.
///
/// The cursor qualifies when, at the class body's own brace depth, the
/// only thing between the last `{`/`}`/`;` boundary and the cursor is
/// skippable trivia plus an optional `$` and identifier characters.
/// Nested braces (method bodies, property hooks, trait-use adaptation
/// blocks) therefore fall out for free, as does any position inside a
/// comment or string literal.
///
/// A leading `$` before the partial is accepted too (`$on|` for a
/// property override); the caller distinguishes it via
/// [`class_body_partial_starts_with_dollar`].
pub(crate) fn is_class_body_member_start(content: &str, body_start: usize, cursor: usize) -> bool {
    let bytes = content.as_bytes();
    let cursor = cursor.min(bytes.len());
    if body_start >= cursor || bytes.get(body_start) != Some(&b'{') {
        return false;
    }

    // Offset of the first significant byte of the member being typed, or
    // `None` when nothing but trivia has followed the last boundary.
    let mut pending_start: Option<usize> = None;
    let mut depth = 0usize;
    let mut i = body_start + 1;

    while i < cursor {
        let b = bytes[i];
        match b {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = skip_line_comment(bytes, i);
                continue;
            }
            b'#' if bytes.get(i + 1) == Some(&b'[') => {
                // Attributes precede the member they decorate, so they
                // leave the member-start position intact.
                let end = skip_attribute(bytes, i);
                if end > cursor {
                    return false;
                }
                i = end;
                continue;
            }
            b'#' => {
                i = skip_line_comment(bytes, i);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = skip_block_comment(bytes, i);
                if end > cursor {
                    return false;
                }
                i = end;
                continue;
            }
            b'\'' | b'"' => {
                let end = crate::text_scan::skip_string_forward(bytes, i);
                if end > cursor {
                    return false;
                }
                if pending_start.is_none() {
                    pending_start = Some(i);
                }
                i = end;
                continue;
            }
            b'<' if bytes[i..].starts_with(b"<<<") => {
                let end = skip_heredoc(bytes, i);
                if end > cursor {
                    return false;
                }
                if pending_start.is_none() {
                    pending_start = Some(i);
                }
                i = end;
                continue;
            }
            b'{' => {
                depth += 1;
                pending_start = None;
            }
            b'}' => {
                if depth == 0 {
                    // The body's closing brace: the cursor is past the
                    // end of this class.
                    return false;
                }
                depth -= 1;
                if depth == 0 {
                    pending_start = None;
                }
            }
            b';' if depth == 0 => pending_start = None,
            _ if b.is_ascii_whitespace() => {}
            _ if depth == 0 && pending_start.is_none() => pending_start = Some(i),
            _ => {}
        }
        i += 1;
    }

    if depth != 0 {
        return false;
    }
    let Some(start) = pending_start else {
        return true;
    };
    let mut k = start;
    if bytes[k] == b'$' {
        k += 1;
    }
    bytes[k..cursor].iter().all(|&b| is_ident_byte(b))
}

/// Whether the partial identifier at `cursor` is preceded by `$`.
pub(crate) fn class_body_partial_starts_with_dollar(content: &str, cursor: usize) -> bool {
    let bytes = content.as_bytes();
    let cursor = cursor.min(bytes.len());
    let i = scan_ident_backward(bytes, cursor);
    i > 0 && bytes[i - 1] == b'$'
}

/// Offset just past the closing `]` of the `#[…]` attribute at `i`,
/// tracking nested brackets and string literals.
fn skip_attribute(bytes: &[u8], i: usize) -> usize {
    let mut k = i + 1;
    let mut brackets = 0usize;
    while k < bytes.len() {
        match bytes[k] {
            b'\'' | b'"' => {
                k = crate::text_scan::skip_string_forward(bytes, k);
                continue;
            }
            b'[' => brackets += 1,
            b']' => {
                brackets -= 1;
                if brackets == 0 {
                    return k + 1;
                }
            }
            _ => {}
        }
        k += 1;
    }
    bytes.len()
}

/// Offset just past the terminator of the heredoc/nowdoc starting at `i`
/// (which points at `<<<`).
fn skip_heredoc(bytes: &[u8], i: usize) -> usize {
    let mut k = i + 3;
    while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
        k += 1;
    }
    let quote = matches!(bytes.get(k), Some(b'\'') | Some(b'"')).then(|| bytes[k]);
    if quote.is_some() {
        k += 1;
    }
    let label_start = k;
    while k < bytes.len() && is_ident_byte(bytes[k]) {
        k += 1;
    }
    let label = &bytes[label_start..k];
    if label.is_empty() {
        return bytes.len();
    }
    // Scan line by line for the closing label, which PHP 7.3+ allows to
    // be indented and followed by any non-identifier byte.
    while k < bytes.len() {
        let Some(n) = bytes[k..].iter().position(|&b| b == b'\n') else {
            return bytes.len();
        };
        k += n + 1;
        let mut line = k;
        while line < bytes.len() && (bytes[line] == b' ' || bytes[line] == b'\t') {
            line += 1;
        }
        if bytes[line..].starts_with(label)
            && !bytes
                .get(line + label.len())
                .is_some_and(|&b| is_ident_byte(b))
        {
            return line + label.len();
        }
    }
    bytes.len()
}

/// Property name after `$` on a property declaration line (not a parameter).
pub(crate) fn is_property_declaration_name_position(content: &str, position: Position) -> bool {
    let bytes = content.as_bytes();
    let cursor = (position_to_offset(content, position) as usize).min(bytes.len());
    is_property_declaration_name_position_at_offset(bytes, cursor)
}

pub(crate) fn is_member_declaration_name_position_at_offset(content: &str, cursor: usize) -> bool {
    let bytes = content.as_bytes();
    let cursor = cursor.min(bytes.len());
    is_function_or_const_name_position_at_offset(bytes, cursor)
        || is_property_declaration_name_position_at_offset(bytes, cursor)
}

/// Whether the cursor at byte `cursor` is typing a declared function,
/// constant or enum case *name*; see
/// [`is_function_or_const_name_position`](crate::completion::context::type_hint_completion::is_function_or_const_name_position).
pub(in crate::completion::context) fn is_function_or_const_name_position_at_offset(
    bytes: &[u8],
    cursor: usize,
) -> bool {
    let mut i = scan_ident_backward(bytes, cursor);

    let after_ident = i;
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i == after_ident && after_ident != cursor {
        return false;
    }
    if i == after_ident {
        return false;
    }

    if keyword_ends_at(bytes, i, b"fn") {
        return true;
    }
    // `case` names an enum case (a member) but also labels a `switch`
    // branch, where a class/const/enum name completion is wanted.
    if keyword_ends_at(bytes, i, b"case") {
        return case_is_enum_declaration(bytes, i);
    }
    if keyword_ends_at(bytes, i, b"function") {
        return !preceded_by_use_keyword_bytes(bytes, i - "function".len());
    }
    if keyword_ends_at(bytes, i, b"const") {
        return !preceded_by_use_keyword_bytes(bytes, i - "const".len());
    }
    has_const_keyword_before_name(bytes, i)
}

/// Whether the `case` keyword ending at `keyword_end` declares an enum case
/// (directly inside an `enum` body) rather than a `switch` branch label.
/// Scans back to the nearest enclosing unmatched `{` and checks whether it
/// opens an enum.
fn case_is_enum_declaration(bytes: &[u8], keyword_end: usize) -> bool {
    let mut depth = 0i32;
    let mut k = keyword_end - "case".len();
    while k > 0 {
        k -= 1;
        match bytes[k] {
            b'}' => depth += 1,
            b'{' => {
                if depth == 0 {
                    return brace_opens_enum(bytes, k);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    false
}

/// Whether the block opened by the `{` at `brace_pos` is an `enum` body,
/// determined from the block header (the text back to the previous
/// statement boundary).
fn brace_opens_enum(bytes: &[u8], brace_pos: usize) -> bool {
    let mut start = brace_pos;
    while start > 0 && !matches!(bytes[start - 1], b';' | b'{' | b'}') {
        start -= 1;
    }
    contains_ascii_word(&bytes[start..brace_pos], b"enum")
}

fn is_property_declaration_name_position_at_offset(bytes: &[u8], cursor: usize) -> bool {
    // Skip partial name.
    let i = scan_ident_backward(bytes, cursor);
    // Must be immediately after `$`.
    if i == 0 || bytes[i - 1] != b'$' {
        return false;
    }
    let dollar = i - 1;
    // Walk back on the same line looking for declaration context.
    let mut j = dollar;
    while j > 0 && bytes[j - 1] != b'\n' {
        j -= 1;
    }
    // Parameters live after `function` on the same line / signature.
    let line = &bytes[j..dollar];
    if contains_ascii_word(line, b"function") {
        return false;
    }
    // Property declarations have a visibility/static/readonly/var keyword.
    const MARKERS: &[&str] = &[
        "public",
        "protected",
        "private",
        "static",
        "readonly",
        "var",
    ];
    MARKERS
        .iter()
        .any(|m| contains_ascii_word(line, m.as_bytes()))
}

fn contains_ascii_word(bytes: &[u8], word: &[u8]) -> bool {
    if word.is_empty() || bytes.len() < word.len() {
        return false;
    }
    bytes.windows(word.len()).enumerate().any(|(idx, window)| {
        window.eq_ignore_ascii_case(word)
            && (idx == 0 || !is_ident_byte(bytes[idx - 1]))
            && (idx + word.len() == bytes.len() || !is_ident_byte(bytes[idx + word.len()]))
    })
}

/// Byte offset of the start of the line containing `position`.
pub(crate) fn line_start_position(content: &str, position: Position) -> Position {
    let offset = position_to_offset(content, position) as usize;
    let line_start = content[..offset.min(content.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    offset_to_position(content, line_start)
}

/// Resolve the enclosing class at the cursor, if any.
pub(crate) fn enclosing_class_at_position<'a>(
    classes: &'a [Arc<ClassInfo>],
    content: &str,
    position: Position,
) -> Option<&'a ClassInfo> {
    let offset = position_to_offset(content, position);
    find_class_at_offset(classes, offset)
}

/// Indent string for the current declaration line (member indent).
pub(crate) fn indent_for_position(content: &str, position: Position, class: &ClassInfo) -> String {
    let offset = position_to_offset(content, position) as usize;
    let line_start = content[..offset.min(content.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let line = &content[line_start..offset.min(content.len())];
    let line_indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    if !line_indent.is_empty() {
        return line_indent;
    }
    detect_class_indent(content, class)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_const_keyword_detects_class_const() {
        let src = "<?php\nclass C {\n    public const ST\n}\n";
        assert!(is_after_const_keyword(
            src,
            Position {
                line: 2,
                character: 19
            }
        ));
    }
}
