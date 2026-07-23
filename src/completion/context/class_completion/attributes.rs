//! Detection of the `#[…]` attribute-list context and its target kind.
use tower_lsp::lsp_types::Position;

/// Detect whether the cursor is inside a PHP attribute list (`#[…]`).
///
/// `j` is the char offset just before the partial identifier the user is
/// typing.  We walk backward through whitespace, commas, nested parens,
/// and prior attribute names looking for the opening `#[`.
///
/// Returns `Some(target_flags)` when confirmed, or `None` when the
/// cursor is not inside an attribute list.
pub(super) fn detect_attribute_context(
    chars: &[char],
    j: usize,
    content: &str,
    position: Position,
) -> Option<u8> {
    let mut k = j;

    // Walk backward over whitespace, comma-separated attributes,
    // and their argument lists to find the `#[` opener.
    loop {
        // Skip whitespace.
        while k > 0 && chars[k - 1].is_ascii_whitespace() {
            k -= 1;
        }

        if k == 0 {
            return None;
        }

        // If we see `#[`, we found the attribute list start.
        if k >= 2 && chars[k - 2] == '#' && chars[k - 1] == '[' {
            let target = infer_attribute_target(content, position);
            return Some(target);
        }

        // If we see `[` without a preceding `#`, this is not a PHP
        // attribute (could be an array).
        if chars[k - 1] == '[' {
            return None;
        }

        // If we see `)`, skip over a balanced parenthesised argument
        // list (e.g. `#[Route('/foo'), |`).
        if chars[k - 1] == ')' {
            k -= 1;
            let mut depth = 1i32;
            while k > 0 && depth > 0 {
                k -= 1;
                match chars[k] {
                    ')' => depth += 1,
                    '(' => depth -= 1,
                    _ => {}
                }
            }
            // Now `k` points at the `(`.  Skip backward past the
            // attribute name before it.
            while k > 0
                && (chars[k - 1].is_alphanumeric() || chars[k - 1] == '_' || chars[k - 1] == '\\')
            {
                k -= 1;
            }
            // Skip whitespace and check for comma.
            while k > 0 && chars[k - 1].is_ascii_whitespace() {
                k -= 1;
            }
            if k > 0 && chars[k - 1] == ',' {
                k -= 1;
                continue;
            }
            // No comma — check for `#[` directly.
            continue;
        }

        // If we see `,`, skip it and continue backward (multiple
        // attributes: `#[A, B, |`).
        if chars[k - 1] == ',' {
            k -= 1;
            // Skip whitespace.
            while k > 0 && chars[k - 1].is_ascii_whitespace() {
                k -= 1;
            }
            // Skip the preceding attribute name (and possibly its args).
            if k > 0 && chars[k - 1] == ')' {
                // Has argument list — loop will handle it next iteration.
                continue;
            }
            // Skip bare attribute name.
            while k > 0
                && (chars[k - 1].is_alphanumeric() || chars[k - 1] == '_' || chars[k - 1] == '\\')
            {
                k -= 1;
            }
            continue;
        }

        // Nothing recognised — not inside an attribute list.
        return None;
    }
}

/// Infer the attribute target flags from the syntactic position of the
/// attribute list.
///
/// Scans lines after the cursor to find the declaration the attribute
/// applies to.  Uses brace depth to distinguish top-level declarations
/// (class, function) from class members (method, property, constant).
fn infer_attribute_target(content: &str, position: Position) -> u8 {
    use crate::types::attribute_target;

    let lines: Vec<&str> = content.lines().collect();
    let cursor_line = position.line as usize;

    // First check brace depth at the cursor line to know whether we are
    // at the top level or inside a class body.
    let depth = {
        let mut d = 0i32;
        for (idx, line) in lines.iter().enumerate() {
            if idx >= cursor_line {
                break;
            }
            for ch in line.chars() {
                match ch {
                    '{' => d += 1,
                    '}' => d -= 1,
                    _ => {}
                }
            }
        }
        d
    };

    // Scan forward from the line after the cursor, skipping blank lines
    // and additional attribute lines, to find the declaration keyword.
    for line in lines
        .iter()
        .take(lines.len().min(cursor_line + 10))
        .skip(cursor_line + 1)
    {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        // Inside a function/method parameter list.
        // We detect this when the following non-blank line starts with
        // a type-hint or `$` (parameter) rather than a declaration
        // keyword.  However, this is tricky to distinguish reliably.
        // For now, look for specific declaration keywords.

        // Tokenise the first few words of the line.
        let words = declaration_keywords(trimmed);

        if words.contains(&"class")
            || words.contains(&"interface")
            || words.contains(&"trait")
            || words.contains(&"enum")
        {
            return attribute_target::TARGET_CLASS;
        }

        if words.contains(&"function") {
            return if depth >= 1 {
                attribute_target::TARGET_METHOD
            } else {
                attribute_target::TARGET_FUNCTION
            };
        }

        if words.contains(&"const") {
            return attribute_target::TARGET_CLASS_CONSTANT;
        }

        // Inside a class body, if we see a visibility modifier or
        // `var`/`readonly`/`static` followed by a type or `$`, it is
        // a property.
        if depth >= 1 {
            // If the line contains `function`, it is a method
            // (handled above).  Otherwise, a modifier chain without
            // `function` or `const` is a property.
            let has_modifier = words.iter().any(|w| {
                matches!(
                    *w,
                    "public"
                        | "protected"
                        | "private"
                        | "readonly"
                        | "static"
                        | "var"
                        | "abstract"
                        | "final"
                )
            });
            if has_modifier {
                return attribute_target::TARGET_PROPERTY;
            }
        }

        // Could not determine — fall back to all targets.
        break;
    }

    // Fallback: if inside a class body, offer method/property/constant
    // targets.  At the top level, offer class/function.
    if depth >= 1 {
        attribute_target::TARGET_METHOD
            | attribute_target::TARGET_PROPERTY
            | attribute_target::TARGET_CLASS_CONSTANT
    } else {
        attribute_target::TARGET_CLASS | attribute_target::TARGET_FUNCTION
    }
}

/// Extract the leading declaration keywords from a source line.
///
/// Returns a vector of lowercase keyword strings found before the first
/// non-keyword token (identifier, `$`, `(`, etc.).  This is used by
/// [`infer_attribute_target`] to determine what kind of declaration
/// follows an attribute list.
fn declaration_keywords(line: &str) -> Vec<&str> {
    let mut result = Vec::new();
    for word in line.split_whitespace() {
        // Stop at tokens that are clearly not keywords.
        if word.starts_with('$')
            || word.starts_with('(')
            || word.starts_with('{')
            || word.starts_with('/')
            || word.starts_with('#')
        {
            break;
        }
        match word.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_') {
            "public" | "protected" | "private" | "static" | "abstract" | "final" | "readonly"
            | "function" | "class" | "interface" | "trait" | "enum" | "const" | "var" => {
                result.push(word.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_'));
            }
            _ => break,
        }
    }
    result
}
