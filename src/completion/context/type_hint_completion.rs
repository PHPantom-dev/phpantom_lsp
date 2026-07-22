//! Type hint completion inside function / method definitions.
//!
//! When the cursor is at a type-hint position — e.g. a function parameter
//! type, a return type after `):`, a property type after a visibility
//! modifier, or a union/intersection/nullable modifier — this module
//! detects the context and provides PHP native scalar types alongside
//! class-name completions (but **not** constants or standalone functions,
//! which are invalid in type positions).
//!
//! Detection is intentionally conservative: we only fire when we can
//! confirm the cursor is inside a function/method *definition* (not a
//! call) or a property/promoted-parameter declaration.

use crate::completion::named_args::{find_enclosing_open_paren, position_to_char_offset};
use crate::text_scan::skip_string_backward;
use tower_lsp::lsp_types::Position;

/// PHP native types valid in type-hint positions (PHP 8.x).
///
/// These are offered before class-name results so that typing `str`
/// suggests `string` alongside any user-defined classes starting with
/// `str`.  The list deliberately omits PHPStan-only pseudo-types like
/// `class-string`, `positive-int`, `non-empty-string`, etc. that are
/// not valid in native PHP declarations.
pub(crate) const PHP_NATIVE_TYPES: &[&str] = &[
    "string", "int", "float", "bool", "array", "object", "mixed", "void", "null", "callable",
    "iterable", "never", "self", "static", "parent", "true", "false",
];

/// Context returned when the cursor is at a type-hint position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeHintContext {
    /// The partial identifier the user has typed so far (may be empty).
    pub partial: String,
    /// Whether the insertion needs a leading space.
    ///
    /// This is `true` when the cursor is in a return-type position and
    /// the character immediately before the partial (after the `:`) is
    /// not a space.  For example, `function foo():` with the cursor
    /// right after `:` needs a space so that the result is `: string`
    /// rather than `:string`.
    pub needs_space_prefix: bool,
}

/// Detect whether the cursor is at a type-hint position inside a
/// function/method definition, return-type declaration, or property
/// declaration.
///
/// Returns `Some(TypeHintContext)` with the partial text when the cursor
/// is eligible for type-hint completion, or `None` otherwise.
pub(crate) fn detect_type_hint_context(
    content: &str,
    position: Position,
) -> Option<TypeHintContext> {
    let chars: Vec<char> = content.chars().collect();
    let cursor = position_to_char_offset(&chars, position)?;

    // ── Extract partial identifier ──────────────────────────────────
    let mut partial_start = cursor;
    while partial_start > 0
        && (chars[partial_start - 1].is_alphanumeric()
            || chars[partial_start - 1] == '_'
            || chars[partial_start - 1] == '\\')
    {
        partial_start -= 1;
    }

    // Preceded by `$` → variable, not a type.
    if partial_start > 0 && chars[partial_start - 1] == '$' {
        return None;
    }

    // Preceded by `->` or `::` → member access, not a type.
    if partial_start >= 2 && chars[partial_start - 2] == '-' && chars[partial_start - 1] == '>' {
        return None;
    }
    if partial_start >= 2 && chars[partial_start - 2] == ':' && chars[partial_start - 1] == ':' {
        return None;
    }

    let partial: String = chars[partial_start..cursor].iter().collect();

    // ── Skip whitespace before the partial ──────────────────────────
    let before = skip_whitespace_backward(&chars, partial_start);

    if before == 0 {
        return None;
    }

    let prev_char = chars[before - 1];

    // ── Check: return type position  `):`  ──────────────────────────
    if prev_char == ':' && is_return_type_colon(&chars, before - 1) {
        // Check whether the character immediately after the colon is a
        // space.  When the user types `):` without a trailing space, we
        // need to prepend one to the inserted text so the result is
        // `: string` instead of `:string`.
        let colon_pos = before - 1;
        let needs_space = if partial.is_empty() {
            // No partial typed yet — check if there is NO whitespace
            // between the colon and the cursor.
            colon_pos + 1 == partial_start
        } else {
            // Partial already typed — check if the char right after the
            // colon is not whitespace (i.e. the partial is jammed
            // against the colon).
            colon_pos + 1 == partial_start
        };
        return Some(TypeHintContext {
            partial,
            needs_space_prefix: needs_space,
        });
    }

    // ── Check: inside function-definition parameter list ────────────
    if prev_char == '(' || prev_char == ',' {
        let open_paren = if prev_char == '(' {
            before - 1
        } else {
            find_enclosing_open_paren(&chars, before - 1)?
        };

        if is_function_definition_paren(&chars, open_paren) {
            if prev_char == ',' {
                // Verify the preceding parameter is complete (has a `$`
                // variable name) so we don't trigger mid-type.
                let text_between: String = chars[open_paren + 1..before - 1].iter().collect();
                {
                    let last_param = text_between.rsplit(',').next()?;
                    if !last_param.contains('$') {
                        return None;
                    }
                }
            }
            return Some(TypeHintContext {
                partial,
                needs_space_prefix: false,
            });
        }
    }

    // ── Check: type modifier `?`, `|`, `&` in a type context ────────
    if (prev_char == '?' || prev_char == '|' || prev_char == '&')
        && is_type_modifier_in_definition(&chars, before - 1)
    {
        return Some(TypeHintContext {
            partial,
            needs_space_prefix: false,
        });
    }

    // ── Check: after property / promoted-param modifier keyword ─────
    // This covers both class property declarations (`public int $x`)
    // and promoted constructor parameters (`private readonly string $y`).
    if prev_char.is_alphabetic() && is_after_modifier_keyword(&chars, before) {
        // With an empty partial right after a modifier (e.g. `public |`),
        // keyword completion should drive the next token (`function`,
        // `const`, `static`, …).  Type-hint completion kicks in once the
        // user actually starts typing a type token.
        if partial.is_empty() {
            return None;
        }
        // If the partial starts with a lowercase letter and is a prefix
        // of any member-declaration keyword, defer to keyword completion
        // so that e.g. `public fu` offers `function` rather than type
        // hints.  Partials starting with an uppercase letter (e.g. `Us`,
        // `User`) are type names, not keywords, so they skip this check.
        if partial.starts_with(|c: char| c.is_ascii_lowercase()) {
            const MEMBER_DECLARATION_KEYWORDS: &[&str] = &[
                "function",
                "fn",
                "const",
                "static",
                "final",
                "abstract",
                "readonly",
                "public",
                "protected",
                "private",
                "use",
                "class",
            ];
            let partial_lower = partial.to_lowercase();
            let is_keyword_prefix = MEMBER_DECLARATION_KEYWORDS
                .iter()
                .any(|kw| kw.starts_with(partial_lower.as_str()));
            if is_keyword_prefix {
                return None;
            }
        }
        return Some(TypeHintContext {
            partial,
            needs_space_prefix: false,
        });
    }

    None
}

/// Whether the cursor is typing a declared member *name* (not a type).
///
/// True for positions like:
/// - `protected function getC|` (method name)
/// - `public const FO|` (class constant name)
/// - `case Pend|` (enum case name)
///
/// Property names are not covered here: they always start with `$`, and
/// the class-name completer already bails when the partial is preceded
/// by `$`.  Type positions like `protected User|` intentionally still
/// offer class names.
pub(crate) fn is_function_or_const_name_position(content: &str, position: Position) -> bool {
    let chars: Vec<char> = content.chars().collect();
    let Some(cursor) = position_to_char_offset(&chars, position) else {
        return false;
    };

    // Skip the partial identifier being typed.
    let mut i = cursor;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }

    // Require whitespace between the keyword and the name (or empty name
    // still after the keyword: `function |`).
    let after_ident = i;
    while i > 0 && chars[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i == after_ident && after_ident != cursor {
        // Identifier jammed against previous token without space — not a
        // declaration name (e.g. `functionfoo`).
        return false;
    }
    // Empty partial right after keyword still needs the keyword check;
    // `function|` (no space yet) is not a name position.
    if i == after_ident {
        return false;
    }

    if check_keyword_ending_at(&chars, i, "fn") || check_keyword_ending_at(&chars, i, "case") {
        return true;
    }
    // `function` / `const` declare member names, but not after `use`
    // (`use function foo`, `use const BAR` still need symbol completion).
    if check_keyword_ending_at(&chars, i, "function") {
        return !preceded_by_use_keyword(&chars, i - "function".len());
    }
    if check_keyword_ending_at(&chars, i, "const") {
        return !preceded_by_use_keyword(&chars, i - "const".len());
    }
    false
}

/// Whether `pos` (end of a keyword span) is preceded by the `use` keyword
/// with only whitespace between (`use function` / `use const`).
fn preceded_by_use_keyword(chars: &[char], keyword_start: usize) -> bool {
    let before = skip_whitespace_backward(chars, keyword_start);
    check_keyword_ending_at(chars, before, "use")
}

// ─── Private helpers ────────────────────────────────────────────────────────

/// Skip whitespace (spaces, tabs, newlines) backward from `pos`
/// (exclusive) and return the new position.
fn skip_whitespace_backward(chars: &[char], pos: usize) -> usize {
    let mut i = pos;
    while i > 0 && chars[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    i
}

/// Check whether the `(` at `paren_pos` belongs to a function or method
/// *definition* (as opposed to a function/method *call*).
///
/// Walks backward from just before the `(`, skipping whitespace and
/// the optional function name, then looks for `function` or `fn`.
fn is_function_definition_paren(chars: &[char], paren_pos: usize) -> bool {
    let mut i = paren_pos;

    // Skip whitespace before `(`
    while i > 0 && chars[i - 1].is_ascii_whitespace() {
        i -= 1;
    }

    // Walk backward through the identifier (function name — may be empty
    // for anonymous functions / closures).
    let name_end = i;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }

    let name: String = chars[i..name_end].iter().collect();

    // Anonymous `function(` or arrow `fn(` — the name *is* the keyword.
    if name == "function" || name == "fn" {
        return true;
    }

    // Named function / method: skip whitespace before the name and check
    // for the `function` or `fn` keyword.
    let mut j = i;
    while j > 0 && chars[j - 1].is_ascii_whitespace() {
        j -= 1;
    }

    if check_keyword_ending_at(chars, j, "function") {
        return true;
    }
    if check_keyword_ending_at(chars, j, "fn") {
        return true;
    }

    false
}

/// Check whether the `:` at `colon_pos` is a return-type colon — i.e.
/// it is preceded (possibly with whitespace) by `)`.
///
/// Also verifies the `)` belongs to a function definition.
fn is_return_type_colon(chars: &[char], colon_pos: usize) -> bool {
    let i = skip_whitespace_backward(chars, colon_pos);
    if i == 0 || chars[i - 1] != ')' {
        return false;
    }

    // Find the matching `(` for this `)`.
    let close_paren = i - 1;
    if let Some(open_paren) = find_matching_paren_backward(chars, close_paren) {
        return is_function_definition_paren(chars, open_paren);
    }
    false
}

/// Find the matching `(` for the `)` at `close_pos`, respecting nesting
/// and string literals.
fn find_matching_paren_backward(chars: &[char], close_pos: usize) -> Option<usize> {
    if chars[close_pos] != ')' {
        return None;
    }
    let mut depth: u32 = 1;
    let mut i = close_pos;
    while i > 0 {
        i -= 1;
        match chars[i] {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            '\'' => {
                i = skip_string_backward(chars, i, '\'');
            }
            '"' => {
                i = skip_string_backward(chars, i, '"');
            }
            '{' | '[' | ';' => return None,
            _ => {}
        }
    }
    None
}

/// Check whether a type modifier (`?`, `|`, or `&`) at `mod_pos` sits
/// inside a function-definition type context.
///
/// Walks backward past existing type names and modifiers until it finds
/// the structural character that anchors the context (one of `(`, `,`,
/// `:` for return type, or a modifier keyword for property declarations).
fn is_type_modifier_in_definition(chars: &[char], mod_pos: usize) -> bool {
    // Walk backward past the preceding type name (if any) and any
    // further type modifiers, collecting context anchors.
    let mut i = mod_pos;

    // Allow chained modifiers:  `A|B|`  or `?A|`
    loop {
        // Skip whitespace
        i = skip_whitespace_backward(chars, i);

        if i == 0 {
            return false;
        }

        // Walk backward through type identifier
        let type_end = i;
        while i > 0
            && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '\\')
        {
            i -= 1;
        }

        // Skip whitespace
        i = skip_whitespace_backward(chars, i);

        if i == 0 {
            return false;
        }

        let prev = chars[i - 1];

        match prev {
            // Another type modifier — keep walking.
            '?' | '|' | '&' => {
                i -= 1;
                continue;
            }
            '(' => {
                // Check if this paren is a function definition.
                return is_function_definition_paren(chars, i - 1);
            }
            ',' => {
                // Find enclosing paren and check if it's a function definition.
                if let Some(open) = find_enclosing_open_paren(chars, i - 1)
                    && is_function_definition_paren(chars, open)
                {
                    // Verify the previous param segment has a `$`
                    let between: String = chars[open + 1..i - 1].iter().collect();
                    if let Some(last_seg) = between.rsplit(',').next() {
                        return last_seg.contains('$');
                    }
                }
                return false;
            }
            ':' => {
                // Return type colon.
                return is_return_type_colon(chars, i - 1);
            }
            _ => {
                // Could be after a modifier keyword (property or promoted param).
                if prev.is_alphabetic() {
                    if is_after_modifier_keyword(chars, i) {
                        return true;
                    }
                    // Could also be: we already consumed the type name above
                    // and now the preceding text ends with a modifier.
                    // But if type_end == i we didn't consume anything, so this
                    // isn't a type context.
                    if type_end == i {
                        return false;
                    }
                    // We consumed a type name; check if what's before it is a
                    // modifier keyword (e.g. `public ?string|`).
                    return is_after_modifier_keyword(chars, i);
                }
                return false;
            }
        }
    }
}

/// Check whether the text ending at `pos` (exclusive) is one of the PHP
/// modifier keywords that can precede a type hint: `public`, `protected`,
/// `private`, `readonly`, or `static`.
///
/// Also handles chains like `private readonly` by recursively checking
/// the preceding token.
fn is_after_modifier_keyword(chars: &[char], pos: usize) -> bool {
    if pos == 0 {
        return false;
    }

    // Walk backward through alphabetic word.
    let word_end = pos;
    let mut start = pos;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }

    if start == word_end {
        return false;
    }

    let word: String = chars[start..word_end].iter().collect();

    match word.as_str() {
        "public" | "protected" | "private" | "readonly" | "static" => {
            // Ensure word boundary before the keyword.
            if start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
                return false;
            }

            // Also handle `function` keyword before the modifier — this
            // means we're looking at a method declaration, not a property.
            // But that's fine: `function` can only appear before the name
            // and parens, so if we're here the cursor must be at a property
            // or promoted-param position.
            //
            // However, we do NOT want to trigger inside function
            // parameter lists that happen to have a modifier keyword
            // on a preceding line — the paren-based detection handles
            // those.  We check that we're NOT inside an unmatched `(`.
            // This avoids double-triggering for promoted constructor
            // params (the paren-based check already handles those).
            //
            // Actually, we DO want to trigger for promoted constructor
            // params like `__construct(private readonly |)` because the
            // modifier is the immediate predecessor.  The paren-based
            // detection would NOT fire there because `readonly` is not
            // `(` or `,`.  So this branch is the correct handler for
            // promoted params with modifiers.
            true
        }
        _ => false,
    }
}

/// Check whether a specific keyword ends at `pos` (exclusive) in `chars`,
/// with a word-boundary before it.
fn check_keyword_ending_at(chars: &[char], pos: usize, keyword: &str) -> bool {
    let kw_len = keyword.len();
    if pos < kw_len {
        return false;
    }
    let start = pos - kw_len;
    let candidate: String = chars[start..pos].iter().collect();
    if candidate != keyword {
        return false;
    }
    // Ensure word boundary before the keyword.
    if start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        return false;
    }
    true
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "type_hint_completion_tests.rs"]
mod tests;
