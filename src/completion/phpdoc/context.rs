//! PHPDoc context detection and symbol info extraction.
//!
//! This module determines what PHP construct follows a docblock (function,
//! class, property, constant, or inline code) and extracts structural
//! information from the declaration (parameter names and types, return
//! type, property type hint).
//!
//! The main public entry points are:
//!
//! - [`detect_context`] — classify the PHP symbol after the docblock
//! - [`extract_symbol_info`] — parse parameter/return/property info
//! - [`detect_docblock_typing_position`] — detect type vs variable
//!   cursor position inside a PHPDoc tag
//! - [`extract_phpdoc_prefix`] — extract the `@tag` prefix being typed

use tower_lsp::lsp_types::Position;

use super::helpers::{find_keyword_pos, find_matching_paren, split_params};
use crate::completion::source::comment_position::{is_inside_docblock, position_to_byte_offset};
use crate::php_type::PhpType;

// ─── Context Detection ─────────────────────────────────────────────────────

/// The kind of PHP symbol that follows the docblock containing the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocblockContext {
    /// The docblock precedes a function or method declaration.
    FunctionOrMethod,
    /// The docblock precedes a class, interface, trait, or enum.
    ClassLike,
    /// The docblock precedes a property declaration.
    Property,
    /// The docblock precedes a constant declaration.
    Constant,
    /// The docblock is inline inside code (not before a declaration).
    ///
    /// Only a small set of tags makes sense here: `@var` (type narrowing),
    /// `@throws` (exception hinting), and general tags like `@todo`,
    /// `@see`, `@example`, `@link`.
    Inline,
    /// Context could not be determined (file-level, or symbol not recognized).
    Unknown,
}

/// Information extracted from the PHP symbol following the docblock.
///
/// Used to pre-fill completion items with concrete types and names.
#[derive(Debug, Clone, Default)]
pub struct SymbolInfo {
    /// Parameters: `(optional_type_hint, $name)`.
    pub params: Vec<(Option<PhpType>, String)>,
    /// Return type hint (e.g. `"string"`, `"void"`, `"?int"`).
    pub return_type: Option<PhpType>,
    /// Property / constant type hint.
    pub type_hint: Option<PhpType>,
    /// Function/method name (e.g. `"__construct"`, `"getName"`).
    /// Only populated for `FunctionOrMethod` declarations.
    pub method_name: Option<String>,
    /// Variable name (e.g. `$items`) for inline variable assignments.
    pub variable_name: Option<String>,
    /// Parent class names from `extends` clause (class-like declarations).
    pub extends_names: Vec<String>,
    /// Interface names from `implements` clause (class-like declarations).
    pub implements_names: Vec<String>,
}

// ─── Docblock Typing Position Detection ─────────────────────────────────────

/// What kind of completion the cursor position inside a docblock tag
/// calls for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocblockTypingContext {
    /// Cursor is at a position where a type/class name is expected.
    ///
    /// `partial` is the identifier fragment being typed (may be empty).
    /// `tag` is the tag name without `@` (e.g. `"throws"`, `"param"`).
    Type { partial: String, tag: String },
    /// Cursor is at a position where a `$variable` name is expected
    /// (e.g. after the type in `@param Type $`).
    ///
    /// `partial` is the `$…` fragment being typed (including the `$`).
    Variable { partial: String },
}

/// Tags whose first argument is a type expression.
const TYPE_TAGS: &[&str] = &[
    "param",
    "return",
    "var",
    "throws",
    "property",
    "property-read",
    "property-write",
    "mixin",
    "extends",
    "implements",
    "use",
    "phpstan-param",
    "phpstan-return",
    "phpstan-self-out",
    "phpstan-this-out",
    "phpstan-assert",
    "phpstan-assert-if-true",
    "phpstan-assert-if-false",
    "phpstan-require-extends",
    "phpstan-require-implements",
    "phpstan-sealed",
    "psalm-param",
    "psalm-return",
];

/// Tags where a `$variable` follows the type (second argument).
const VARIABLE_TAGS: &[&str] = &[
    "param",
    "property",
    "property-read",
    "property-write",
    "phpstan-param",
    "phpstan-assert",
    "phpstan-assert-if-true",
    "phpstan-assert-if-false",
    "psalm-param",
];

/// Detect whether the cursor is at a **type** or **$variable** position
/// inside a PHPDoc tag line.
///
/// Returns `None` when the cursor is in a description area, on a line
/// without a recognised tag, or outside a docblock.
///
/// # Examples
///
/// ```text
/// /** @param |          → Type { partial: "" }
/// /** @param Str|       → Type { partial: "Str" }
/// /** @param string |   → Variable { partial: "" }
/// /** @param string $n| → Variable { partial: "$n" }
/// /** @return |         → Type { partial: "" }
/// /** @return Coll|     → Type { partial: "Coll" }
/// /** @throws |         → Type { partial: "" }
/// ```
pub fn detect_docblock_typing_position(
    content: &str,
    position: Position,
) -> Option<DocblockTypingContext> {
    if !is_inside_docblock(content, position) {
        return None;
    }

    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;
    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = crate::util::utf16_col_to_byte_offset(line, position.character);
    let before_cursor = &line[..col];

    // Find the `@tag` on this line.
    // Look for `@` preceded by whitespace or `*` (docblock prefix).
    let tag_name = extract_tag_name_from_line(before_cursor)?;

    // Only recognised type-accepting tags trigger completion.
    let tag_lower = tag_name.to_lowercase();
    if !TYPE_TAGS.iter().any(|t| *t == tag_lower) {
        return None;
    }

    // Find where the tag ends in `before_cursor`.
    // The tag is `@<tag_name>` — find the byte right after it.
    let at_pos = before_cursor.rfind('@')?;
    let tag_end = at_pos + 1 + tag_name.len();

    // Text between the end of the tag and the cursor.
    let after_tag = &before_cursor[tag_end..];

    // If there's no whitespace after the tag yet, the user is still
    // typing the tag name — `extract_phpdoc_prefix` handles that.
    if after_tag.is_empty() || !after_tag.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }

    let trimmed = after_tag.trim_start();

    // Nothing after whitespace → empty type position.
    if trimmed.is_empty() {
        return Some(DocblockTypingContext::Type {
            partial: String::new(),
            tag: tag_lower.clone(),
        });
    }

    // Does the text after the tag already contain a complete type
    // followed by whitespace?  We need to respect balanced brackets
    // so that `array<string, int>` counts as one type token.
    let (type_token_len, finished) = measure_type_token(trimmed);

    if !finished {
        // Still inside the type expression — extract the partial
        // identifier fragment being typed (the last word-like segment).
        let partial = extract_trailing_identifier(trimmed);
        return Some(DocblockTypingContext::Type {
            partial,
            tag: tag_lower.clone(),
        });
    }

    // The type token is complete.  Does this tag expect a $variable next?
    let expects_var = VARIABLE_TAGS.iter().any(|t| *t == tag_lower);
    if !expects_var {
        // Tags like @return, @throws, @mixin — after the type it's
        // just a description, no special completion.
        return None;
    }

    // Text after the type token.
    let after_type = trimmed[type_token_len..].trim_start();
    if after_type.is_empty() {
        // Space after the type, nothing typed yet → variable position.
        return Some(DocblockTypingContext::Variable {
            partial: String::new(),
        });
    }

    if after_type.starts_with('$') {
        // User is typing a variable name.
        // Extract the partial `$…` fragment (up to cursor).
        let var_end = after_type
            .find(|c: char| c.is_whitespace())
            .unwrap_or(after_type.len());
        let var_fragment = &after_type[..var_end];

        // If there's a space after the variable, we're in description.
        if var_end < after_type.len() {
            return None;
        }

        return Some(DocblockTypingContext::Variable {
            partial: var_fragment.to_string(),
        });
    }

    // Something else after the type (not `$`) — description territory.
    None
}

/// Extract the tag name (without `@`) from a docblock line prefix.
///
/// Scans backward to find the last `@` that is preceded by whitespace
/// or `*`, then reads forward through alphanumeric/`-`/`_` characters.
fn extract_tag_name_from_line(before_cursor: &str) -> Option<String> {
    // Find the last `@` that's preceded by whitespace or `*`.
    let bytes = before_cursor.as_bytes();
    let mut at_pos = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'@' && (i == 0 || bytes[i - 1].is_ascii_whitespace() || bytes[i - 1] == b'*') {
            at_pos = Some(i);
        }
    }
    let at = at_pos?;
    let after_at = &before_cursor[at + 1..];

    // Read tag name: alphanumeric, `-`, `_`
    let tag_end = after_at
        .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .unwrap_or(after_at.len());

    if tag_end == 0 {
        return None;
    }

    Some(after_at[..tag_end].to_string())
}

/// Measure the length of a type token at the start of `text`.
///
/// Returns `(byte_length, finished)` where `finished` is `true` when
/// the type token is followed by whitespace (meaning it's complete).
///
/// Handles balanced `<>`, `{}`, and `()` so that generic types like
/// `array<string, int>` and array shapes like `array{name: string}`
/// are treated as a single token.
fn measure_type_token(text: &str) -> (usize, bool) {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut depth_angle: i32 = 0;
    let mut depth_brace: i32 = 0;
    let mut depth_paren: i32 = 0;

    while i < len {
        let b = bytes[i];

        // Inside nested brackets, keep consuming.
        if depth_angle > 0 || depth_brace > 0 || depth_paren > 0 {
            match b {
                b'<' => depth_angle += 1,
                b'>' => depth_angle -= 1,
                b'{' => depth_brace += 1,
                b'}' => depth_brace -= 1,
                b'(' => depth_paren += 1,
                b')' => depth_paren -= 1,
                _ => {}
            }
            i += 1;
            continue;
        }

        // At depth 0:
        match b {
            b'<' => depth_angle += 1,
            b'{' => depth_brace += 1,
            b'(' => depth_paren += 1,
            // Union / intersection separators — type continues.
            b'|' | b'&' => {}
            // Whitespace at depth 0 → type token is complete.
            _ if b.is_ascii_whitespace() => return (i, true),
            _ => {}
        }
        i += 1;
    }

    // Reached end of text — type is unfinished (cursor is inside it).
    (i, false)
}

/// Extract the trailing identifier fragment from a partial type string.
///
/// Walks backward from the end through characters that can appear in a
/// PHP class name (`A-Za-z0-9_\`).  This handles union types like
/// `string|Fo` → `"Fo"` and nullable types like `?Fo` → `"Fo"`.
fn extract_trailing_identifier(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut i = bytes.len();
    while i > 0
        && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_' || bytes[i - 1] == b'\\')
    {
        i -= 1;
    }
    text[i..].to_string()
}

/// Check whether the cursor at `position` is inside a `/** … */` docblock
/// comment, and if so, return the partial tag prefix the user is typing
/// (e.g. `"@par"`, `"@"`, `"@phpstan-a"`).
///
/// Returns `None` if the cursor is not inside a docblock or is not at a
/// tag position (i.e. no `@` on the current line before the cursor).
pub fn extract_phpdoc_prefix(content: &str, position: Position) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = position.line as usize;
    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let chars: Vec<char> = line.chars().collect();
    let col = (position.character as usize).min(chars.len());

    // Walk backwards from cursor to find `@`
    let mut i = col;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '-' || chars[i - 1] == '_') {
        i -= 1;
    }

    // Must be preceded by `@`
    if i == 0 || chars[i - 1] != '@' {
        return None;
    }
    // Include the `@`
    i -= 1;

    // The character before `@` (if any) must be whitespace or `*`
    // (typical docblock line prefix).  This prevents matching email
    // addresses or annotations in regular strings.
    if i > 0 {
        let prev = chars[i - 1];
        if !prev.is_whitespace() && prev != '*' {
            return None;
        }
    }

    let prefix: String = chars[i..col].iter().collect();

    // Now verify that we are actually inside a `/** … */` block.
    if !is_inside_docblock(content, position) {
        return None;
    }

    Some(prefix)
}

/// Determine what PHP symbol follows the docblock at the cursor position.
///
/// This looks at the content after the docblock's closing `*/` (or after
/// the current line if the docblock is still open) to identify the next
/// meaningful PHP token.
pub fn detect_context(content: &str, position: Position) -> DocblockContext {
    let remaining = get_text_after_docblock(content, position);
    classify_from_tokens(&remaining)
}

/// Extract information about the PHP symbol following the docblock.
///
/// Parses the declaration line(s) after `*/` to pull out parameter names,
/// type hints, return types, etc.
pub fn extract_symbol_info(content: &str, position: Position) -> SymbolInfo {
    let remaining = get_text_after_docblock(content, position);
    parse_symbol_info(&remaining)
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Get the text after the current docblock's closing `*/`.
///
/// If the docblock isn't closed yet, returns the text after the cursor
/// position (skipping lines that look like docblock continuation).
fn get_text_after_docblock(content: &str, position: Position) -> String {
    let byte_offset = position_to_byte_offset(content, position);
    let after_cursor = &content[byte_offset.min(content.len())..];

    if let Some(close_pos) = after_cursor.find("*/") {
        after_cursor[close_pos + 2..].to_string()
    } else {
        // Docblock not closed — return whatever follows
        after_cursor.to_string()
    }
}

/// Classify the PHP symbol from the first meaningful tokens.
fn classify_from_tokens(text: &str) -> DocblockContext {
    // Track whether a blank line appears before the first code line.
    // Inline `@var` annotations must be on the line immediately before
    // the variable assignment — a blank line in between means the
    // docblock is not attached to the assignment.
    //
    // The text starts right after `*/`, so the very first line is the
    // remainder of the closing `*/` line (typically empty or whitespace).
    // That first line is NOT a real blank gap — only subsequent empty
    // lines count.
    let mut saw_blank_line = false;
    let mut first_code_line: Option<&str> = None;
    let mut tokens = Vec::new();
    let mut skipped_first_line = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !skipped_first_line {
                // The tail of the `*/` line — always skip it.
                skipped_first_line = true;
            } else if tokens.is_empty() {
                // A real blank line before any code.
                saw_blank_line = true;
            }
            continue;
        }
        skipped_first_line = true;
        if trimmed.starts_with('*') || trimmed.starts_with("/**") {
            continue;
        }
        if first_code_line.is_none() {
            first_code_line = Some(trimmed);
        }
        for word in trimmed.split_whitespace() {
            // Store original casing so that uppercase-initial class names
            // (e.g. `Collection`) are recognised as type hints later.
            tokens.push(word.to_string());
            if tokens.len() >= 6 {
                break;
            }
        }
        if tokens.len() >= 6 {
            break;
        }
    }

    if tokens.is_empty() {
        return DocblockContext::Unknown;
    }

    let mut saw_modifier = false;
    for token in &tokens {
        let t = token.as_str();
        let lower = t.to_ascii_lowercase();
        match lower.as_str() {
            "function" => return DocblockContext::FunctionOrMethod,
            "class" | "interface" | "trait" | "enum" => return DocblockContext::ClassLike,
            "const" => return DocblockContext::Constant,
            "public" | "protected" | "private" | "static" | "readonly" | "abstract" | "final" => {
                saw_modifier = true;
                continue;
            }
            _ => {
                if t.starts_with('$') {
                    // A `$variable` preceded by an access modifier like
                    // `public` is a property declaration.  Without a
                    // modifier it *might* be an inline variable assignment
                    // (e.g. `/** @var User $u */ $u = getUser();`).
                    if saw_modifier {
                        return DocblockContext::Property;
                    }
                    // Inline context requires:
                    //  1. No blank line between `*/` and the code line.
                    //  2. The statement is a variable assignment (`$v = …`),
                    //     not a method call (`$v->foo()`) or other use.
                    if !saw_blank_line && is_variable_assignment(first_code_line.unwrap_or("")) {
                        return DocblockContext::Inline;
                    }
                    return DocblockContext::Unknown;
                }
                if t.starts_with('?')
                    || t.starts_with('\\')
                    || t.chars().next().is_some_and(|c| c.is_uppercase())
                    || is_type_keyword(&lower)
                {
                    continue;
                }
                return DocblockContext::Unknown;
            }
        }
    }

    DocblockContext::Unknown
}

/// Check whether a trimmed code line is a simple variable assignment.
///
/// Returns `true` for lines like `$foo = expr;` or `$foo = expr` (no
/// semicolon yet).  Returns `false` for method calls (`$foo->bar()`),
/// comparisons (`$foo == bar`), and other non-assignment uses.
fn is_variable_assignment(line: &str) -> bool {
    // Find the first `$` — start of the variable name.
    let Some(dollar) = line.find('$') else {
        return false;
    };
    // Skip past the variable name (alphanumeric, `_`, `$`).
    let after_name = &line[dollar..];
    let name_len = after_name
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .count();
    let rest = after_name[name_len..].trim_start();

    // Must start with `=` but not `==` or `=>`.
    if let Some(stripped) = rest.strip_prefix('=') {
        !stripped.starts_with('=') && !stripped.starts_with('>')
    } else {
        false
    }
}

/// Check if a token is a PHP type keyword (used in property declarations).
fn is_type_keyword(token: &str) -> bool {
    crate::php_type::is_keyword_type(token)
}

/// Parse symbol info (params, return type, property type) from the
/// declaration text following the docblock.
fn parse_symbol_info(text: &str) -> SymbolInfo {
    let mut info = SymbolInfo::default();

    // Track blank lines — inline variable assignment detection requires
    // the assignment to be on the very next line (no blank gap).
    //
    // The text starts right after `*/`, so the first line is the tail
    // of the closing `*/` line (usually empty).  That does NOT count
    // as a blank gap — only subsequent empty lines do.
    let mut saw_blank_before_code = false;
    let mut skipped_first_line = false;

    // Collect the declaration — may span multiple lines until `{` or `;`
    let mut decl = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('*') || trimmed.starts_with("/**") {
            continue;
        }
        if trimmed.is_empty() {
            if !skipped_first_line {
                skipped_first_line = true;
            } else if decl.is_empty() {
                saw_blank_before_code = true;
            }
            continue;
        }
        skipped_first_line = true;
        decl.push(' ');
        decl.push_str(trimmed);
        if trimmed.contains('{') || trimmed.contains(';') {
            break;
        }
    }

    let decl = decl.trim();
    if decl.is_empty() {
        return info;
    }

    // Check if it's a function/method
    if let Some(func_pos) = find_keyword_pos(decl, "function") {
        let after_func = &decl[func_pos + 8..]; // "function" is 8 chars

        // Find the parameter list between ( and )
        if let Some(open_paren) = after_func.find('(') {
            let after_open = &after_func[open_paren + 1..];
            if let Some(close_paren) = find_matching_paren(after_open) {
                let params_str = &after_open[..close_paren];
                info.params = parse_params(params_str);

                // Extract return type: look for `: Type` after the closing paren
                let after_close = &after_open[close_paren + 1..];
                info.return_type = extract_return_type_from_decl(after_close);
            }
        }
    } else {
        // Property or constant — extract type hint
        info.type_hint = extract_property_type(decl);

        // For inline variable assignments (e.g. `$items = getUser();`),
        // extract the variable name so that @var completion can detect
        // whether a variable definition follows the docblock.
        //
        // Only extract when:
        //  - There is no blank line between `*/` and the code.
        //  - The statement is actually an assignment (`$v = …`), not a
        //    method call (`$v->foo()`) or comparison (`$v == bar`).
        if !saw_blank_before_code
            && is_variable_assignment(decl)
            && let Some(dollar) = decl.find('$')
        {
            let name: String = decl[dollar..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if !name.is_empty() {
                info.variable_name = Some(name);
            }
        }
    }

    info
}

/// Parse a comma-separated parameter list into `(type_hint, $name)` pairs.
fn parse_params(params_str: &str) -> Vec<(Option<PhpType>, String)> {
    if params_str.trim().is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();

    // Split on commas, respecting nested parens/angle brackets
    for param in split_params(params_str) {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }

        // Each param looks like: [Type] [$name] [= default]
        // or: [Type] &$name, [Type] ...$name
        let tokens: Vec<&str> = param.split_whitespace().collect();

        let mut type_hint: Option<PhpType> = None;
        let mut name: Option<String> = None;

        for token in &tokens {
            let t = *token;
            // Skip default value part
            if t == "=" {
                break;
            }
            if t.starts_with('$') || t.starts_with("&$") || t.starts_with("...$") {
                // This is the variable name
                let clean = t.trim_start_matches("...").trim_start_matches('&');
                name = Some(clean.to_string());
                break;
            }
            // Skip constructor promotion modifiers — they are not type hints
            match t.to_lowercase().as_str() {
                "public" | "protected" | "private" | "static" | "readonly" => continue,
                _ => {}
            }
            // Otherwise it's (part of) the type hint
            if let Some(existing) = type_hint {
                // Union/intersection types with spaces shouldn't happen,
                // but handle it gracefully
                type_hint = Some(PhpType::parse(&format!("{}{}", existing, t)));
            } else {
                type_hint = Some(PhpType::parse(t));
            }
        }

        if let Some(n) = name {
            result.push((type_hint, n));
        }
    }

    result
}

/// Extract the return type from the portion after `)` in a function declaration.
///
/// Looks for `: Type` pattern.
fn extract_return_type_from_decl(after_close_paren: &str) -> Option<PhpType> {
    let trimmed = after_close_paren.trim();
    let rest = trimmed.strip_prefix(':')?;
    let rest = rest.trim();

    // The return type is everything up to `{`, `;`, or end
    let end = rest.find(['{', ';']).unwrap_or(rest.len());

    let ret_type = rest[..end].trim();
    if ret_type.is_empty() {
        None
    } else {
        Some(PhpType::parse(ret_type))
    }
}

/// Extract the type hint from a property declaration.
///
/// Handles: `public string $name`, `protected ?int $count = 0`,
/// `private static array $cache`, `readonly Foo $bar`, etc.
fn extract_property_type(decl: &str) -> Option<PhpType> {
    let tokens: Vec<&str> = decl.split_whitespace().collect();

    // Walk tokens: skip modifiers, the token before `$var` is the type
    let mut last_non_modifier: Option<PhpType> = None;
    for token in &tokens {
        let t = *token;
        let lower = t.to_lowercase();

        if t.starts_with('$') {
            // The previous non-modifier token is the type
            return last_non_modifier;
        }

        // Skip `=` and everything after it
        if t == "=" || t == ";" {
            break;
        }

        match lower.as_str() {
            "public" | "protected" | "private" | "static" | "readonly" | "const" => {
                continue;
            }
            _ => {
                last_non_modifier = Some(PhpType::parse(t));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_promoted_property_type() {
        let result = parse_params("public readonly bool $selected");
        assert_eq!(result.len(), 1);
        let (type_hint, name) = &result[0];
        assert_eq!(type_hint, &Some(PhpType::parse("bool")));
        assert_eq!(name, "$selected");
    }
}
