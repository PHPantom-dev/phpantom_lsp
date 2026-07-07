//! Named argument completion for PHP 8.0+ syntax.
//!
//! When the cursor is inside the parentheses of a function or method call,
//! this module detects the call context and offers parameter names as
//! completion items with a trailing `:` so the user can quickly write
//! named arguments like `foo(paramName: $value)`.
//!
//! ## Supported call forms
//!
//! - Standalone functions: `foo(|)`
//! - Instance methods: `$this->method(|)`, `$var->method(|)`
//! - Static methods: `ClassName::method(|)`, `self::method(|)`
//! - Constructors: `new ClassName(|)`
//!
//! ## Smart features
//!
//! - Already-used named arguments are excluded from suggestions
//! - Positional arguments are counted to skip leading parameters
//! - The user's partial prefix is used for filtering

use tower_lsp::lsp_types::*;

// ─── Shared helpers ─────────────────────────────────────────────────────────

/// Check whether `cursor` (byte offset) sits inside a `[…]` or `{…}` pair
/// that is nested within the call's argument span starting at `args_start`.
///
/// Scans forward from `args_start` to `cursor`, tracking bracket and brace
/// depth while skipping string literals.  Returns `true` when the net depth
/// is > 0, meaning the cursor is inside an array literal or braced
/// expression — not at the top-level argument list of the call.
pub fn cursor_inside_nested_bracket(content: &str, args_start: usize, cursor: usize) -> bool {
    let bytes = content.as_bytes();
    let end = cursor.min(bytes.len());
    let mut i = args_start;
    let mut bracket_depth: i32 = 0; // tracks [ ]
    let mut brace_depth: i32 = 0; // tracks { }

    while i < end {
        match bytes[i] {
            b'[' => bracket_depth += 1,
            b']' => bracket_depth -= 1,
            b'{' => brace_depth += 1,
            b'}' => brace_depth -= 1,
            // Skip single-quoted strings
            b'\'' => {
                i += 1;
                while i < end {
                    if bytes[i] == b'\\' {
                        i += 1; // skip escaped char
                    } else if bytes[i] == b'\'' {
                        break;
                    }
                    i += 1;
                }
            }
            // Skip double-quoted strings
            b'"' => {
                i += 1;
                while i < end {
                    if bytes[i] == b'\\' {
                        i += 1; // skip escaped char
                    } else if bytes[i] == b'"' {
                        break;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    bracket_depth > 0 || brace_depth > 0
}

// ─── Context ────────────────────────────────────────────────────────────────

/// Information about a named-argument completion context.
#[derive(Debug, Clone)]
pub struct NamedArgContext {
    /// The call expression in a format suitable for resolution:
    /// - `"functionName"` for standalone functions
    /// - `"$this->method"` or `"$var->method"` for instance methods
    /// - `"ClassName::method"` or `"self::method"` for static methods
    /// - `"new ClassName"` for constructor calls
    pub call_expression: String,
    /// Parameter names already specified as named arguments in this call.
    pub existing_named_args: Vec<String>,
    /// Number of positional (non-named) arguments before the cursor.
    pub positional_count: usize,
    /// The partial identifier prefix the user has typed (e.g. `"na"` from `foo(na|)`).
    pub prefix: String,
}

// ─── Detection ──────────────────────────────────────────────────────────────

/// Detect whether the cursor is inside a function/method call and extract
/// the context needed for named-argument completion.
///
/// Returns `None` if the cursor is not at an eligible position (e.g. after
/// `$`, `->`, `::`, or inside a string/comment).
pub fn detect_named_arg_context(content: &str, position: Position) -> Option<NamedArgContext> {
    let chars: Vec<char> = content.chars().collect();
    let cursor = position_to_char_offset(&chars, position)?;

    // ── Check eligibility at cursor ─────────────────────────────────
    // Walk backward from cursor through identifier chars to find the
    // start of the current "word".
    let mut word_start = cursor;
    while word_start > 0
        && (chars[word_start - 1].is_alphanumeric() || chars[word_start - 1] == '_')
    {
        word_start -= 1;
    }

    // If preceded by `$`, this is a variable — not a named arg.
    if word_start > 0 && chars[word_start - 1] == '$' {
        return None;
    }

    // If preceded by `->` or `::`, member completion handles this.
    if word_start >= 2 && chars[word_start - 2] == '-' && chars[word_start - 1] == '>' {
        return None;
    }
    if word_start >= 2 && chars[word_start - 2] == ':' && chars[word_start - 1] == ':' {
        return None;
    }

    let prefix: String = chars[word_start..cursor].iter().collect();

    // ── Find enclosing open paren ───────────────────────────────────
    let open_paren = find_enclosing_open_paren(&chars, word_start)?;

    // ── Extract call expression before `(` ──────────────────────────
    let call_expr = extract_call_expression(&chars, open_paren)?;
    if call_expr.is_empty() {
        return None;
    }

    // ── Parse arguments between `(` and cursor ──────────────────────
    let args_text: String = chars[open_paren + 1..word_start].iter().collect();
    let (existing_named, positional_count) = parse_existing_args(&args_text);

    Some(NamedArgContext {
        call_expression: call_expr,
        existing_named_args: existing_named,
        positional_count,
        prefix,
    })
}

// Re-exported from `crate::util` for backward compatibility with
// existing import paths.
pub use crate::util::position_to_char_offset;

/// Walk backward from `start` (exclusive) to find the unmatched `(` that
/// encloses the cursor.
///
/// Skips balanced `(…)` pairs and string literals.  Returns `None` if no
/// enclosing `(` is found (cursor is not inside call parens).
pub fn find_enclosing_open_paren(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    let mut depth: i32 = 0;

    while i > 0 {
        i -= 1;
        match chars[i] {
            ')' => depth += 1,
            '(' => {
                if depth > 0 {
                    depth -= 1;
                } else {
                    // Found unmatched `(` — this is the call's open paren.
                    return Some(i);
                }
            }
            // Skip single-quoted strings backwards
            '\'' => {
                i = skip_string_backward(chars, i, '\'');
            }
            // Skip double-quoted strings backwards
            '"' => {
                i = skip_string_backward(chars, i, '"');
            }
            // If we hit `{` or `[` without a matching `}` or `]`, we've
            // left the expression context — stop searching.
            '{' | '[' => return None,
            // If we hit `;` we've gone past a statement boundary.
            ';' => return None,
            _ => {}
        }
    }

    None
}

/// Skip backward past a string literal ending at position `end` (which
/// points to the closing quote character `q`).
///
/// Returns the position of the opening quote, or 0 if not found.
pub fn skip_string_backward(chars: &[char], end: usize, q: char) -> usize {
    if end == 0 {
        return 0;
    }
    let mut j = end - 1;
    while j > 0 {
        if chars[j] == q {
            // Check it's not escaped
            let mut backslashes = 0u32;
            let mut k = j;
            while k > 0 && chars[k - 1] == '\\' {
                backslashes += 1;
                k -= 1;
            }
            if backslashes.is_multiple_of(2) {
                // Not escaped — this is the opening quote
                return j;
            }
        }
        j -= 1;
    }
    0
}

/// Extract the call expression that precedes the opening paren at `open`.
///
/// Handles:
/// - `foo(` → `"foo"`
/// - `$this->method(` → `"$this->method"`
/// - `$var->method(` → `"$var->method"`
/// - `ClassName::method(` → `"ClassName::method"`
/// - `self::method(` / `static::method(` / `parent::method(` → as-is
/// - `new ClassName(` → `"new ClassName"`
/// - `(new Foo())->method(` → `"$this->method"` etc. — simplified
pub fn extract_call_expression(chars: &[char], open: usize) -> Option<String> {
    if open == 0 {
        return None;
    }

    let mut i = open;

    // Skip whitespace before `(`
    while i > 0 && chars[i - 1] == ' ' {
        i -= 1;
    }

    if i == 0 {
        return None;
    }

    // ── If preceded by `)`, this is a chained call like `foo()->bar(`.
    // We won't try to resolve through call chains for named args — the
    // complexity is high and the user can rely on member completion.
    // But we DO need to handle `(new Foo)(` — skip for now.
    if chars[i - 1] == ')' {
        return None;
    }

    // ── Read the identifier (function/method name) ──────────────────
    let ident_end = i;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '\\') {
        i -= 1;
    }
    if i == ident_end {
        return None;
    }
    let ident: String = chars[i..ident_end].iter().collect();

    // ── Check what precedes the identifier ──────────────────────────

    // Instance method: `->method(`
    if i >= 2 && chars[i - 2] == '-' && chars[i - 1] == '>' {
        let subject = extract_subject_before_arrow(chars, i - 2);
        if !subject.is_empty() {
            return Some(format!("{}->{}", subject, ident));
        }
        return None;
    }

    // Null-safe method: `?->method(`
    if i >= 3 && chars[i - 3] == '?' && chars[i - 2] == '-' && chars[i - 1] == '>' {
        let subject = extract_subject_before_arrow(chars, i - 3);
        if !subject.is_empty() {
            return Some(format!("{}->{}", subject, ident));
        }
        return None;
    }

    // Static method: `::method(`
    if i >= 2 && chars[i - 2] == ':' && chars[i - 1] == ':' {
        let class_name = extract_class_name_backward(chars, i - 2);
        if !class_name.is_empty() {
            return Some(format!("{}::{}", class_name, ident));
        }
        return None;
    }

    // Constructor: `new ClassName(`
    // Skip whitespace and check for `new` keyword.
    let mut j = i;
    while j > 0 && chars[j - 1] == ' ' {
        j -= 1;
    }
    if j >= 3 && chars[j - 3] == 'n' && chars[j - 2] == 'e' && chars[j - 1] == 'w' {
        // Verify word boundary before `new`
        let before_ok = j == 3 || { !chars[j - 4].is_alphanumeric() && chars[j - 4] != '_' };
        if before_ok {
            return Some(format!("new {}", ident));
        }
    }

    // Standalone function call: `foo(`
    Some(ident)
}

/// Extract the subject before `->` for method calls.
///
/// `arrow_pos` points to the `-` of `->`.
/// Handles `$this`, `$var`, and simple variable names.
pub fn extract_subject_before_arrow(chars: &[char], arrow_pos: usize) -> String {
    let mut i = arrow_pos;
    // Skip whitespace
    while i > 0 && chars[i - 1] == ' ' {
        i -= 1;
    }

    // Check for `)` — chained call, skip for now
    if i > 0 && chars[i - 1] == ')' {
        return String::new();
    }

    // Read identifier (property or variable name without `$`)
    let end = i;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }

    // Check for `$` prefix (variable)
    if i > 0 && chars[i - 1] == '$' {
        i -= 1;
        return chars[i..end].iter().collect();
    }

    // Could be a chained property: `$this->prop->method(` — just return
    // the identifier; resolution in server.rs will handle it.
    chars[i..end].iter().collect()
}

/// Extract a class name (possibly namespace-qualified) before `::`.
///
/// `colon_pos` points to the first `:` of `::`.
pub fn extract_class_name_backward(chars: &[char], colon_pos: usize) -> String {
    let mut i = colon_pos;
    // Skip whitespace
    while i > 0 && chars[i - 1] == ' ' {
        i -= 1;
    }
    let end = i;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '\\') {
        i -= 1;
    }
    chars[i..end].iter().collect()
}

/// Parse the arguments text between `(` and the cursor to determine:
/// - Which parameter names have already been used as named arguments
/// - How many positional (non-named) arguments precede the cursor
///
/// Returns `(existing_named_args, positional_count)`.
pub fn parse_existing_args(args_text: &str) -> (Vec<String>, usize) {
    let mut named = Vec::new();
    let mut positional = 0usize;

    // Split by commas at the top level (respecting nested parens/strings)
    let args = split_args_top_level(args_text);

    for arg in &args {
        let trimmed = arg.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Check if this argument is a named argument: `name: value`
        // Named args look like `identifier:` (but NOT `::`)
        if let Some(name) = extract_named_arg_name(trimmed) {
            named.push(name);
        } else {
            positional += 1;
        }
    }

    (named, positional)
}

/// Split argument text by commas at the top level (depth 0), respecting
/// nested parentheses and string literals.
pub fn split_args_top_level(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '(' | '[' => {
                depth += 1;
                current.push(chars[i]);
            }
            ')' | ']' => {
                depth -= 1;
                current.push(chars[i]);
            }
            ',' if depth == 0 => {
                args.push(std::mem::take(&mut current));
            }
            '\'' | '"' => {
                let q = chars[i];
                current.push(q);
                i += 1;
                while i < chars.len() {
                    current.push(chars[i]);
                    if chars[i] == q {
                        // Count the backslashes immediately preceding this
                        // quote (skipping the quote we just pushed). An even
                        // count means the quote is not escaped and closes the
                        // string. Counting by `char` keeps this correct on
                        // lines with multibyte characters.
                        let backslashes = current
                            .chars()
                            .rev()
                            .skip(1)
                            .take_while(|&c| c == '\\')
                            .count();
                        if backslashes.is_multiple_of(2) {
                            break;
                        }
                    }
                    i += 1;
                }
            }
            _ => current.push(chars[i]),
        }
        i += 1;
    }

    // Don't push the last segment — it's the argument currently being typed
    // and is handled separately as the prefix.
    // Actually, we DO want to push it if it has content, because parse_existing_args
    // needs to count it. But the caller already stripped the prefix from args_text,
    // so the last segment here (if any) is a complete previous argument.
    if !current.trim().is_empty() {
        args.push(current);
    }

    args
}

/// If `arg` looks like a named argument (`name: ...`), return the name.
/// Returns `None` for positional arguments.
pub fn extract_named_arg_name(arg: &str) -> Option<String> {
    // Look for `identifier:` at the start (but not `::`)
    let chars: Vec<char> = arg.chars().collect();
    let mut i = 0;

    // Skip whitespace
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }

    // Read identifier
    let start = i;
    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
        i += 1;
    }

    if i == start {
        return None;
    }

    // Must be followed by `:` (but not `::`)
    if i < chars.len() && chars[i] == ':' {
        // Check it's not `::`
        if i + 1 < chars.len() && chars[i + 1] == ':' {
            return None;
        }
        let name: String = chars[start..i].iter().collect();
        return Some(name);
    }

    None
}

// ─── Completion Builder ─────────────────────────────────────────────────────

/// Build named-argument completion items from a list of parameters.
///
/// Parameters that have already been used as named arguments or that are
/// covered by positional arguments are excluded.
pub fn build_named_arg_completions(
    ctx: &NamedArgContext,
    parameters: &[crate::types::ParameterInfo],
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let prefix_lower = ctx.prefix.to_lowercase();

    for (idx, param) in parameters.iter().enumerate() {
        // The parameter name in PHP includes `$`, but named args use the
        // bare name: `$name` → `name:`
        let bare_name = param.name.strip_prefix('$').unwrap_or(&param.name);

        // Skip parameters covered by positional arguments
        if idx < ctx.positional_count {
            continue;
        }

        // Skip parameters already specified as named arguments
        if ctx.existing_named_args.iter().any(|n| n == bare_name) {
            continue;
        }

        // Apply prefix filter
        if !bare_name.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }

        // Build the label showing type info
        let label = if let Some(ref th) = param.type_hint {
            format!("{}: {}", bare_name, th)
        } else {
            format!("{}:", bare_name)
        };

        // Insert text: `name: ` (bare name + colon + space)
        let insert = format!("{}: ", bare_name);

        let detail = if param.is_variadic {
            Some("Named argument (variadic)".to_string())
        } else if !param.is_required {
            Some("Named argument (optional)".to_string())
        } else {
            Some("Named argument".to_string())
        };

        items.push(CompletionItem {
            label,
            kind: Some(CompletionItemKind::VARIABLE),
            detail,
            insert_text: Some(insert),
            filter_text: Some(bare_name.to_string()),
            sort_text: Some(format!("0_{:03}", idx)),
            ..CompletionItem::default()
        });
    }

    items
}
