//! Byte-level scanning for the constructs that make a missing symbol
//! legitimate: an `isset()`/`empty()` argument, and an existence check
//! (`function_exists`, `class_exists`, `method_exists`) a call is written
//! under.
//!
//! Textual rather than AST-based because these run over a buffer that is
//! mid-edit and often does not parse.

use std::collections::HashMap;

use super::helpers::ByteRange;
use crate::text_scan::{find_matching_forward_bytes, find_unmatched_close_bytes};

/// Compute the byte ranges of `isset(...)` and `empty(...)` argument lists.
///
/// A member or array-index access inside these constructs never triggers
/// a runtime error or warning even when the accessed member doesn't
/// exist — that is the entire purpose of `isset()`/`empty()`.  Callers
/// use this to suppress unknown-member, unresolved-member, and
/// scalar-member-access diagnostics for spans that fall inside one of
/// these ranges.
pub(crate) fn compute_isset_empty_argument_ranges(content: &str) -> Vec<ByteRange> {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < len {
        let after_name = if matches_ident(bytes, i, b"isset") {
            Some(i + b"isset".len())
        } else if matches_ident(bytes, i, b"empty") {
            Some(i + b"empty".len())
        } else {
            None
        };
        if let Some(after_name) = after_name {
            // Must not be preceded by an identifier character (avoid
            // matching a variable/function named `myisset`).
            let preceded_by_ident = i > 0 && is_ident_char(bytes[i - 1]);
            if !preceded_by_ident {
                let paren_start = skip_ws(bytes, after_name);
                if paren_start < len
                    && bytes[paren_start] == b'('
                    && let Some(paren_end) =
                        find_matching_forward_bytes(bytes, paren_start, b'(', b')')
                {
                    ranges.push((paren_start + 1, paren_end));
                    i = paren_end + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    ranges
}

/// Information about symbols guarded by existence checks.
///
/// When code is wrapped in `if (function_exists('foo')) { foo(); }`, the
/// call to `foo()` should not produce an "unknown function" diagnostic
/// because the developer explicitly checked for its existence.
pub(crate) struct ExistenceGuards {
    /// Function names guarded by `function_exists('name')`.
    /// Maps function name (lowercase) to list of guarded byte ranges.
    pub function_guards: HashMap<String, Vec<ByteRange>>,
    /// Class names guarded by `class_exists(Name::class)` or `class_exists('Name')`.
    /// Maps class name (case-preserved) to list of guarded byte ranges.
    pub class_guards: HashMap<String, Vec<ByteRange>>,
    /// Method names guarded by `method_exists($obj, 'name')`.
    /// Maps method name (lowercase) to list of guarded byte ranges.
    pub method_guards: HashMap<String, Vec<ByteRange>>,
}

impl ExistenceGuards {
    /// Check whether a function call at `offset` is guarded by `function_exists()`.
    pub fn is_function_guarded(&self, name: &str, offset: u32) -> bool {
        self.function_guards
            .get(&name.to_lowercase())
            .is_some_and(|ranges| {
                ranges
                    .iter()
                    .any(|&(start, end)| (offset as usize) >= start && (offset as usize) < end)
            })
    }

    /// Check whether a class reference at `offset` is guarded by `class_exists()`.
    pub fn is_class_guarded(&self, name: &str, offset: u32) -> bool {
        // Check the name as-is first, then try the short name.
        self.class_guards
            .get(name)
            .or_else(|| {
                let short = name.rsplit('\\').next().unwrap_or(name);
                if short != name {
                    self.class_guards.get(short)
                } else {
                    None
                }
            })
            .is_some_and(|ranges| {
                ranges
                    .iter()
                    .any(|&(start, end)| (offset as usize) >= start && (offset as usize) < end)
            })
    }

    /// Check whether a member access at `offset` is guarded by `method_exists()`.
    pub fn is_method_guarded(&self, name: &str, offset: u32) -> bool {
        self.method_guards
            .get(&name.to_lowercase())
            .is_some_and(|ranges| {
                ranges
                    .iter()
                    .any(|&(start, end)| (offset as usize) >= start && (offset as usize) < end)
            })
    }
}

/// The kind of existence check detected.
enum ExistenceKind {
    Function,
    Class,
    Method,
}

/// Scan the source for existence-check guards and compute the byte
/// ranges they protect.
///
/// Detects:
/// - `function_exists('name')` / `function_exists("name")`
/// - `class_exists(Name::class)` / `class_exists('Name')` / `class_exists("Name")`
/// - `method_exists($var, 'name')` / `method_exists($var, "name")`
///
/// Negated checks (`!function_exists(...)`) are skipped because they
/// typically guard polyfill definitions, not usage of the symbol.
pub(crate) fn compute_existence_guards(content: &str) -> ExistenceGuards {
    let mut guards = ExistenceGuards {
        function_guards: HashMap::new(),
        class_guards: HashMap::new(),
        method_guards: HashMap::new(),
    };

    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if let Some((kind, name, call_end)) = try_parse_existence_call(bytes, i) {
            let negated = is_negated(bytes, i);
            let guarded_range = if negated {
                // Negated check with early exit: `if (!exists('x')) return;`
                // guards code AFTER the if-statement.
                find_negated_guard_range(bytes, i, call_end)
            } else {
                find_guarded_range(bytes, i, call_end)
            };
            if let Some(guarded_range) = guarded_range {
                match kind {
                    ExistenceKind::Function => {
                        guards
                            .function_guards
                            .entry(name.to_lowercase())
                            .or_default()
                            .push(guarded_range);
                    }
                    ExistenceKind::Class => {
                        guards
                            .class_guards
                            .entry(name)
                            .or_default()
                            .push(guarded_range);
                    }
                    ExistenceKind::Method => {
                        guards
                            .method_guards
                            .entry(name.to_lowercase())
                            .or_default()
                            .push(guarded_range);
                    }
                }
            }
            i = call_end;
        } else {
            i += 1;
        }
    }

    guards
}

/// Check if the existence call at position `start` is negated by a `!`.
fn is_negated(bytes: &[u8], start: usize) -> bool {
    // A global function written the explicit way (`!\class_exists(…)`)
    // carries a `\` between the `!` and the name, with no room for
    // whitespace in between. Step over it first, or the scan below reads
    // it as the character that ends the search and calls the check
    // un-negated.
    let mut j = start;
    if j > 0 && bytes[j - 1] == b'\\' {
        j -= 1;
    }
    // Scan backward, skipping whitespace, looking for `!`.
    while j > 0 {
        j -= 1;
        match bytes[j] {
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            b'!' => return true,
            _ => return false,
        }
    }
    false
}

/// For negated existence checks like `if (!function_exists('foo')) return;`,
/// determine the guard range: from the end of the if-statement to the end
/// of the enclosing scope (next `}` at depth 0, or end of file).
///
/// Only applies when the if-body is an early termination statement
/// (`return`, `throw`, `die`, `exit`, `continue`, `break`).
fn find_negated_guard_range(
    bytes: &[u8],
    call_start: usize,
    _call_end: usize,
) -> Option<ByteRange> {
    let len = bytes.len();

    let if_pos = find_preceding_if(bytes, call_start)?;

    let paren_start = skip_ws(bytes, if_pos + 2);
    if paren_start >= len || bytes[paren_start] != b'(' {
        return None;
    }

    let cond_end = find_matching_forward_bytes(bytes, paren_start, b'(', b')')?;

    let body_start_pos = skip_ws(bytes, cond_end + 1);
    if body_start_pos >= len {
        return None;
    }

    // Determine end of the if-statement and check for early exit.
    let if_stmt_end = if bytes[body_start_pos] == b'{' {
        let block_end = find_matching_forward_bytes(bytes, body_start_pos, b'{', b'}')?;
        let body_content = &bytes[body_start_pos + 1..block_end];
        if !contains_early_exit(body_content) {
            return None;
        }
        block_end + 1
    } else {
        // Single-statement body: find `;`
        let mut s = body_start_pos;
        while s < len && bytes[s] != b';' {
            s += 1;
        }
        if s >= len {
            return None;
        }
        let stmt_content = &bytes[body_start_pos..s];
        if !contains_early_exit(stmt_content) {
            return None;
        }
        s + 1
    };

    // Guard from end of the if-statement to end of enclosing scope.
    let scope_end = find_enclosing_scope_end(bytes, if_stmt_end);
    Some((if_stmt_end, scope_end))
}

/// Check if a byte slice contains an early-exit keyword at the start.
fn contains_early_exit(body: &[u8]) -> bool {
    let s = String::from_utf8_lossy(body);
    let trimmed = s.trim();
    trimmed.starts_with("return")
        || trimmed.starts_with("throw")
        || trimmed.starts_with("die")
        || trimmed.starts_with("exit")
        || trimmed.starts_with("continue")
        || trimmed.starts_with("break")
}

/// Find the end of the enclosing scope from a given position: the next
/// `}` that does not close a block opened after `from`, or EOF.
fn find_enclosing_scope_end(bytes: &[u8], from: usize) -> usize {
    find_unmatched_close_bytes(bytes, from, b'{', b'}').unwrap_or(bytes.len())
}

/// Try to parse an existence-check call starting at position `i`.
///
/// Returns `(kind, extracted_name, position_after_closing_paren)` on success.
fn try_parse_existence_call(bytes: &[u8], i: usize) -> Option<(ExistenceKind, String, usize)> {
    let len = bytes.len();

    // Match one of the three function names.
    let (kind, after_name) = if matches_ident(bytes, i, b"function_exists") {
        (ExistenceKind::Function, i + b"function_exists".len())
    } else if matches_ident(bytes, i, b"class_exists") {
        (ExistenceKind::Class, i + b"class_exists".len())
    } else if matches_ident(bytes, i, b"method_exists") {
        (ExistenceKind::Method, i + b"method_exists".len())
    } else {
        return None;
    };

    // Must not be preceded by an identifier character (avoid matching
    // `my_function_exists`).
    if i > 0 && is_ident_char(bytes[i - 1]) {
        return None;
    }

    // Skip whitespace and expect `(`.
    let mut pos = skip_ws(bytes, after_name);
    if pos >= len || bytes[pos] != b'(' {
        return None;
    }
    pos += 1; // skip `(`

    // Extract arguments based on kind.
    match kind {
        ExistenceKind::Function => {
            // Expect a string literal: 'name' or "name"
            pos = skip_ws(bytes, pos);
            let (name, after_str) = extract_string_literal(bytes, pos)?;
            pos = skip_ws(bytes, after_str);
            if pos >= len || bytes[pos] != b')' {
                return None;
            }
            Some((kind, name, pos + 1))
        }
        ExistenceKind::Class => {
            // Expect either Name::class or a string literal.
            pos = skip_ws(bytes, pos);
            if let Some((name, after)) = try_extract_class_const(bytes, pos) {
                let after = skip_ws(bytes, after);
                if after >= len || bytes[after] != b')' {
                    return None;
                }
                Some((kind, name, after + 1))
            } else if let Some((name, after_str)) = extract_string_literal(bytes, pos) {
                let after_str = skip_ws(bytes, after_str);
                if after_str >= len || bytes[after_str] != b')' {
                    return None;
                }
                Some((kind, name, after_str + 1))
            } else {
                None
            }
        }
        ExistenceKind::Method => {
            // Skip first argument (any expression), find comma, extract second string literal.
            // Simple approach: count parens to skip first arg until comma at depth 0.
            let mut depth = 0u32;
            while pos < len {
                match bytes[pos] {
                    b'(' => depth += 1,
                    b')' => {
                        if depth == 0 {
                            return None; // no comma found
                        }
                        depth -= 1;
                    }
                    b',' if depth == 0 => break,
                    _ => {}
                }
                pos += 1;
            }
            if pos >= len || bytes[pos] != b',' {
                return None;
            }
            pos += 1; // skip comma
            pos = skip_ws(bytes, pos);
            let (name, after_str) = extract_string_literal(bytes, pos)?;
            let after_str = skip_ws(bytes, after_str);
            if after_str >= len || bytes[after_str] != b')' {
                return None;
            }
            Some((kind, name, after_str + 1))
        }
    }
}

/// Try to extract `Name::class` pattern, returning the class name.
fn try_extract_class_const(bytes: &[u8], pos: usize) -> Option<(String, usize)> {
    let len = bytes.len();
    // Expect an identifier (possibly with backslashes) followed by `::class`.
    let start = pos;
    let mut end = pos;
    while end < len && (is_ident_char(bytes[end]) || bytes[end] == b'\\') {
        end += 1;
    }
    if end == start {
        return None;
    }
    if end + 7 > len {
        return None;
    }
    if &bytes[end..end + 7] != b"::class" {
        return None;
    }
    let name = String::from_utf8_lossy(&bytes[start..end]).to_string();
    Some((name, end + 7))
}

/// Extract a single- or double-quoted string literal at `pos`.
fn extract_string_literal(bytes: &[u8], pos: usize) -> Option<(String, usize)> {
    let len = bytes.len();
    if pos >= len {
        return None;
    }
    let quote = bytes[pos];
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    let start = pos + 1;
    let mut end = start;
    while end < len && bytes[end] != quote {
        if bytes[end] == b'\\' {
            end += 1; // skip escaped char
        }
        end += 1;
    }
    if end >= len {
        return None;
    }
    // The runtime value, not the source text: a class named in a guard is
    // written `'Vendor\\Optional\\Config'` as often as `'Vendor\Optional\Config'`,
    // and only one of those matches the reference it guards unless the
    // escapes are resolved first.
    let raw = String::from_utf8_lossy(&bytes[pos..=end]);
    let name = crate::util::unescape_php_string_literal(&raw)
        .unwrap_or_else(|| String::from_utf8_lossy(&bytes[start..end]).to_string());
    Some((name, end + 1))
}

/// Determine the byte range guarded by an existence check.
///
/// Strategy: from the call position, find the enclosing `if` statement
/// and return the body range. For `&&` chains without a clear if-body,
/// guard to end of statement.
fn find_guarded_range(bytes: &[u8], call_start: usize, call_end: usize) -> Option<ByteRange> {
    let len = bytes.len();

    // Strategy 1: Find enclosing `if` by scanning backward.
    if let Some(if_pos) = find_preceding_if(bytes, call_start) {
        let paren_start = skip_ws(bytes, if_pos + 2); // skip "if"
        if paren_start < len
            && bytes[paren_start] == b'('
            && let Some(cond_end) = find_matching_forward_bytes(bytes, paren_start, b'(', b')')
        {
            let body_start_pos = skip_ws(bytes, cond_end + 1);
            if body_start_pos < len {
                if bytes[body_start_pos] == b'{' {
                    // Block body: find matching `}`.
                    if let Some(block_end) =
                        find_matching_forward_bytes(bytes, body_start_pos, b'{', b'}')
                    {
                        // Guard covers from start of condition (to catch && patterns
                        // in the condition itself) through the block end.
                        return Some((paren_start, block_end + 1));
                    }
                } else {
                    // Single-statement body: find `;`.
                    let mut s = body_start_pos;
                    while s < len && bytes[s] != b';' {
                        s += 1;
                    }
                    if s < len {
                        return Some((paren_start, s + 1));
                    }
                }
            }
        }
    }

    // Strategy 2: No enclosing `if` found — guard from call_end to `;`.
    let mut s = call_end;
    while s < len && bytes[s] != b';' {
        s += 1;
    }
    if s < len {
        return Some((call_end, s + 1));
    }

    None
}

/// Scan backward from `pos` to find `if` keyword (within 200 chars).
fn find_preceding_if(bytes: &[u8], pos: usize) -> Option<usize> {
    let search_start = pos.saturating_sub(200);
    let mut j = pos;
    while j >= search_start + 2 {
        j -= 1;
        // Look for `if` preceded by non-ident and followed by whitespace or `(`.
        if bytes[j] == b'i' && j + 1 < bytes.len() && bytes[j + 1] == b'f' {
            // Check it's a word boundary.
            let before_ok = j == 0 || !is_ident_char(bytes[j - 1]);
            let after_ok = j + 2 >= bytes.len() || bytes[j + 2] == b' ' || bytes[j + 2] == b'(';
            if before_ok && after_ok {
                return Some(j);
            }
        }
        if j == 0 {
            break;
        }
    }
    None
}

/// Check if `bytes[i..]` starts with `ident` and is followed by non-ident.
fn matches_ident(bytes: &[u8], i: usize, ident: &[u8]) -> bool {
    let end = i + ident.len();
    if end > bytes.len() {
        return false;
    }
    if &bytes[i..end] != ident {
        return false;
    }
    // Must be followed by non-ident char (or EOF).
    end >= bytes.len() || !is_ident_char(bytes[end])
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_ws(bytes: &[u8], mut pos: usize) -> usize {
    let len = bytes.len();
    while pos < len
        && (bytes[pos] == b' ' || bytes[pos] == b'\t' || bytes[pos] == b'\n' || bytes[pos] == b'\r')
    {
        pos += 1;
    }
    pos
}
