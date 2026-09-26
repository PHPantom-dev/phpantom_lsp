//! PHPStan `assertType()` fixture runner.
//!
//! This harness processes PHP files from PHPStan's `nsrt/` test corpus.
//! Each file contains calls like `assertType('expected_type', $expr)`.
//! The runner:
//!
//! 1. Parses the PHP source to find every `assertType()` call.
//! 2. Transforms each call into `$__phpantom_assert_N = expr;` so that
//!    the expression result is assigned to a variable we can hover on.
//! 3. Opens the transformed source in a test backend.
//! 4. Hovers on each `$__phpantom_assert_N` variable to resolve its type.
//! 5. Compares the hover type against the expected type string.
//!
//! Files are placed in `tests/phpstan_nsrt/` and picked up automatically
//! by `datatest_stable`. Lines containing `assertNativeType` are ignored
//! (PHPantom does not track native vs PHPDoc types separately).
//!
//! To skip an assertion that PHPantom cannot yet handle, add `// SKIP`
//! on the same line as the `assertType()` call.
//!
//! Expected types are matched exactly, modulo the cosmetic spellings
//! `normalize_type` canonicalizes. In particular a scalar literal is not
//! interchangeable with its base type: an assertion expecting `string`
//! fails against `'foo'`. Where PHPantom resolves a value more precisely
//! than the upstream corpus does, record the precise type and say so in a
//! comment; where it is *less* precise, keep the upstream expectation and
//! mark the line `// SKIP` so the gap stays visible instead of being
//! absorbed by a lenient comparison.

use std::collections::HashMap;
use std::path::Path;

use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

// ─── Assertion extraction ───────────────────────────────────────────────────

/// A single `assertType('expected', expr)` call found in the source.
#[derive(Debug)]
struct AssertTypeCall {
    /// The expected type string from the first argument.
    expected: String,
    /// The raw expression text from the second argument.
    expr: String,
    /// 1-based line number in the original source.
    original_line: usize,
    /// Number of source lines the call spans (1 unless it is multi-line).
    line_count: usize,
    /// Byte range of the call within its line, from the function name to
    /// the closing `)`, for a call on a single line.  The call is replaced
    /// in place so the code around it keeps running: an assertion inside
    /// `(fn () => assertType(…, $this))->call($foo)` must still see the
    /// `$this` that `call()` binds.
    in_line: Option<(usize, usize)>,
}

/// Extract all `assertType()` calls from the PHP source.
///
/// This uses a simple text-based parser rather than a full AST walk,
/// since the PHPStan test files follow a very consistent format:
/// `assertType('expected', expr);`
///
/// Handles:
/// - Single-quoted and double-quoted expected strings
/// - `Foo::class` as expected type (resolved to the class name)
/// - Nested parentheses in the expression argument
/// - Multi-line assertType calls (rare but possible)
fn extract_assert_type_calls(source: &str) -> Vec<AssertTypeCall> {
    let mut results = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Skip lines with SKIP annotation.
        if trimmed.contains("// SKIP") || trimmed.contains("/* SKIP */") {
            i += 1;
            continue;
        }

        // Skip commented-out lines (the assertType call is not active code).
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
            i += 1;
            continue;
        }

        // Skip assertNativeType calls.
        if trimmed.contains("assertNativeType") && !trimmed.contains("assertType") {
            i += 1;
            continue;
        }

        // Look for assertType( calls.
        if let Some(call_start) = find_assert_type_start(trimmed) {
            let after_paren = &trimmed[call_start..];

            // Collect the full call text, potentially spanning multiple lines.
            let (call_text, lines_consumed) = collect_call_text(after_paren, &lines, i);

            if let Some(mut parsed) = parse_assert_type_call(&call_text, source, &lines, i) {
                parsed.line_count = lines_consumed;
                if lines_consumed == 1
                    && let Some(close) = matching_close_paren(after_paren)
                {
                    let indent = line.len() - line.trim_start().len();
                    let name_start = trimmed[..call_start]
                        .rfind("assertType(")
                        .expect("the call starts with its name");
                    parsed.in_line = Some((indent + name_start, indent + call_start + close + 1));
                }
                results.push(parsed);
            }

            i += lines_consumed;
        } else {
            i += 1;
        }
    }

    results
}

/// Find the start index of `assertType(` in a trimmed line.
/// Returns the index right after the opening parenthesis.
fn find_assert_type_start(line: &str) -> Option<usize> {
    // Match both `assertType(` and `\PHPStan\Testing\assertType(`
    let patterns = ["assertType(", "\\PHPStan\\Testing\\assertType("];
    for pat in &patterns {
        if let Some(pos) = line.find(pat) {
            // Make sure it's not `assertNativeType`
            if pos > 0 && line[..pos].ends_with("Native") {
                continue;
            }
            return Some(pos + pat.len());
        }
    }
    None
}

/// Collect the full call text from `(` to matching `)`, potentially
/// spanning multiple source lines. Returns (call_text, lines_consumed).
fn collect_call_text(after_open_paren: &str, lines: &[&str], start_line: usize) -> (String, usize) {
    let mut text = after_open_paren.to_string();
    let mut depth: i32 = 1; // We're already past the opening `(`
    let mut consumed = 1;

    // Check if the call is complete on this line.
    for ch in after_open_paren.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return (text, consumed);
                }
            }
            _ => {}
        }
    }

    // Multi-line: keep collecting.
    let mut line_idx = start_line + 1;
    while line_idx < lines.len() && depth > 0 {
        text.push('\n');
        text.push_str(lines[line_idx].trim());
        consumed += 1;

        for ch in lines[line_idx].chars() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return (text, consumed);
                    }
                }
                _ => {}
            }
        }

        line_idx += 1;
    }

    (text, consumed)
}

/// Parse the arguments of an `assertType(expected, expr)` call.
/// `call_text` starts right after `assertType(`.
fn parse_assert_type_call(
    call_text: &str,
    _source: &str,
    _lines: &[&str],
    line_idx: usize,
) -> Option<AssertTypeCall> {
    let text = call_text.trim();

    // Parse the first argument (expected type).
    let (expected, rest) = parse_first_argument(text)?;

    // The rest should start with `,` after optional whitespace.
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(',')?;
    let rest = rest.trim_start();

    // Parse the second argument (expression) — everything up to the
    // closing `)` at depth 0.
    let expr = parse_second_argument(rest)?;

    Some(AssertTypeCall {
        expected,
        expr,
        original_line: line_idx + 1,
        line_count: 1,
        in_line: None,
    })
}

/// Byte offset of the `)` closing a call whose arguments start `text`,
/// skipping parentheses inside string literals.
fn matching_close_paren(text: &str) -> Option<usize> {
    let mut depth = 1;
    let mut string_char = None;
    let mut escaped = false;
    for (i, ch) in text.char_indices() {
        if let Some(quote) = string_char {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                string_char = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => string_char = Some(ch),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse the first argument of assertType, which is either:
/// - A string literal: `'int'` or `"int"`
/// - A class constant: `Foo::class`
///
/// Returns (expected_type, remaining_text).
fn parse_first_argument(text: &str) -> Option<(String, &str)> {
    if text.starts_with('\'') || text.starts_with('"') {
        let quote = text.as_bytes()[0] as char;
        let rest = &text[1..];
        // Find closing quote, handling escaped quotes.
        let mut i = 0;
        let bytes = rest.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2; // Skip escaped character.
                continue;
            }
            if bytes[i] == quote as u8 {
                let value = &rest[..i];
                // Unescape the string.
                let unescaped = value.replace("\\'", "'").replace("\\\"", "\"");
                return Some((unescaped, &rest[i + 1..]));
            }
            i += 1;
        }
        None
    } else {
        // Look for `SomeClass::class` pattern.
        let class_suffix = "::class";
        if let Some(pos) = text.find(class_suffix) {
            let class_name = text[..pos].trim();
            let rest = &text[pos + class_suffix.len()..];
            return Some((class_name.to_string(), rest));
        }
        None
    }
}

/// Parse the second argument — the expression to type-check.
/// Handles nested parentheses and stops at the closing `)` at depth 0.
fn parse_second_argument(text: &str) -> Option<String> {
    let mut depth: i32 = 0;
    let mut end = 0;
    let mut in_string = false;
    let mut string_char: char = '\'';
    let bytes = text.as_bytes();

    while end < bytes.len() {
        let ch = bytes[end] as char;

        if in_string {
            if ch == '\\' {
                end += 2;
                continue;
            }
            if ch == string_char {
                in_string = false;
            }
            end += 1;
            continue;
        }

        match ch {
            '\'' | '"' => {
                in_string = true;
                string_char = ch;
            }
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    let expr = text[..end].trim();
                    if expr.is_empty() {
                        return None;
                    }
                    return Some(expr.to_string());
                }
                depth -= 1;
            }
            _ => {}
        }
        end += 1;
    }

    None
}

// ─── Source transformation ──────────────────────────────────────────────────

/// Transform the PHP source by replacing each `assertType(...)` call
/// with `$__phpantom_assert_N = expr;` so we can hover on the variable.
///
/// Returns the transformed source and a list of (var_name, expected_type,
/// line_in_transformed, original_line) tuples.
fn transform_source(
    source: &str,
    assertions: &[AssertTypeCall],
) -> (String, Vec<(String, String, u32, usize)>) {
    if assertions.is_empty() {
        return (source.to_string(), Vec::new());
    }

    // Build a map from original 1-based line number to assertion index.
    let mut line_to_assertions: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, a) in assertions.iter().enumerate() {
        line_to_assertions
            .entry(a.original_line)
            .or_default()
            .push(i);
    }

    let mut result = String::with_capacity(source.len());
    let mut assertion_locations: Vec<(String, String, u32, usize)> = Vec::new();
    let mut output_line: u32 = 0; // 0-based line counter in output
    // A multi-line call is replaced as a whole on its first line, so the
    // original lines it continues onto must not be copied through.
    let mut skip_until = 0;

    for (line_idx, line) in source.lines().enumerate() {
        let original_line_1based = line_idx + 1;

        if original_line_1based <= skip_until {
            continue;
        }

        if let Some(indices) = line_to_assertions.get(&original_line_1based) {
            for &idx in indices {
                let a = &assertions[idx];
                let var_name = format!("$__phpantom_assert_{}", idx);
                let replacement = match a.in_line {
                    Some((start, end)) => {
                        // The name may be qualified (`\PHPStan\Testing\assertType`).
                        let before = line[..start]
                            .trim_end_matches(|c: char| c.is_alphanumeric() || "_\\".contains(c));
                        format!("{before}{var_name} = {}{}", a.expr, &line[end..])
                    }
                    None => {
                        // Preserve indentation from the original line.
                        let indent = &line[..line.len() - line.trim_start().len()];
                        format!("{indent}{var_name} = {};", a.expr)
                    }
                };
                result.push_str(&replacement);
                result.push('\n');

                assertion_locations.push((
                    var_name,
                    a.expected.clone(),
                    output_line,
                    a.original_line,
                ));
                output_line += replacement.lines().count() as u32;
                skip_until = skip_until.max(a.original_line + a.line_count - 1);
            }
        } else {
            result.push_str(line);
            result.push('\n');
            output_line += 1;
        }
    }

    (result, assertion_locations)
}

// ─── Type comparison ────────────────────────────────────────────────────────

/// Rewrite double-quoted string literals to single quotes.
///
/// PHPStan and Psalm always render a string literal with single quotes,
/// while PHPantom keeps the literal's source spelling, so `$a = "hello"`
/// resolves to `"hello"` and `$b = 'hello'` to `'hello'`. Only the quote
/// style differs, so canonicalize it on both sides and let upstream
/// expectations port verbatim.
///
/// Literals containing a backslash or a single quote are left untouched:
/// PHP's escape rules differ between the two quote styles, so rewriting
/// those would change which value the literal denotes.
fn canonicalize_literal_quotes(ty: &str) -> String {
    let mut result = String::with_capacity(ty.len());
    let mut rest = ty;

    while let Some(open) = rest.find('"') {
        result.push_str(&rest[..open]);
        let after = &rest[open + 1..];

        let Some(close) = after.find('"') else {
            result.push_str(&rest[open..]);
            return result;
        };

        let content = &after[..close];
        let quote = if content.contains('\\') || content.contains('\'') {
            '"'
        } else {
            '\''
        };
        result.push(quote);
        result.push_str(content);
        result.push(quote);

        rest = &after[close + 1..];
    }

    result.push_str(rest);
    result
}

/// Normalize a type string for comparison.
///
/// PHPStan and PHPantom may format the same type differently. This
/// function canonicalizes both sides so that cosmetic differences
/// (spacing, leading backslash, FQN vs short name, `?T` vs `T|null`,
/// literal quote style) don't cause spurious failures.
fn normalize_type(ty: &str) -> String {
    let mut s = canonicalize_literal_quotes(ty.trim());

    // Strip leading backslash from FQN types.
    if s.starts_with('\\') {
        s = s[1..].to_string();
    }

    // Normalize `?T` to `T|null`.
    if s.starts_with('?') && !s.contains('|') {
        s = format!("{}|null", &s[1..]);
    }

    // Normalize whitespace around `|` and `&`.
    s = s.replace(" | ", "|").replace(" & ", "&");

    // Normalize callable return type syntax: `Closure(int): bool` → `Closure(int):bool`.
    s = s.replace("): ", "):");

    // Normalize shape syntax: `object{key: type}` → `object{key:type}`.
    // Remove spaces after colons inside curly braces.
    {
        let mut normalized = String::with_capacity(s.len());
        let mut brace_depth = 0i32;
        let mut cs = s.chars().peekable();
        while let Some(ch) = cs.next() {
            match ch {
                '{' => {
                    brace_depth += 1;
                    normalized.push(ch);
                }
                '}' => {
                    brace_depth -= 1;
                    normalized.push(ch);
                }
                ':' if brace_depth > 0 => {
                    normalized.push(':');
                    // Skip whitespace after colon inside shapes.
                    while cs.peek() == Some(&' ') {
                        cs.next();
                    }
                }
                ',' if brace_depth > 0 => {
                    normalized.push(',');
                    // Skip whitespace after comma inside shapes.
                    while cs.peek() == Some(&' ') {
                        cs.next();
                    }
                }
                _ => normalized.push(ch),
            }
        }
        s = normalized;
    }

    // Normalize `array<int, string>` vs `array<int,string>` etc.
    // Remove spaces after commas inside angle brackets.
    let mut result = String::with_capacity(s.len());
    let mut angle_depth = 0i32;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '<' => {
                angle_depth += 1;
                result.push(ch);
            }
            '>' => {
                angle_depth -= 1;
                result.push(ch);
            }
            ',' if angle_depth > 0 => {
                result.push(',');
                // Skip whitespace after comma inside generics.
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                // Add exactly one space for readability.
                result.push(' ');
            }
            _ => result.push(ch),
        }
    }

    canonicalize_union_spelling(&drop_sequential_shape_keys(&strip_template_scopes(&result)))
}

/// Remove the scope PHPStan appends to a template type's name
/// (`T (method Foo::bar(), argument)`, `T of array (class Foo, parameter)`).
fn strip_template_scopes(ty: &str) -> String {
    let mut out = String::with_capacity(ty.len());
    let mut rest = ty;
    while let Some(pos) = [" (class ", " (method ", " (function "]
        .iter()
        .filter_map(|marker| rest.find(marker))
        .min()
    {
        out.push_str(&rest[..pos]);
        // The scope can itself hold parentheses (`method Foo::bar()`), so
        // find the `)` that closes the one opened at `pos + 1`.
        let mut depth = 0i32;
        let close = rest[pos + 1..].char_indices().find_map(|(i, ch)| {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            None
        });
        match close {
            Some(close) => rest = &rest[pos + 1 + close + 1..],
            None => {
                rest = &rest[pos..];
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Split `ty` at the `|` separators that are not nested inside brackets,
/// braces, parentheses, or quotes.
fn split_top_level_union(ty: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (i, ch) in ty.char_indices() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '{' | '<' | '(' | '[' => depth += 1,
            '}' | '>' | ')' | ']' => depth -= 1,
            '|' if depth == 0 => {
                parts.push(&ty[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&ty[start..]);
    parts
}

/// Canonicalize spellings of the same type that PHPStan and PHPantom
/// print differently: `callable(): mixed` is `callable`, `array-key` is
/// `int|string`, `mixed[]` and
/// `array<int|string, V>` are `array` and `array<V>`, `(A|B)` is `A|B`,
/// a union with `mixed` in it is `mixed`, and `array{}` beside a wider
/// `array` member adds nothing.
fn canonicalize_union_spelling(ty: &str) -> String {
    let mut ty = ty.trim();
    while ty.starts_with('(')
        && ty.ends_with(')')
        && split_top_level_union(&ty[1..ty.len() - 1]).len() > 1
        && ty[1..ty.len() - 1].chars().filter(|&c| c == '(').count()
            == ty[1..ty.len() - 1].chars().filter(|&c| c == ')').count()
    {
        ty = &ty[1..ty.len() - 1];
    }
    let canonical_member = |member: &str| -> String {
        let member = member.trim();
        match member {
            "callable():mixed" => "callable".to_string(),
            "Closure():mixed" => "Closure".to_string(),
            "array-key" => "int|string".to_string(),
            "mixed[]" | "array<mixed>" | "array<mixed, mixed>" => "array".to_string(),
            _ => {
                for prefix in ["array<int|string, ", "array<array-key, "] {
                    if let Some(rest) = member.strip_prefix(prefix) {
                        return format!("array<{rest}");
                    }
                }
                member.to_string()
            }
        }
    };
    let members: Vec<String> = split_top_level_union(ty)
        .into_iter()
        .map(canonical_member)
        .collect();
    if members.len() > 1 && members.iter().any(|m| m == "mixed") {
        return "mixed".to_string();
    }
    let has_wider_array = members
        .iter()
        .any(|m| m == "array" || m.starts_with("array<"));
    let kept: Vec<String> = members
        .into_iter()
        .filter(|m| !(has_wider_array && m == "array{}"))
        .collect();
    kept.join("|")
}

/// Rewrite every `array{0:A,1:B}` whose keys run 0, 1, 2, … with none
/// optional to the positional `array{A,B}` spelling PHPStan prints.
///
/// Expects the output of the whitespace pass in [`normalize_type`], where
/// shape entries are separated by a bare `,` and keyed as `key:value`.
fn drop_sequential_shape_keys(ty: &str) -> String {
    const OPEN: &str = "array{";
    let Some(start) = ty.find(OPEN) else {
        return ty.to_string();
    };
    let body_start = start + OPEN.len();
    // Find the matching `}` and split the body at its top-level commas.
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut entries: Vec<&str> = Vec::new();
    let mut entry_start = body_start;
    let mut body_end = None;
    for (i, ch) in ty[body_start..].char_indices() {
        let i = body_start + i;
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '{' | '<' | '(' | '[' => depth += 1,
            '}' if depth == 0 => {
                entries.push(&ty[entry_start..i]);
                body_end = Some(i);
                break;
            }
            '}' | '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                entries.push(&ty[entry_start..i]);
                entry_start = i + 1;
            }
            _ => {}
        }
    }
    let Some(body_end) = body_end else {
        return ty.to_string();
    };

    let positional: Option<Vec<&str>> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let (key, value) = entry.split_once(':')?;
            (key == index.to_string()).then_some(value)
        })
        .collect();
    let body = match positional {
        Some(values) if !values.is_empty() => values.join(","),
        _ => ty[body_start..body_end].to_string(),
    };
    format!(
        "{}{}{}}}{}",
        &ty[..start],
        OPEN,
        drop_sequential_shape_keys(&body),
        drop_sequential_shape_keys(&ty[body_end + 1..])
    )
}

/// Compare expected (PHPStan) type with actual (PHPantom hover) type.
/// Returns true if they match after normalization.
fn types_match(expected: &str, actual: &str) -> bool {
    let ne = normalize_type(expected);
    let na = normalize_type(actual);

    if ne == na {
        return true;
    }

    // `*ERROR*` is PHPStan's type for an expression it cannot resolve;
    // PHPantom's counterpart is `mixed`.
    if ne == "*ERROR*" && na == "mixed" {
        return true;
    }

    // `*NEVER*` is how PHPStan prints the bottom type.
    if ne == "*NEVER*" && na == "never" {
        return true;
    }

    // Generator<K, V> is semantically equivalent to Generator<K, V, mixed, mixed>.
    // Normalize both sides to compare without trailing `mixed` params.
    let ne_gen = normalize_generator_params(&ne);
    let na_gen = normalize_generator_params(&na);
    if ne_gen == na_gen {
        return true;
    }

    // PHPStan uses FQN in expected types but PHPantom may use short names.
    // Try matching against just the short name of each component.
    let ne_short = shorten_fqn_components(&ne);
    let na_short = shorten_fqn_components(&na);

    if ne_short == na_short {
        return true;
    }

    // Union and intersection member order may differ: sort members and
    // compare.
    let sorted_members = |ty: &str| {
        ty.split('|')
            .map(|part| {
                let mut members: Vec<&str> = part.split('&').collect();
                members.sort();
                members.join("&")
            })
            .collect::<Vec<_>>()
    };
    let ne_members = sorted_members(&ne_short);
    let na_members = sorted_members(&na_short);
    let mut ne_parts: Vec<&str> = ne_members.iter().map(String::as_str).collect();
    let mut na_parts: Vec<&str> = na_members.iter().map(String::as_str).collect();
    ne_parts.sort();
    na_parts.sort();
    if ne_parts == na_parts {
        return true;
    }

    false
}

/// Shorten FQN components in a type string.
/// `App\Models\User|null` → `User|null`
/// `static(App\Models\User)` → `static(User)`
fn shorten_fqn_components(ty: &str) -> String {
    let mut result = String::new();
    let mut word = String::new();
    for ch in ty.chars() {
        if ch == '\\' {
            word.clear();
        } else if ch.is_alphanumeric() || ch == '_' || ch == '$' {
            word.push(ch);
        } else {
            result.push_str(&word);
            word.clear();
            result.push(ch);
        }
    }
    result.push_str(&word);
    result
}

/// Strip trailing `, mixed` params from Generator types so that
/// `Generator<int, stdClass>` matches `Generator<int, stdClass, mixed, mixed>`.
fn normalize_generator_params(ty: &str) -> String {
    // Simple regex-free approach: find `Generator<...>` and strip trailing `, mixed` entries.
    let mut result = ty.to_string();
    while let Some(start) = result.find("Generator<") {
        let gen_start = start + "Generator<".len();
        // Find matching `>`.
        let mut depth = 1i32;
        let mut end = gen_start;
        for (i, ch) in result[gen_start..].char_indices() {
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = gen_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let inner = &result[gen_start..end];
        let trimmed = inner.trim_end_matches(", mixed").trim_end_matches(",mixed");
        if trimmed != inner {
            let new = format!("Generator<{}>", trimmed);
            result = format!("{}{}{}", &result[..start], new, &result[end + 1..]);
        } else {
            break;
        }
    }
    result
}

// ─── Hover helpers ──────────────────────────────────────────────────────────

/// Extract plain text from a Hover response.
fn extract_hover_text(hover: &Hover) -> String {
    match &hover.contents {
        HoverContents::Markup(mc) => mc.value.clone(),
        HoverContents::Scalar(MarkedString::String(s)) => s.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => ls.value.clone(),
        HoverContents::Array(items) => items
            .iter()
            .map(|ms| match ms {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(ls) => ls.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Extract the type string from a variable hover result.
///
/// PHPantom hover for variables outputs something like:
/// ```text
/// ```php
/// <?php
/// $varname = TypeHere
/// ```
/// ```
///
/// This function extracts `TypeHere`.
fn extract_type_from_hover(hover_text: &str, var_name: &str) -> Option<String> {
    // Look for `$varname = Type` pattern.
    // When a union type has multiple class-like members, the hover
    // renders each member as a separate code block:
    //   ```php
    //   $var = Foo
    //   ```
    //   ---
    //   ```php
    //   $var = Bar
    //   ```
    // Collect all such occurrences and join them with `|`.
    let pattern = format!("{} = ", var_name);
    let mut found_types: Vec<String> = Vec::new();

    for line in hover_text.lines() {
        let trimmed = line.trim();
        if let Some(pos) = trimmed.find(&pattern) {
            let after = &trimmed[pos + pattern.len()..];
            let ty = after.trim();
            if !ty.is_empty() && !found_types.contains(&ty.to_string()) {
                found_types.push(ty.to_string());
            }
        }
    }

    if !found_types.is_empty() {
        return Some(found_types.join("|"));
    }

    // Fallback: look for any `= Type` after the var name in the whole text.
    if let Some(pos) = hover_text.find(&pattern) {
        let after = &hover_text[pos + pattern.len()..];
        // Take until newline or end of code block.
        let ty = after.lines().next().unwrap_or("").trim();
        if !ty.is_empty() {
            return Some(ty.to_string());
        }
    }

    None
}

// ─── Test runner ────────────────────────────────────────────────────────────

fn create_assert_type_backend() -> Backend {
    Backend::new_test_with_full_stubs()
}

fn fixture_uri(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .expect("assert-type runner should have a current directory")
            .join(path)
    };

    Url::from_file_path(&absolute)
        .expect("assert-type fixture path should convert to a file URI")
        .to_string()
}

fn run_assert_type(path: &Path, content: String) -> datatest_stable::Result<()> {
    // Parse assertions from original source.
    let assertions = extract_assert_type_calls(&content);

    if assertions.is_empty() {
        eprintln!("WARNING: No assertType() calls found in {}", path.display());
        return Ok(());
    }

    // Transform the source: replace assertType() calls with variable assignments.
    let (transformed, locations) = transform_source(&content, &assertions);

    // Create backend and open the file.
    let backend = create_assert_type_backend();
    let uri = fixture_uri(path);
    backend.update_ast(&uri, &transformed);

    let mut failures: Vec<String> = Vec::new();
    let mut passed = 0;
    let mut skipped = 0;

    for (var_name, expected, line, original_line) in &locations {
        // Find the column of the variable in the transformed source.
        let transformed_lines: Vec<&str> = transformed.lines().collect();
        let target_line = *line as usize;

        if target_line >= transformed_lines.len() {
            failures.push(format!(
                "  Line {} (original {}): transformed line out of range",
                line, original_line
            ));
            continue;
        }

        let line_text = transformed_lines[target_line];
        let col = line_text.find(var_name.as_str()).unwrap_or(0) as u32;

        // Hover on the variable.
        let position = Position {
            line: *line,
            character: col + 1, // +1 to land inside the variable name (past $)
        };

        let hover = backend.handle_hover(&uri, &transformed, position);

        match hover {
            Some(h) => {
                let hover_text = extract_hover_text(&h);
                match extract_type_from_hover(&hover_text, var_name) {
                    Some(actual_type) => {
                        if types_match(expected, &actual_type) {
                            passed += 1;
                        } else {
                            failures.push(format!(
                                "  Line {} (original {}): expected `{}`, got `{}`",
                                line, original_line, expected, actual_type
                            ));
                        }
                    }
                    None => {
                        // Could not extract type from hover — might be unresolved.
                        if expected == "mixed" || expected == "*ERROR*" {
                            // Unresolved hover is acceptable for mixed/error types.
                            passed += 1;
                        } else {
                            failures.push(format!(
                                "  Line {} (original {}): expected `{}`, hover returned no type. Hover text: {}",
                                line, original_line, expected,
                                hover_text.chars().take(200).collect::<String>()
                            ));
                        }
                    }
                }
            }
            None => {
                if expected == "mixed" || expected == "*ERROR*" {
                    passed += 1;
                } else {
                    skipped += 1;
                    // No hover result — expression type could not be resolved.
                    failures.push(format!(
                        "  Line {} (original {}): expected `{}`, no hover result",
                        line, original_line, expected
                    ));
                }
            }
        }
    }

    let total = locations.len();
    eprintln!(
        "{}: {}/{} passed, {} failed, {} skipped",
        path.display(),
        passed,
        total,
        failures.len(),
        skipped
    );

    if !failures.is_empty() {
        let msg = format!(
            "{}: {}/{} assertions failed:\n{}",
            path.display(),
            failures.len(),
            total,
            failures.join("\n")
        );
        return Err(msg.into());
    }

    Ok(())
}

datatest_stable::harness! {
    { test = run_assert_type, root = "tests/phpstan_nsrt", pattern = r"\.php$" },
    { test = run_assert_type, root = "tests/psalm_assertions", pattern = r"\.php$" },
}
