//! The SIMD byte-lexer fast path shared by both scanners.
//!
//! [`find_classes`] (the PSR-4 scanner) and [`find_symbols`] (the
//! full-scan) are single-pass state machines that walk raw PHP source
//! bytes without building an AST.  This file is intentionally kept as
//! an isolated, self-contained unit: it owns the state machine (string
//! / comment / heredoc skipping, keyword-boundary detection, name
//! reading) and the `memchr`/`memmem`-backed helpers that give it its
//! performance. Do not fold this logic back into a general parser —
//! see the module-level docs on [`super`] for why these scanners
//! exist alongside the full AST parser.

use memchr::{memchr, memmem};

use super::ScanResult;

/// The **full-scan**: a single-pass byte-level scanner that extracts
/// fully-qualified class, function, and constant names from PHP source
/// bytes.
///
/// This is the extended version of [`find_classes`] (the PSR-4 scanner)
/// that also recognises `function` declarations, `define()` calls, and
/// top-level `const` statements.  It is used for both non-Composer
/// projects (full workspace scan) and Composer autoload files
/// (`autoload_files.php` and their `require_once` chains).
pub fn find_symbols(content: &[u8]) -> ScanResult {
    // Quick rejection — if the file has none of the relevant keywords
    // we can bail immediately.
    if !has_any_keyword(content) {
        return ScanResult::default();
    }
    let mut result = ScanResult::default();
    scan::<true>(content, &mut result);
    result
}

/// The **PSR-4 scanner**: a single-pass byte-level scanner that
/// extracts fully-qualified class, interface, trait, and enum names
/// from PHP source bytes.
///
/// This is the classes-only scanner used by the PSR-4 directory walker
/// and vendor package scanner.  For a scanner that also extracts
/// functions and constants, see [`find_symbols`] (the full-scan).
pub fn find_classes(content: &[u8]) -> Vec<String> {
    // Quick rejection — use SIMD to check if any class-like keywords exist
    if !has_class_keyword(content) {
        return Vec::new();
    }
    let mut result = ScanResult {
        classes: Vec::with_capacity(4),
        ..ScanResult::default()
    };
    scan::<false>(content, &mut result);
    result.classes
}

/// The state machine behind both scanners, collecting into `result`.
///
/// Skips comments, strings, heredocs, and nowdocs inline without
/// allocating a separate "cleaned" buffer.  With `SYMBOLS` it also
/// tracks brace depth and collects top-level functions and constants;
/// without it, it collects only class-likes and compiles down to the
/// lean PSR-4 loop.
fn scan<const SYMBOLS: bool>(content: &[u8], result: &mut ScanResult) {
    let mut namespace = String::new();
    let len = content.len();
    let mut i = 0;

    // Brace depth tracking for top-level `function` / `const` detection
    // (`SYMBOLS` only). Depth 0 = top-level, depth 1 = inside a
    // class/namespace block.
    let mut brace_depth: u32 = 0;
    let mut in_braced_namespace = false;
    // The brace depth at which the current namespace was opened.
    // `const` declarations at this depth (or depth 0 outside braced
    // namespaces) are top-level.
    let mut namespace_brace_depth: u32 = 0;
    // Whether the scan is inside a class-like or function body, and the
    // depth that body's brace opened at. A `function` outside every body
    // declares a function even within a block, as in the
    // `if (! function_exists('foo')) { function foo() {} }` guard helper
    // files are written with.
    let mut in_body = false;
    let mut body_brace_depth: u32 = 0;
    // A class-like or function keyword was read outside every body, so
    // the next `{` opens one.
    let mut body_pending = false;

    // State flags
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_single_string = false;
    let mut in_double_string = false;
    let mut in_heredoc = false;
    let mut heredoc_id: &[u8] = &[];

    while i < len {
        // ── Skip: line comment (memchr to newline) ──────────────────
        if in_line_comment {
            if let Some(pos) = memchr(b'\n', &content[i..]) {
                i += pos + 1;
            } else {
                break; // rest of file is a comment
            }
            in_line_comment = false;
            continue;
        }

        // ── Skip: block comment (memmem to "*/") ────────────────────
        if in_block_comment {
            if let Some(pos) = memmem::find(&content[i..], b"*/") {
                i += pos + 2;
                in_block_comment = false;
            } else {
                break; // unclosed block comment
            }
            continue;
        }

        // ── Skip: single-quoted string (memchr to '\'' or '\\') ────
        if in_single_string {
            match memchr2_single_string(&content[i..]) {
                Some((offset, b'\\')) => {
                    i += offset + 2; // skip escaped char
                }
                Some((offset, _)) => {
                    // Found closing quote
                    i += offset + 1;
                    in_single_string = false;
                }
                None => break, // unclosed string
            }
            continue;
        }

        // ── Skip: double-quoted string (memchr to '"' or '\\') ─────
        if in_double_string {
            match memchr2_double_string(&content[i..]) {
                Some((offset, b'\\')) => {
                    i += offset + 2; // skip escaped char
                }
                Some((offset, _)) => {
                    // Found closing quote
                    i += offset + 1;
                    in_double_string = false;
                }
                None => break, // unclosed string
            }
            continue;
        }

        // ── Skip: heredoc / nowdoc (memchr to newline) ──────────────
        if in_heredoc {
            let line_start = i;
            // Skip leading whitespace (PHP 7.3+ flexible heredoc)
            while i < len && (content[i] == b' ' || content[i] == b'\t') {
                i += 1;
            }
            if i + heredoc_id.len() <= len && &content[i..i + heredoc_id.len()] == heredoc_id {
                let after = i + heredoc_id.len();
                if after >= len
                    || content[after] == b';'
                    || content[after] == b'\n'
                    || content[after] == b'\r'
                    || content[after] == b','
                    || content[after] == b')'
                {
                    in_heredoc = false;
                    i = after;
                    continue;
                }
            }
            // Skip to next line
            i = line_start;
            if let Some(pos) = memchr(b'\n', &content[i..]) {
                i += pos + 1;
            } else {
                break; // rest of file is inside heredoc
            }
            continue;
        }

        // ── Main code parsing ───────────────────────────────────────
        let b = content[i];

        // Braces for depth tracking
        if SYMBOLS {
            if b == b'{' {
                if body_pending {
                    body_pending = false;
                    in_body = true;
                    body_brace_depth = brace_depth;
                }
                brace_depth += 1;
                i += 1;
                continue;
            }
            if b == b'}' {
                brace_depth = brace_depth.saturating_sub(1);
                if in_body && brace_depth == body_brace_depth {
                    in_body = false;
                }
                // Exiting a braced namespace block resets the namespace.
                if in_braced_namespace && brace_depth == namespace_brace_depth {
                    in_braced_namespace = false;
                    namespace.clear();
                }
                i += 1;
                continue;
            }
        }

        // Comments: // and /* */
        if b == b'/' && i + 1 < len {
            if content[i + 1] == b'/' {
                in_line_comment = true;
                i += 2;
                continue;
            }
            if content[i + 1] == b'*' {
                in_block_comment = true;
                i += 2;
                continue;
            }
        }

        // Hash comments (but not PHP attributes #[...])
        if b == b'#' {
            if i + 1 < len && content[i + 1] == b'[' {
                // PHP attribute — skip past it (it's not a comment)
                i += 1;
                continue;
            }
            in_line_comment = true;
            i += 1;
            continue;
        }

        // Strings
        if b == b'\'' {
            in_single_string = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_double_string = true;
            i += 1;
            continue;
        }

        // Heredoc / nowdoc: <<<
        if b == b'<' && i + 2 < len && content[i + 1] == b'<' && content[i + 2] == b'<' {
            i += 3;
            while i < len && content[i] == b' ' {
                i += 1;
            }
            // Skip optional quote (nowdoc uses single quotes)
            if i < len && (content[i] == b'\'' || content[i] == b'"') {
                i += 1;
            }
            let id_start = i;
            while i < len && (content[i].is_ascii_alphanumeric() || content[i] == b'_') {
                i += 1;
            }
            if i > id_start {
                heredoc_id = &content[id_start..i];
                in_heredoc = true;
                // Skip closing quote
                if i < len && (content[i] == b'\'' || content[i] == b'"') {
                    i += 1;
                }
                while i < len && content[i] != b'\n' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
            }
            continue;
        }

        // ── Keyword detection ───────────────────────────────────────
        // Only match at valid keyword boundaries to avoid matching
        // property accesses like `$node->class`.
        if is_keyword_boundary(content, i) {
            if b == b'n'
                && i + 9 <= len
                && &content[i..i + 9] == b"namespace"
                && (i + 9 >= len
                    || content[i + 9].is_ascii_whitespace()
                    || content[i + 9] == b';'
                    || content[i + 9] == b'{')
            {
                i += 9;
                while i < len && content[i].is_ascii_whitespace() {
                    i += 1;
                }

                // Braced (`namespace Foo { ... }`) or semicolon form;
                // either way, read the name.
                let ns_start = i;
                while i < len {
                    let c = content[i];
                    if c.is_ascii_alphanumeric()
                        || c == b'_'
                        || c == b'\\'
                        || c.is_ascii_whitespace()
                    {
                        i += 1;
                    } else {
                        break;
                    }
                }
                namespace = content[ns_start..i]
                    .iter()
                    .filter(|&&c| !c.is_ascii_whitespace())
                    .map(|&c| c as char)
                    .collect();
                if !namespace.is_empty() && !namespace.ends_with('\\') {
                    namespace.push('\\');
                }

                // A braced namespace ends at its closing brace.
                if SYMBOLS && i < len && content[i] == b'{' {
                    in_braced_namespace = true;
                    namespace_brace_depth = brace_depth;
                    brace_depth += 1;
                    i += 1;
                }
                continue;
            }

            if let Some(keyword_len) = class_keyword_at(content, i) {
                i += keyword_len;
                if SYMBOLS && !in_body {
                    body_pending = true;
                }
                if let Some(name) = read_name(content, &mut i) {
                    result.classes.push(format!("{namespace}{name}"));
                }
                continue;
            }

            if SYMBOLS {
                // An anonymous class, `new class(…) {` or `new class{`,
                // which `class_keyword_at` leaves alone: its body still
                // holds methods, not functions.
                if !in_body
                    && b == b'c'
                    && content[i..].starts_with(b"class")
                    && matches!(content.get(i + 5), Some(b'(' | b'{'))
                {
                    body_pending = true;
                    i += 5;
                    continue;
                }

                // Only top-level declarations: depth 0 outside a braced
                // namespace, or directly inside the braced namespace.
                let is_top_level = || {
                    if in_braced_namespace {
                        brace_depth == namespace_brace_depth + 1
                    } else {
                        brace_depth == 0
                    }
                };

                // function (standalone — not inside a class-like or
                // function body)
                if b == b'f'
                    && i + 8 <= len
                    && &content[i..i + 8] == b"function"
                    && (i + 8 >= len
                        || content[i + 8].is_ascii_whitespace()
                        || content[i + 8] == b'(')
                {
                    i += 8;
                    // Skip `use function …;` import statements — these
                    // are not function declarations.
                    if is_preceded_by_use(content, i - 8) {
                        skip_use_statement(content, &mut i);
                    } else if !in_body {
                        body_pending = true;
                        // Skip `function (` — that's a closure, not a named function.
                        let mut j = i;
                        while j < len && content[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        if j < len && content[j] == b'(' {
                            // Anonymous function / closure — skip.
                            i = j;
                        } else if let Some(name) = read_name(content, &mut i) {
                            result.functions.push(format!("{namespace}{name}"));
                        }
                    }
                    continue;
                }

                // define('NAME', ...)
                if b == b'd'
                    && i + 6 <= len
                    && &content[i..i + 6] == b"define"
                    && (i + 6 < len && content[i + 6] == b'(')
                {
                    i += 7; // skip `define(`
                    while i < len && content[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    // Read the constant name from the string argument.
                    if let Some(name) = read_define_name(content, &mut i) {
                        result.constants.push(name.to_string());
                    }
                    continue;
                }

                // const NAME = ... (top-level only)
                if b == b'c'
                    && i + 5 <= len
                    && &content[i..i + 5] == b"const"
                    && (i + 5 >= len || content[i + 5].is_ascii_whitespace())
                {
                    i += 5;
                    // Skip `use const …;` import statements.
                    if is_preceded_by_use(content, i - 5) {
                        skip_use_statement(content, &mut i);
                    } else if is_top_level()
                        && let Some(name) = read_name(content, &mut i)
                    {
                        // Top-level const names are FQN with namespace.
                        result.constants.push(format!("{namespace}{name}"));
                    }
                    continue;
                }
            }
        }

        i += 1;
    }
}

/// The length of the `class`, `interface`, `trait` or `enum` keyword
/// starting at `i` and followed by whitespace, if there is one.
#[inline(always)]
fn class_keyword_at(content: &[u8], i: usize) -> Option<usize> {
    let keyword: &[u8] = match content[i] {
        b'c' => b"class",
        b'i' => b"interface",
        b't' => b"trait",
        b'e' => b"enum",
        _ => return None,
    };
    let end = i + keyword.len();
    (end <= content.len()
        && &content[i..end] == keyword
        && (end >= content.len() || content[end].is_ascii_whitespace()))
    .then_some(keyword.len())
}

/// Advance `i` past the rest of a `use function` / `use const` import so
/// the names it lists are not mistaken for declarations.
#[inline]
fn skip_use_statement(content: &[u8], i: &mut usize) {
    let len = content.len();
    while *i < len && content[*i] != b';' && content[*i] != b'\n' {
        *i += 1;
    }
    if *i < len && content[*i] == b';' {
        *i += 1;
    }
}

/// SIMD-accelerated pre-screening: check whether the content contains
/// any of the class-like keywords.
#[inline]
fn has_class_keyword(content: &[u8]) -> bool {
    memmem::find(content, b"class").is_some()
        || memmem::find(content, b"interface").is_some()
        || memmem::find(content, b"trait").is_some()
        || memmem::find(content, b"enum").is_some()
}

/// SIMD-accelerated pre-screening: check whether the content contains
/// any keyword relevant to symbol extraction (classes, functions,
/// constants).
#[inline]
fn has_any_keyword(content: &[u8]) -> bool {
    memmem::find(content, b"class").is_some()
        || memmem::find(content, b"interface").is_some()
        || memmem::find(content, b"trait").is_some()
        || memmem::find(content, b"enum").is_some()
        || memmem::find(content, b"function").is_some()
        || memmem::find(content, b"define").is_some()
        || memmem::find(content, b"const").is_some()
}

/// Check if a character is a valid boundary (not part of an identifier).
#[inline]
fn is_boundary_char(c: u8) -> bool {
    !c.is_ascii_alphanumeric() && c != b'_' && c != b':' && c != b'$'
}

/// Find the next single-quote or backslash in a slice, returning the
/// offset and the byte found.  Uses `memchr` for SIMD acceleration.
#[inline]
fn memchr2_single_string(haystack: &[u8]) -> Option<(usize, u8)> {
    memchr::memchr2(b'\'', b'\\', haystack).map(|pos| (pos, haystack[pos]))
}

/// Find the next double-quote or backslash in a slice, returning the
/// offset and the byte found.  Uses `memchr` for SIMD acceleration.
#[inline]
fn memchr2_double_string(haystack: &[u8]) -> Option<(usize, u8)> {
    memchr::memchr2(b'"', b'\\', haystack).map(|pos| (pos, haystack[pos]))
}

/// Check whether the keyword at position `i` is preceded by `use `
/// (with optional whitespace), indicating a `use function` or `use const`
/// import statement rather than a declaration.
fn is_preceded_by_use(content: &[u8], i: usize) -> bool {
    if i < 4 {
        return false;
    }
    // Walk backwards over whitespace.
    let mut j = i - 1;
    while j > 0 && content[j].is_ascii_whitespace() {
        j -= 1;
    }
    // Check for `use` (the 'e' is at j, 'u' at j-2).
    if j >= 2 && &content[j - 2..=j] == b"use" {
        // Make sure `use` itself is at a keyword boundary (not part
        // of a longer identifier like `reuse`).
        if j - 2 == 0 || is_boundary_char(content[j - 3]) {
            return true;
        }
    }
    false
}

/// Check whether a keyword can start at this offset.
///
/// Rejects property accesses like `$node->class` and
/// `$node?->class` to avoid false positives.
#[inline]
fn is_keyword_boundary(content: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }

    let prev = content[i - 1];
    if !is_boundary_char(prev) {
        return false;
    }

    // Reject object/nullsafe property access: ->class, ?->class
    if prev == b'>' && i >= 2 {
        let prev2 = content[i - 2];
        if prev2 == b'-' || prev2 == b'?' {
            return false;
        }
    }

    true
}

/// Read the constant name from the first argument of a `define()` call.
///
/// Expects `i` to point at the first character after `define(` (with
/// optional whitespace already skipped).  Handles both single-quoted
/// and double-quoted string literals.  Returns the raw name string
/// (without quotes).
#[inline]
fn read_define_name<'a>(content: &'a [u8], i: &mut usize) -> Option<&'a str> {
    let len = content.len();
    if *i >= len {
        return None;
    }
    let quote = content[*i];
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    *i += 1; // skip opening quote
    let start = *i;
    while *i < len && content[*i] != quote {
        if content[*i] == b'\\' && *i + 1 < len {
            let next = content[*i + 1];
            if next == quote || next == b'\\' {
                // Escaped quote or escaped backslash — the name
                // contains a real escape sequence, which is unusual
                // for constant names.  Bail out.
                return None;
            }
            // A bare backslash (e.g. namespace separator in
            // 'App\Config\DB_HOST') is literal in single-quoted
            // strings and safe to include.
        }
        *i += 1;
    }
    if *i >= len {
        return None;
    }
    let name = &content[start..*i];
    *i += 1; // skip closing quote
    std::str::from_utf8(name).ok()
}

/// Read a class/interface/trait/enum name after the keyword.
///
/// Skips whitespace, then reads an identifier.  Returns `None` for
/// keywords like `extends`/`implements` that can follow `class` in
/// anonymous class expressions (`new class extends Foo {}`).
#[inline]
fn read_name<'a>(content: &'a [u8], i: &mut usize) -> Option<&'a str> {
    let len = content.len();

    while *i < len && content[*i].is_ascii_whitespace() {
        *i += 1;
    }

    let start = *i;

    // Read identifier characters
    while *i < len {
        let c = content[*i];
        if c.is_ascii_alphanumeric() || c == b'_' {
            *i += 1;
        } else {
            break;
        }
    }

    if *i == start {
        return None;
    }

    let name = &content[start..*i];

    // Skip keywords that appear in anonymous class expressions
    if name == b"extends" || name == b"implements" {
        return None;
    }

    std::str::from_utf8(name).ok()
}

#[cfg(test)]
#[path = "lexer_tests.rs"]
mod tests;
