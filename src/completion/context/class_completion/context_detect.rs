//! Syntactic detection of the [`super::ClassNameContext`] a cursor is in.
//!
//! Walks the raw source text backward from the cursor to find the
//! preceding keyword (`new`, `extends`, `implements`, `use`, …) without
//! requiring a full parse, since completion runs on text the user is
//! still typing (and thus often unparseable).
use tower_lsp::lsp_types::Position;

use crate::completion::named_args::position_to_char_offset;
#[cfg(test)]
use crate::types::ClassLikeKind;
#[cfg(test)]
use crate::util::short_name;

use super::ClassNameContext;
use super::attributes::detect_attribute_context;

/// Check whether a keyword (case-insensitive) ends exactly at position
/// `end` in the character array.
fn keyword_ends_at(chars: &[char], end: usize, keyword: &str) -> bool {
    let kw_len = keyword.len();
    if end < kw_len {
        return false;
    }
    let start = end - kw_len;

    // The character before the keyword must NOT be alphanumeric or `_`
    // (otherwise we matched the tail end of a longer identifier).
    if start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        return false;
    }

    let candidate: String = chars[start..end].iter().collect();
    candidate.eq_ignore_ascii_case(keyword)
}

/// Determine whether `extends` is in a class or interface declaration.
fn determine_extends_context(chars: &[char], extends_start: usize) -> ClassNameContext {
    // Walk backward past whitespace, then past any identifier (the
    // class/interface name itself), then past more whitespace, looking
    // for the `class` or `interface` keyword.
    let mut i = extends_start;
    while i > 0 && chars[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    // Skip over the class/interface name.
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }
    // Skip whitespace.
    while i > 0 && chars[i - 1].is_ascii_whitespace() {
        i -= 1;
    }

    // Check for `interface` first (longer match).
    if keyword_ends_at(chars, i, "interface") {
        return ClassNameContext::ExtendsInterface;
    }
    if keyword_ends_at(chars, i, "class") {
        return ClassNameContext::ExtendsClass;
    }
    // Could be after modifiers like `final`, `abstract`, `readonly`.
    // Walk past those and check again.
    for _ in 0..5 {
        while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            i -= 1;
        }
        while i > 0 && chars[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        if keyword_ends_at(chars, i, "class") {
            return ClassNameContext::ExtendsClass;
        }
    }
    // Fallback — allow anything.
    ClassNameContext::ExtendsClass
}

/// Count the brace depth at a given character position.
///
/// Used to distinguish top-level `use` (namespace import) from `use`
/// inside a class body (trait use).
fn brace_depth_at(chars: &[char], pos: usize) -> i32 {
    let mut depth = 0i32;
    for &c in &chars[..pos] {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// Detect the syntactic context for a class name being typed at
/// `position`.
///
/// Walks backward from the cursor past identifiers, whitespace, and
/// comma-separated lists to find the preceding keyword.
pub(crate) fn detect_class_name_context(content: &str, position: Position) -> ClassNameContext {
    let chars: Vec<char> = content.chars().collect();
    let Some(offset) = position_to_char_offset(&chars, position) else {
        return ClassNameContext::Any;
    };

    // Walk back past the partial identifier (alphanumeric, _, \).
    let mut i = offset;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '\\') {
        i -= 1;
    }

    // ── Attribute context (`#[…]`) ──────────────────────────────────
    // Before checking keywords, detect whether the cursor is inside a
    // PHP attribute list.  Walk backward from the position (past the
    // partial identifier) looking for `#[`.  Skip over commas and
    // already-typed attribute names/args (e.g. `#[Override, Ov|`).
    if let Some(target) = detect_attribute_context(&chars, i, content, position) {
        return ClassNameContext::Attribute(target);
    }

    // Skip whitespace (including newlines for multi-line declarations).
    while i > 0 && chars[i - 1].is_ascii_whitespace() {
        i -= 1;
    }

    // Handle comma-separated lists (e.g. `implements Foo, Bar, Baz`).
    // Walk past `Identifier,` sequences.
    while i > 0 && chars[i - 1] == ',' {
        i -= 1; // skip comma
        // Skip whitespace.
        while i > 0 && chars[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        // Skip identifier (including backslashes for FQNs).
        while i > 0
            && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '\\')
        {
            i -= 1;
        }
        // Skip whitespace.
        while i > 0 && chars[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
    }

    // Now `i` points just past the keyword (if any). Check which keyword
    // precedes us.
    if keyword_ends_at(&chars, i, "instanceof") {
        return ClassNameContext::Instanceof;
    }
    if keyword_ends_at(&chars, i, "new") {
        // Check if `throw` precedes `new` → ThrowNew context.
        let new_start = i - "new".len();
        let mut j = new_start;
        while j > 0 && chars[j - 1].is_ascii_whitespace() {
            j -= 1;
        }
        if keyword_ends_at(&chars, j, "throw") {
            return ClassNameContext::ThrowNew;
        }
        return ClassNameContext::New;
    }
    if keyword_ends_at(&chars, i, "implements") {
        return ClassNameContext::Implements;
    }
    if keyword_ends_at(&chars, i, "extends") {
        let extends_start = i - "extends".len();
        return determine_extends_context(&chars, extends_start);
    }

    // `use function` and `use const` (two-word keywords).
    // Check for `function` / `const` first, then walk back to `use`.
    if keyword_ends_at(&chars, i, "function") {
        let kw_start = i - "function".len();
        let mut j = kw_start;
        while j > 0 && chars[j - 1].is_ascii_whitespace() {
            j -= 1;
        }
        if keyword_ends_at(&chars, j, "use") && brace_depth_at(&chars, j) < 1 {
            return ClassNameContext::UseFunction;
        }
    }
    if keyword_ends_at(&chars, i, "const") {
        let kw_start = i - "const".len();
        let mut j = kw_start;
        while j > 0 && chars[j - 1].is_ascii_whitespace() {
            j -= 1;
        }
        if keyword_ends_at(&chars, j, "use") && brace_depth_at(&chars, j) < 1 {
            return ClassNameContext::UseConst;
        }
    }

    if keyword_ends_at(&chars, i, "use") {
        // Distinguish trait `use` (inside class body, brace depth >= 1)
        // from namespace `use` (top level, brace depth 0).
        if brace_depth_at(&chars, i) >= 1 {
            return ClassNameContext::TraitUse;
        }
        return ClassNameContext::UseImport;
    }

    if keyword_ends_at(&chars, i, "namespace") && brace_depth_at(&chars, i) < 1 {
        return ClassNameContext::NamespaceDeclaration;
    }

    ClassNameContext::Any
}

/// Detect whether the cursor is positioned inside a class/interface/trait/enum
/// declaration name.
///
/// Returns `true` when the user is typing the name of a new class-like
/// declaration, e.g. `class F|`, `abstract class F|`, `interface F|`,
/// `trait F|`, `enum F|`, `final readonly class F|`, etc.
///
/// Returns `false` for anonymous classes (`new class {}`).
pub(crate) fn is_class_declaration_name(content: &str, position: Position) -> bool {
    let chars: Vec<char> = content.chars().collect();
    let Some(offset) = position_to_char_offset(&chars, position) else {
        return false;
    };

    // Walk back past the partial identifier (alphanumeric, _).
    // Declaration names never contain `\`.
    let mut i = offset;
    while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        i -= 1;
    }

    // Skip whitespace.
    while i > 0 && chars[i - 1].is_ascii_whitespace() {
        i -= 1;
    }

    // Check for declaration keywords.
    let is_decl = keyword_ends_at(&chars, i, "class")
        || keyword_ends_at(&chars, i, "interface")
        || keyword_ends_at(&chars, i, "trait")
        || keyword_ends_at(&chars, i, "enum");

    if !is_decl {
        return false;
    }

    // For `class`, ensure this is not `new class` (anonymous class).
    if keyword_ends_at(&chars, i, "class") {
        let kw_start = i - "class".len();
        let mut j = kw_start;
        while j > 0 && chars[j - 1].is_ascii_whitespace() {
            j -= 1;
        }
        if keyword_ends_at(&chars, j, "new") {
            return false;
        }
    }

    true
}

/// Detect the class-like kind from raw PHP stub source without
/// full parsing.
///
/// Looks for a declaration line like `class Foo`, `interface Bar`,
/// `trait Baz`, or `enum Qux` and returns the kind along with
/// `is_abstract` and `is_final` flags.
#[cfg(test)]
pub(crate) fn detect_stub_class_kind(
    class_name: &str,
    source: &str,
) -> Option<(ClassLikeKind, bool, bool)> {
    let sn = short_name(class_name);
    // Quick rejection: the short name must appear somewhere in the
    // source (a necessary condition for a declaration line).
    if !source.contains(sn) {
        return None;
    }

    for line in source.lines() {
        let trimmed = line.trim();
        // Skip comments and blank lines.
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with('*')
            || trimmed.starts_with("/*")
        {
            continue;
        }

        // We're looking for `<modifiers> class|interface|trait|enum ShortName`.
        // Split by whitespace and find the keyword + name pair.
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        for (idx, token) in tokens.iter().enumerate() {
            let kind = match token.to_lowercase().as_str() {
                "class" => Some(ClassLikeKind::Class),
                "interface" => Some(ClassLikeKind::Interface),
                "trait" => Some(ClassLikeKind::Trait),
                "enum" => Some(ClassLikeKind::Enum),
                _ => None,
            };
            if let Some(kind) = kind {
                // The token after the keyword should be the class name
                // (possibly followed by `{`, `extends`, etc.).
                if let Some(name_token) = tokens.get(idx + 1) {
                    let name = name_token.trim_end_matches(['{', ':']);
                    if name == sn {
                        let prefix = &tokens[..idx];
                        let is_abstract = prefix.iter().any(|t| t.eq_ignore_ascii_case("abstract"));
                        let is_final = prefix.iter().any(|t| t.eq_ignore_ascii_case("final"));
                        return Some((kind, is_abstract, is_final));
                    }
                }
            }
        }
    }

    None
}
