use super::CapturedDirective;
use super::shared::{LineOut, closes_args};

/// Capture an `@use(...)` / `@inject(...)` argument list until its parens
/// balance and emit the real PHP construct it becomes, reporting whether
/// the closing paren was reached.
pub(super) fn consume(
    kind: CapturedDirective,
    ch: char,
    paren_depth: &mut i32,
    capture_buffer: &mut String,
    hoisted_uses: &mut Vec<String>,
    mut out: LineOut<'_>,
) -> bool {
    // Capture the argument text (in `buffer`, via the fall-through
    // push the caller does) until the parens balance, then transform it.
    if closes_args(ch, paren_depth) {
        *out.char_idx += 1;
        *out.current_utf16_col += 1;
        // `capture_buffer` holds any prior lines of this
        // argument list; `buffer` holds the current line's
        // text from the opening `(` (or line start) up to
        // (but not including) this closing `)`. Together
        // they are the argument text from the opening `(`
        // to the closing `)`.
        let mut raw = std::mem::take(capture_buffer);
        raw.push_str(out.buffer);
        out.buffer.clear();
        let emitted = match kind {
            CapturedDirective::Use => {
                if let Some(stmt) = build_use_statement(&raw) {
                    hoisted_uses.push(stmt);
                }
                // The import is hoisted; nothing inline.
                String::new()
            }
            CapturedDirective::Inject => build_inject_statement(&raw),
        };

        out.emit_suffix(&emitted);

        return true;
    }
    false
}

/// Trim surrounding whitespace and quote characters, matching Blade's
/// compiler (`trim($x, " '\"")`).
fn trim_quotes_and_space(s: &str) -> &str {
    s.trim_matches(|c: char| c == ' ' || c == '\'' || c == '"')
}

/// Whether `s` is a valid PHP identifier (variable name without the `$`).
fn is_php_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Translate the captured argument text of an `@use(...)` directive into a
/// real top-level `use` statement, mirroring Blade's `compileUse`. `raw` is
/// everything from the opening `(` up to (not including) the closing `)`.
///
/// Handles the plain form (`'App\Models\Post'`), the inline alias
/// (`'App\Models\Post as Article'`), the two-argument alias
/// (`'App\Models\Post', 'Article'`), grouped imports
/// (`'App\Models\{Post, Comment}'`), and the `function`/`const` modifiers.
/// Returns `None` when no importable path can be parsed.
fn build_use_statement(raw: &str) -> Option<String> {
    // Blade strips all parens, then trims whitespace/quotes.
    let expression: String = raw.chars().filter(|c| *c != '(' && *c != ')').collect();
    let expression = trim_quotes_and_space(&expression);

    let (path_with_modifier, alias) = if expression.contains('{') {
        // Grouped import: the braces are the argument, no alias.
        (expression.to_string(), String::new())
    } else {
        let mut segments = expression.splitn(2, ',');
        let path = trim_quotes_and_space(segments.next().unwrap_or("")).to_string();
        let alias = match segments.next() {
            Some(a) => format!(" as {}", trim_quotes_and_space(a)),
            None => String::new(),
        };
        (path, alias)
    };

    // Split off a `function ` / `const ` modifier if present.
    let (modifier, path) = if let Some(rest) = path_with_modifier.strip_prefix("function ") {
        ("function ", rest)
    } else if let Some(rest) = path_with_modifier.strip_prefix("const ") {
        ("const ", rest)
    } else {
        ("", path_with_modifier.as_str())
    };
    let path = path.trim().trim_start_matches('\\');

    if path.is_empty() {
        return None;
    }

    Some(format!("use {modifier}{path}{alias};"))
}

/// Translate the captured argument text of an `@inject(...)` directive into
/// an inline `$var = app(service);` assignment, mirroring Blade's
/// `compileInject`. `raw` is everything from the opening `(` up to (not
/// including) the closing `)`. Returns an empty string when the argument
/// list has no valid variable name or service.
fn build_inject_statement(raw: &str) -> String {
    let stripped: String = raw.chars().filter(|c| *c != '(' && *c != ')').collect();
    let mut segments = stripped.splitn(2, ',');
    let variable = trim_quotes_and_space(segments.next().unwrap_or(""));
    // The service keeps its own quotes; only surrounding whitespace is trimmed.
    let service = segments.next().unwrap_or("").trim();

    if variable.is_empty() || !is_php_identifier(variable) || service.is_empty() {
        return String::new();
    }

    format!(" ${variable} = app({service}); ")
}
