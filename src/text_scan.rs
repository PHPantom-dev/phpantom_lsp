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

/// Remove surrounding single or double quotes from a PHP string literal.
///
/// `"'hello'"` → `Some("hello")`, `"\"world\""` → `Some("world")`,
/// `"bare"` → `None`.
pub(crate) fn unquote_php_string(raw: &str) -> Option<&str> {
    raw.strip_prefix('\'')
        .and_then(|r| r.strip_suffix('\''))
        .or_else(|| raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')))
}

/// Find the first `;` in `s` that is not nested inside `()`, `[]`,
/// `{}`, or string literals.
///
/// Returns the byte offset of the semicolon, or `None` if no
/// top-level semicolon exists.  Used by multiple completion modules
/// to delimit the right-hand side of assignment statements.
pub(crate) fn find_semicolon_balanced(s: &str) -> Option<usize> {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut prev_char = '\0';

    for (i, ch) in s.char_indices() {
        if in_single_quote {
            if ch == '\'' && prev_char != '\\' {
                in_single_quote = false;
            }
            prev_char = ch;
            continue;
        }
        if in_double_quote {
            if ch == '"' && prev_char != '\\' {
                in_double_quote = false;
            }
            prev_char = ch;
            continue;
        }
        match ch {
            '\'' => in_single_quote = true,
            '"' => in_double_quote = true,
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            ';' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                return Some(i);
            }
            _ => {}
        }
        prev_char = ch;
    }
    None
}

/// Find the position of the closing delimiter that matches the opening
/// delimiter at `open_pos`, scanning forward.
///
/// `open` and `close` are the opening and closing byte values (e.g.
/// `b'{'` / `b'}'` or `b'('` / `b')'`).  The scan is aware of string
/// literals (`'…'` and `"…"` with backslash escaping) and both styles
/// of PHP comment (`// …` and `/* … */`), so delimiters inside strings
/// or comments are not counted.
pub(crate) fn find_matching_forward(
    text: &str,
    open_pos: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    if open_pos >= len || bytes[open_pos] != open {
        return None;
    }
    let mut depth = 1u32;
    let mut pos = open_pos + 1;
    let mut in_single = false;
    let mut in_double = false;
    while pos < len && depth > 0 {
        let b = bytes[pos];
        if in_single {
            if b == b'\\' {
                pos += 1;
            } else if b == b'\'' {
                in_single = false;
            }
        } else if in_double {
            if b == b'\\' {
                pos += 1;
            } else if b == b'"' {
                in_double = false;
            }
        } else {
            match b {
                b'\'' => in_single = true,
                b'"' => in_double = true,
                b if b == open => depth += 1,
                b if b == close => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(pos);
                    }
                }
                b'/' if pos + 1 < len => {
                    if bytes[pos + 1] == b'/' {
                        // Line comment — skip to end of line
                        while pos < len && bytes[pos] != b'\n' {
                            pos += 1;
                        }
                        continue;
                    }
                    if bytes[pos + 1] == b'*' {
                        // Block comment — skip to `*/`
                        pos += 2;
                        while pos + 1 < len {
                            if bytes[pos] == b'*' && bytes[pos + 1] == b'/' {
                                pos += 1;
                                break;
                            }
                            pos += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        pos += 1;
    }
    None
}

/// Find the position of the opening delimiter that matches the closing
/// delimiter at `close_pos`, scanning backward.
///
/// `open` and `close` are the opening and closing byte values (e.g.
/// `b'{'` / `b'}'` or `b'('` / `b')'`).  The scan skips over string
/// literals (`'…'` and `"…"`) by counting preceding backslashes to
/// distinguish escaped from unescaped quotes.
pub(crate) fn find_matching_backward(
    text: &str,
    close_pos: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let bytes = text.as_bytes();
    if close_pos >= bytes.len() || bytes[close_pos] != close {
        return None;
    }

    let mut depth = 1i32;
    let mut pos = close_pos;

    while pos > 0 {
        pos -= 1;
        match bytes[pos] {
            b if b == close => depth += 1,
            b if b == open => {
                depth -= 1;
                if depth == 0 {
                    return Some(pos);
                }
            }
            // Skip string literals by walking backward to the opening quote.
            b'\'' | b'"' => {
                let quote = bytes[pos];
                if pos > 0 {
                    pos -= 1;
                    while pos > 0 {
                        if bytes[pos] == quote {
                            // Check for escape — count preceding backslashes
                            let mut bs = 0;
                            let mut check = pos;
                            while check > 0 && bytes[check - 1] == b'\\' {
                                bs += 1;
                                check -= 1;
                            }
                            if bs % 2 == 0 {
                                break; // unescaped quote — string start
                            }
                        }
                        pos -= 1;
                    }
                }
            }
            _ => {}
        }
    }

    None
}

/// Collapse multi-line method chains around the cursor into a single line.
///
/// When the cursor line (after trimming leading whitespace) begins with
/// `->` or `?->`, this function walks backwards through preceding lines
/// that are also continuations, plus the base expression line, and joins
/// them into one flattened string.  The returned column is the cursor's
/// position within that flattened string.
///
/// If the cursor line is not a continuation, the original line and column
/// are returned unchanged.
///
/// # Returns
///
/// `(collapsed_line, adjusted_column)` — the flattened text and the
/// cursor's character offset within it.
pub(crate) fn collapse_continuation_lines(
    lines: &[&str],
    cursor_line: usize,
    cursor_col: usize,
) -> (String, usize) {
    let line = lines[cursor_line];
    let trimmed = line.trim_start();

    // Only collapse when the cursor line is a continuation (starts with
    // `->` or `?->` after optional whitespace).
    if !trimmed.starts_with("->") && !trimmed.starts_with("?->") {
        return (line.to_string(), cursor_col);
    }

    let cursor_leading_ws = line.len() - trimmed.len();

    // Walk backwards to find the first non-continuation line (the base).
    //
    // A continuation line is one that starts with `->` or `?->`.  However,
    // multi-line closure/function arguments can break the chain:
    //
    //   Brand::whereNested(function (Builder $q): void {
    //   })
    //   ->   // ← cursor
    //
    // Here line `})` is NOT a continuation but is part of the call
    // expression on the base line.  We detect this by tracking
    // brace/paren balance: when the accumulated lines (from the current
    // candidate upwards to the cursor) have unmatched closing delimiters,
    // we keep walking backwards until the delimiters balance out.
    let mut start = cursor_line;
    while start > 0 {
        let prev_trimmed = lines[start - 1].trim_start();

        // Skip blank (whitespace-only) lines — they don't terminate a
        // chain.  Without this, a blank line between chain segments
        // causes the backward walk to stop prematurely.
        if prev_trimmed.is_empty() {
            start -= 1;
            continue;
        }

        if prev_trimmed.starts_with("->") || prev_trimmed.starts_with("?->") {
            start -= 1;
        } else {
            // Check whether the accumulated text from this candidate
            // line through the line just before the cursor has
            // unbalanced closing delimiters.  If so, this line is in
            // the middle of a multi-line argument list and we must
            // keep walking backwards.
            start -= 1;

            // Count paren/brace balance from `start` up to (but not
            // including) the cursor line.
            let mut paren_depth: i32 = 0;
            let mut brace_depth: i32 = 0;
            for line in lines.iter().take(cursor_line).skip(start) {
                for ch in line.chars() {
                    match ch {
                        '(' => paren_depth += 1,
                        ')' => paren_depth -= 1,
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }
            }

            // If balanced (or net-open), this is a proper base line.
            if paren_depth >= 0 && brace_depth >= 0 {
                break;
            }

            // Unbalanced — keep walking backwards until we close the
            // gap.  Each step re-checks the running balance.
            while start > 0 && (paren_depth < 0 || brace_depth < 0) {
                start -= 1;
                for ch in lines[start].chars() {
                    match ch {
                        '(' => paren_depth += 1,
                        ')' => paren_depth -= 1,
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }
            }

            // After re-balancing we may have landed on a continuation
            // line (e.g. `->where(...\n...\n)->`) — keep walking if so.
            if start > 0 {
                let landed = lines[start].trim_start();
                if landed.starts_with("->") || landed.starts_with("?->") {
                    continue;
                }
            }
            break;
        }
    }

    // Build the collapsed string from the base line through the cursor line,
    // skipping blank lines so they don't leave gaps in the collapsed result.
    let mut prefix = String::new();
    for (i, line) in lines.iter().enumerate().take(cursor_line).skip(start) {
        let piece = if i == start {
            line.trim_end()
        } else {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            t
        };
        prefix.push_str(piece);
    }

    // The cursor position in the collapsed string is the length of the
    // prefix (everything before the cursor line) plus the cursor's offset
    // within the trimmed cursor line.
    let new_col = prefix.chars().count() + (cursor_col.saturating_sub(cursor_leading_ws));

    prefix.push_str(trimmed);

    (prefix, new_col)
}
