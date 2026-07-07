/// Class name completions.
///
/// This module handles building completion items for class, interface,
/// trait, and enum names when no member-access operator (`->` or `::`)
/// is present.
///
/// Also provides a Throwable-filtered variant for catch clause fallback
/// and `throw new` completion, which only suggests exception classes
/// from already-parsed sources and includes everything else (class index,
/// stubs) unfiltered.
///
/// Constant, function, and namespace completions live in their own
/// sibling modules (`constant_completion`, `function_completion`,
/// `namespace_completion`).
use std::collections::{HashMap, HashSet};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::completion::named_args::position_to_char_offset;
use crate::completion::use_edit;
use crate::types::*;
use crate::util::{short_name, strip_fqn_prefix};

use crate::completion::builder::{
    analyze_use_block, build_callable_snippet, build_use_edit, use_import_conflicts,
};

/// The syntactic context in which a class name is being completed.
///
/// Different PHP positions accept only certain kinds of class-like
/// declarations. For example, `extends` in a class declaration only
/// accepts non-final classes, while `implements` only accepts interfaces.
/// This enum lets `build_class_name_completions` filter out invalid
/// suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassNameContext {
    /// No specific context — any class-like is valid.
    Any,
    /// After `new` keyword — only concrete (non-abstract) classes.
    New,
    /// After `throw new` — only concrete (non-abstract) classes that
    /// implement `Throwable`.
    ThrowNew,
    /// Inside a `catch()` clause or `@throws` docblock tag — any
    /// class or interface that implements `Throwable`.
    Catch,
    /// After `extends` in a class declaration — only non-final,
    /// non-abstract-only classes.
    ExtendsClass,
    /// After `extends` in an interface declaration — only interfaces.
    ExtendsInterface,
    /// After `implements` — only interfaces.
    Implements,
    /// After `use` inside a class body — only traits.
    TraitUse,
    /// After `instanceof` — classes, interfaces, enums (not traits).
    Instanceof,
    /// In a type-hint position (parameter type, return type, property
    /// type) or PHPDoc type reference (`@param`, `@return`, `@var`).
    /// Accepts classes, interfaces, and enums but rejects traits.
    TypeHint,
    /// After `use` at the top level — any class (namespace import).
    /// Treated specially: FQN is always inserted, no `use` text-edit.
    UseImport,
    /// After `use function` — only functions (handled elsewhere).
    UseFunction,
    /// After `use const` — only constants (handled elsewhere).
    UseConst,
    /// After `namespace` keyword at the top level — namespace names
    /// (handled by `namespace_completion`).
    NamespaceDeclaration,
    /// Inside a PHP attribute list (`#[…]`).
    ///
    /// Only classes decorated with `#[\Attribute]` are valid here.
    /// The inner `u8` is the attribute target bitmask for the
    /// declaration the attribute is applied to (e.g.
    /// [`attribute_target::TARGET_CLASS`] when writing `#[…]` above a
    /// class).  Loaded classes are hard-filtered by target; unloaded
    /// classes are demoted when their short name does not contain
    /// "Attribute".
    Attribute(u8),
}

impl ClassNameContext {
    /// Check whether a `ClassInfo` matches this context.
    pub(crate) fn matches(&self, cls: &ClassInfo) -> bool {
        match self {
            Self::Any => true,
            Self::New => cls.kind == ClassLikeKind::Class && !cls.is_abstract,
            Self::ThrowNew => cls.kind == ClassLikeKind::Class && !cls.is_abstract,
            Self::Catch => matches!(cls.kind, ClassLikeKind::Class | ClassLikeKind::Interface),
            Self::ExtendsClass => cls.kind == ClassLikeKind::Class && !cls.is_final,
            Self::ExtendsInterface => cls.kind == ClassLikeKind::Interface,
            Self::Implements => cls.kind == ClassLikeKind::Interface,
            Self::TraitUse => cls.kind == ClassLikeKind::Trait,
            Self::Instanceof | Self::TypeHint => cls.kind != ClassLikeKind::Trait,
            Self::UseImport => true,
            Self::UseFunction | Self::UseConst | Self::NamespaceDeclaration => false,
            Self::Attribute(target) => {
                cls.attribute_targets != 0 && (cls.attribute_targets & target) != 0
            }
        }
    }

    /// Check whether a class-like matches this context given only kind
    /// flags (used by tests for `detect_stub_class_kind`).
    #[cfg(test)]
    pub(crate) fn matches_kind_flags(
        &self,
        kind: ClassLikeKind,
        is_abstract: bool,
        is_final: bool,
    ) -> bool {
        match self {
            Self::Any | Self::UseImport => true,
            Self::New => kind == ClassLikeKind::Class && !is_abstract,
            Self::ThrowNew => kind == ClassLikeKind::Class && !is_abstract,
            Self::Catch => matches!(kind, ClassLikeKind::Class | ClassLikeKind::Interface),
            Self::ExtendsClass => kind == ClassLikeKind::Class && !is_final,
            Self::ExtendsInterface => kind == ClassLikeKind::Interface,
            Self::Implements => kind == ClassLikeKind::Interface,
            Self::TraitUse => kind == ClassLikeKind::Trait,
            Self::Instanceof | Self::TypeHint => kind != ClassLikeKind::Trait,
            Self::UseFunction | Self::UseConst | Self::NamespaceDeclaration => false,
            // Cannot verify attribute status from kind flags alone.
            Self::Attribute(_) => true,
        }
    }

    /// Whether only class-like names are valid in this context (as
    /// opposed to constants and functions).
    pub(crate) fn is_class_only(&self) -> bool {
        matches!(
            self,
            Self::New
                | Self::ThrowNew
                | Self::Catch
                | Self::ExtendsClass
                | Self::ExtendsInterface
                | Self::Implements
                | Self::TraitUse
                | Self::Instanceof
                | Self::TypeHint
                | Self::UseImport
                | Self::Attribute(_)
        )
    }

    /// Whether this context is `New`.
    pub(crate) fn is_new(&self) -> bool {
        matches!(self, Self::New | Self::ThrowNew)
    }

    /// Whether this context is inside an attribute list (`#[…]`).
    pub(crate) fn is_attribute(&self) -> bool {
        matches!(self, Self::Attribute(_))
    }

    /// Whether this context requires a very specific class-like kind
    /// (trait, interface, etc.) and should reject unverifiable entries.
    pub(crate) fn is_narrow_kind(&self) -> bool {
        matches!(
            self,
            Self::TraitUse | Self::Implements | Self::ExtendsInterface
        )
    }

    /// Heuristic: names that are unlikely to match this context.
    ///
    /// Used to demote (but not exclude) unloaded classes whose actual
    /// kind or attribute status cannot be verified.  For example,
    /// `new AbstractFoo` is very likely wrong, and `#[UserService]`
    /// is unlikely to be an attribute class.
    ///
    /// This should only be called for classes that could NOT be loaded
    /// (i.e. `matches_context_or_unloaded` returned `true` because the
    /// class was unloaded, not because it passed `matches()`).
    pub(crate) fn likely_mismatch(&self, short_name: &str) -> bool {
        match self {
            Self::New => likely_non_instantiable(short_name),
            Self::ThrowNew => {
                // Demote if it doesn't look instantiable OR doesn't look
                // like an exception class.
                likely_non_instantiable(short_name)
                    || (!short_name.ends_with("Exception")
                        && !short_name.ends_with("Error")
                        && !short_name.ends_with("Throwable"))
            }
            Self::Catch => {
                !short_name.ends_with("Exception")
                    && !short_name.ends_with("Error")
                    && !short_name.ends_with("Throwable")
            }
            Self::ExtendsClass => likely_interface_name(short_name),
            Self::Implements | Self::ExtendsInterface => likely_non_interface_name(short_name),
            Self::TraitUse => likely_non_instantiable(short_name),
            Self::Attribute(_) => !likely_attribute_name(short_name),
            _ => false,
        }
    }
}

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

/// Heuristic: the short name looks like it could be an attribute class.
///
/// Returns `true` when the name contains "Attribute" as a substring
/// (case-insensitive), or is one of the well-known built-in attributes
/// (`Override`, `Deprecated`, `SensitiveParameter`, etc.).
fn likely_attribute_name(short_name: &str) -> bool {
    let lower = short_name.to_lowercase();
    if lower.contains("attribute") {
        return true;
    }
    // Well-known PHP built-in attributes that don't have "Attribute"
    // in their name.
    matches!(
        short_name,
        "Override"
            | "Deprecated"
            | "SensitiveParameter"
            | "AllowDynamicProperties"
            | "ReturnTypeWillChange"
    )
}

/// Heuristic: names that look like interfaces (`IFoo`, `FooInterface`).
fn likely_interface_name(name: &str) -> bool {
    if name.starts_with('I') && name.len() > 1 {
        let second = name.chars().nth(1).unwrap();
        if second.is_uppercase() {
            return true;
        }
    }
    if name.ends_with("Interface") {
        return true;
    }
    false
}

/// Heuristic: names that positively look like non-interface types.
///
/// Used to demote unlikely interface candidates in `Implements` and
/// `ExtendsInterface` contexts. Only returns `true` when the name
/// matches a known non-interface naming pattern (Abstract*, *Abstract,
/// Base[A-Z]*). Names that don't match any pattern are left alone
/// (returns `false`).
fn likely_non_interface_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("abstract") || lower.ends_with("abstract") {
        return true;
    }
    // `Base[A-Z]` prefix — e.g. `BaseController`, `BaseModel`.
    if name.starts_with("Base") && name.len() >= 5 {
        let fifth = name.as_bytes()[4];
        if fifth.is_ascii_uppercase() {
            return true;
        }
    }
    false
}

/// Heuristic: names that look like they cannot be instantiated.
///
/// Combines interface-like names, abstract-like names, and trait-like
/// names. Used to demote (but not exclude) class index/stub items in
/// `new` context.
fn likely_non_instantiable(name: &str) -> bool {
    if likely_interface_name(name) {
        return true;
    }
    if name.starts_with("Abstract") {
        return true;
    }
    // `Base[A-Z]` prefix — e.g. `BaseController`, `BaseModel`.
    // `Baseline`, `Based`, etc. are NOT matched (5th char is lowercase).
    if name.starts_with("Base") && name.len() >= 5 {
        let fifth = name.as_bytes()[4];
        if fifth.is_ascii_uppercase() {
            return true;
        }
    }
    if name.ends_with("Abstract") || name.ends_with("Trait") {
        return true;
    }
    false
}

/// Whether a class name represents an anonymous class.
/// Detect whether the cursor is inside a PHP attribute list (`#[…]`).
///
/// `j` is the char offset just before the partial identifier the user is
/// typing.  We walk backward through whitespace, commas, nested parens,
/// and prior attribute names looking for the opening `#[`.
///
/// Returns `Some(target_flags)` when confirmed, or `None` when the
/// cursor is not inside an attribute list.
fn detect_attribute_context(
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

/// Expand a namespace alias at the start of a typed prefix.
///
/// Given a prefix like `OA\Re` and a use-map containing
/// `OA → OpenApi\Attributes`, returns `Some("OpenApi\Attributes\Re")`.
/// Returns `None` when the first segment of the prefix is not a known
/// alias or the prefix contains no backslash.
pub(in crate::completion) fn expand_alias_prefix(
    normalized_prefix: &str,
    use_map: &HashMap<String, String>,
) -> Option<String> {
    let bs = normalized_prefix.find('\\')?;
    let first_segment = &normalized_prefix[..bs];
    let rest = &normalized_prefix[bs + 1..]; // may be empty
    let fqn_ns = use_map.get(first_segment)?;
    if rest.is_empty() {
        Some(format!("{}\\", fqn_ns))
    } else {
        Some(format!("{}\\{}", fqn_ns, rest))
    }
}

/// Check whether a class matches the typed prefix.
///
/// In FQN-prefix mode (`is_fqn` is `true`) both the short name and the
/// fully-qualified name are checked so that `App\Models\U` can surface
/// `App\Models\User`.  When `expanded_prefix_lower` is provided (the
/// alias-expanded form of the prefix, e.g. `openapi\attributes\re` for
/// a typed `OA\Re`), it is also checked against the FQN so that
/// classes under an aliased namespace are matched.
///
/// In non-FQN mode only the short name is checked to avoid flooding the
/// response with every class under a broad namespace prefix.
pub(in crate::completion) fn matches_class_prefix(
    short_name: &str,
    fqn: &str,
    prefix_lower: &str,
    is_fqn: bool,
    expanded_prefix_lower: Option<&str>,
) -> bool {
    if is_fqn {
        short_name.to_lowercase().contains(prefix_lower)
            || fqn.to_lowercase().contains(prefix_lower)
            || expanded_prefix_lower.is_some_and(|exp| fqn.to_lowercase().contains(exp))
    } else {
        short_name.to_lowercase().contains(prefix_lower)
    }
}

/// Try to shorten a FQN using the file's use-map.
///
/// Checks whether any existing `use` import provides a prefix (or exact
/// match) for the given FQN.  Returns the shortest reference that is
/// valid given the imports, or `None` if no shortening is possible.
///
/// Examples (given `use Cassandra\Exception;`):
///   - `Cassandra\Exception\AlreadyExistsException` → `Exception\AlreadyExistsException`
///   - `Cassandra\Exception` → `Exception`
fn shorten_fqn_via_use_map(fqn: &str, use_map: &HashMap<String, String>) -> Option<String> {
    let mut best: Option<String> = None;
    for (alias, import_fqn) in use_map {
        let shortened = if fqn == import_fqn {
            // Exact match: the full FQN is directly imported.
            Some(alias.clone())
        } else {
            // Prefix match: a parent namespace is imported.
            fqn.strip_prefix(&format!("{}\\", import_fqn))
                .map(|suffix| format!("{}\\{}", alias, suffix))
        };
        if let Some(ref s) = shortened
            && best.as_ref().is_none_or(|b| s.len() < b.len())
        {
            best = shortened;
        }
    }
    best
}

/// Build a table mapping namespace prefixes to their occurrence count.
///
/// The table is derived from the file's `use` imports and its own
/// namespace declaration.  Each FQN's namespace portion is exploded
/// into all ancestor prefixes, and the count records how many sources
/// (imports + file namespace) contribute to each prefix.
///
/// Used once per completion request to score candidates by namespace
/// affinity.
pub(crate) fn build_affinity_table(
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> HashMap<String, u32> {
    let mut table: HashMap<String, u32> = HashMap::new();

    let mut namespaces: Vec<&str> = Vec::new();

    if let Some(ns) = file_namespace {
        namespaces.push(ns.as_str());
    }

    for fqn in use_map.values() {
        if let Some(pos) = fqn.rfind('\\') {
            namespaces.push(&fqn[..pos]);
        }
    }

    for ns in namespaces {
        let parts: Vec<&str> = ns.split('\\').collect();
        for depth in 1..=parts.len() {
            let prefix = parts[..depth].join("\\");
            *table.entry(prefix).or_insert(0) += 1;
        }
    }

    table
}

/// Score a candidate FQN against the affinity table.
///
/// Extracts the candidate's namespace, explodes it into ancestor
/// prefixes, and sums the counts from the table for every matching
/// prefix.  Higher scores indicate the candidate lives in a namespace
/// that is heavily used by the current file.
pub(crate) fn affinity_score(fqn: &str, table: &HashMap<String, u32>) -> u32 {
    let ns = match fqn.rfind('\\') {
        Some(pos) => &fqn[..pos],
        None => return 0,
    };
    let parts: Vec<&str> = ns.split('\\').collect();
    let mut score = 0u32;
    for depth in 1..=parts.len() {
        let prefix = parts[..depth].join("\\");
        if let Some(&count) = table.get(&prefix) {
            score += count;
        }
    }
    score
}

/// Classify how well a short name matches the typed prefix.
///
/// Returns `'a'` for exact match, `'b'` for starts-with (or empty
/// prefix), `'c'` for substring-only.  The character is used as the
/// highest-weight dimension of the sort_text so exact matches always
/// appear before prefix matches, which always appear before substring
/// matches.
pub(in crate::completion) fn match_quality(short_name: &str, prefix: &str) -> char {
    if prefix.is_empty() {
        return 'b';
    }
    let sn = short_name.to_lowercase();
    let p = prefix.to_lowercase();
    if sn == p {
        'a'
    } else if sn.starts_with(&p) {
        'b'
    } else {
        'c'
    }
}

/// Assemble a sort_text string for a class name completion item.
///
/// The format is `{match_quality}{source_tier}{affinity}{demote}{gap}_{short_name_lower}`
/// where:
/// - `match_quality`: `'a'` exact, `'b'` starts-with, `'c'` contains
/// - `source_tier`: `'0'` use-imported, `'1'` same-namespace, `'2'` everything else
/// - `affinity`: 4-digit inverted score (`9999 - score`, so higher scores sort first)
/// - `demote`: `'0'` normal, `'1'` heuristically demoted
/// - `gap`: 3-digit distance between short name length and prefix length
///   (`short_name.len() - prefix.len()`).  Within the same affinity and
///   demotion group, names closer in length to what the user typed sort
///   first.  This smooths the visual transition as a prefix match
///   narrows toward an exact match (e.g. typing "Pro" ranks `Product`
///   above `ProductFilterTerm` when both share the same affinity).
pub(in crate::completion) fn class_sort_text(
    short_name: &str,
    fqn: &str,
    prefix: &str,
    source_tier: char,
    demoted: bool,
    affinity_table: &HashMap<String, u32>,
) -> String {
    let quality = match_quality(short_name, prefix);
    let score = affinity_score(fqn, affinity_table);
    let affinity = format!("{:04}", 9999_u32.saturating_sub(score.min(9999)));
    let gap = format!(
        "{:03}",
        short_name.len().saturating_sub(prefix.len()).min(999)
    );
    let demote = if demoted { '1' } else { '0' };
    format!(
        "{}{}{}{}{}_{}",
        quality,
        source_tier,
        affinity,
        demote,
        gap,
        short_name.to_lowercase()
    )
}

/// Compute the insert-text base, filter-text, and optional use-import
/// FQN for a class completion item.
///
/// This function handles **editing concerns only** (what text to insert
/// and whether a `use` statement is needed).  The completion item's
/// visual presentation (label, label_details, filter_text) is set by
/// [`ClassItemCtx::build_item`], the single authority for how items
/// appear in the editor popup.
///
/// In FQN-prefix mode the insert text is the namespace-qualified
/// reference.  When the FQN belongs to the current namespace the
/// reference is simplified to a relative name (e.g. typing `\Demo\` in
/// namespace `Demo` for class `Demo\Box` inserts just `Box`).
///
/// In non-FQN mode the short name is inserted with a `use` import.
///
/// Returns `(insert_base, filter_text, use_import_fqn)`.
/// `use_import_fqn` is `None` when no `use` statement is needed (FQN
/// mode or same-namespace class).
pub(in crate::completion) fn class_edit_texts(
    short_name: &str,
    fqn: &str,
    is_fqn: bool,
    has_leading_backslash: bool,
    file_namespace: &Option<String>,
) -> (String, String, Option<String>) {
    if is_fqn {
        // When the FQN belongs to the current namespace, simplify to a
        // relative reference so that `\Demo\` + `Demo\Box` → `Box`.
        if let Some(ns) = file_namespace {
            let ns_prefix = format!("{}\\", ns);
            if let Some(relative) = fqn.strip_prefix(&ns_prefix) {
                // Filter text keeps the full typed form so the editor's
                // fuzzy matcher still finds the item.
                let filter = if has_leading_backslash {
                    format!("\\{}", fqn)
                } else {
                    fqn.to_string()
                };
                return (relative.to_string(), filter, None);
            }
        }

        let insert = if has_leading_backslash {
            format!("\\{}", fqn)
        } else {
            fqn.to_string()
        };
        (insert.clone(), insert, None)
    } else {
        // Non-FQN mode: insert the short name and import the full FQN.
        // Use the short name as filter_text so the editor's fuzzy
        // matcher scores candidates by short-name relevance, not by
        // accidental substring hits inside the namespace path.  The
        // FQN is still visible in `label` and `detail`.
        let filter = short_name.to_string();
        (short_name.to_string(), filter, Some(fqn.to_string()))
    }
}

/// Shared context for building class completion items.
///
/// Bundles the parameters that are constant across all class sources
/// within a single completion request, so that `apply_import_fixups`
/// and `build_item` don't need seven-plus arguments each.
pub(in crate::completion) struct ClassItemCtx<'a> {
    pub(in crate::completion) is_fqn_prefix: bool,
    pub(in crate::completion) is_new: bool,
    /// Whether this completion is inside an attribute list (`#[…]`).
    ///
    /// When true, `build_item` generates constructor snippets with
    /// named arguments and smart default placeholders instead of
    /// inserting the bare class name.
    pub(in crate::completion) is_attribute: bool,
    pub(in crate::completion) fqn_replace_range: Option<Range>,
    pub(in crate::completion) file_use_map: &'a HashMap<String, String>,
    pub(in crate::completion) use_block: use_edit::UseBlockInfo,
    pub(in crate::completion) file_namespace: &'a Option<String>,
    /// Namespace prefix → occurrence count table used for affinity scoring.
    pub(in crate::completion) affinity_table: HashMap<String, u32>,
    /// The short-name portion of the typed prefix, used for match quality
    /// classification (e.g. `"Order"` from `"Illuminate\\Database\\Order"`).
    pub(in crate::completion) quality_prefix: String,
    /// Whether the typed prefix contains a `\` (after stripping a leading
    /// `\`).  When false in FQN mode (e.g. `use Order` rather than
    /// `use Illuminate\Order`), `build_item` uses the short name as
    /// `filter_text` so the editor's fuzzy scorer matches against class
    /// names rather than full namespace paths.
    pub(in crate::completion) prefix_has_namespace: bool,
    /// The file URI where the completion was triggered, stored in the
    /// `data` field for `completionItem/resolve`.
    pub(in crate::completion) uri: &'a str,
}

/// Parameters for [`Backend::build_class_name_completions`].
///
/// Groups the arguments that the three call sites assemble differently
/// (especially `affinity_table_override` for `UseImport` context).
pub(crate) struct ClassCompletionParams<'a> {
    pub(crate) file_use_map: &'a HashMap<String, String>,
    pub(crate) file_namespace: &'a Option<String>,
    pub(crate) prefix: &'a str,
    pub(crate) content: &'a str,
    pub(crate) context: ClassNameContext,
    pub(crate) position: Position,
    /// When `Some`, used for namespace affinity scoring instead of
    /// building a table from `file_use_map`.  Needed for `UseImport`
    /// context where the caller passes an empty use-map (to suppress
    /// bogus source-1 entries) but still wants affinity from the real
    /// imports.
    pub(crate) affinity_table_override: Option<HashMap<String, u32>>,
    /// The file URI where the completion was triggered, used for
    /// `completionItem/resolve` data.
    pub(crate) uri: &'a str,
}

/// Per-item editing fields produced by `class_edit_texts` and
/// post-processed by `apply_import_fixups`.
///
/// These fields control *what gets inserted* when the user accepts the
/// completion.  The label and filter_text (what the user *sees* and
/// what the editor matches against) are set by
/// [`ClassItemCtx::build_item`].
pub(in crate::completion) struct ClassItemTexts {
    pub(in crate::completion) base_name: String,
    pub(in crate::completion) filter: String,
    pub(in crate::completion) use_import: Option<String>,
}

impl ClassItemCtx<'_> {
    /// Fix up `base_name` and `use_import` after `class_edit_texts`
    /// to handle import conflicts and FQN alias collisions.
    ///
    /// - If the short name conflicts with an existing import, falls back
    ///   to a fully-qualified reference (prepends `\`).
    /// - In FQN mode, if the first namespace segment matches an existing
    ///   alias (and the name wasn't intentionally shortened through that
    ///   alias), prepends `\` so PHP resolves from the global namespace.
    pub(in crate::completion) fn apply_import_fixups(
        &self,
        base_name: &mut String,
        use_import: &mut Option<String>,
        was_shortened: bool,
    ) {
        if let Some(ref import_fqn) = *use_import
            && use_import_conflicts(import_fqn, self.file_use_map)
        {
            *base_name = format!("\\{}", import_fqn);
            *use_import = None;
        }
        if self.is_fqn_prefix
            && !was_shortened
            && !base_name.starts_with('\\')
            && let Some(first_seg) = base_name.split('\\').next()
            && self
                .file_use_map
                .keys()
                .any(|a| a.eq_ignore_ascii_case(first_seg))
        {
            *base_name = format!("\\{}", base_name);
        }
    }

    /// Build a `CompletionItem` for a class name completion.
    ///
    /// This is the **single authority** for the visual presentation of
    /// class name completions.
    ///
    /// When the typed prefix contains no namespace separator (e.g.
    /// `Order`, `Exc`), the label is the **short name** and the
    /// namespace is shown via `label_details.description`, giving a
    /// clean two-column layout:
    ///
    /// ```text
    ///   Order            App\Models
    ///   Order            Luxplus\Database\Model\Orders
    /// ```
    ///
    /// The `filter_text` is also the short name so the editor's fuzzy
    /// scorer matches against class names, not accidental substrings
    /// inside namespace paths.
    ///
    /// When the prefix is namespace-qualified (e.g. `Illuminate\D`,
    /// `\App\Models\U`), the label and filter_text remain the full
    /// FQN so namespace drilling works as expected.
    ///
    /// The sort_text is computed internally from `source_tier` and
    /// `demoted` using the affinity table and quality prefix stored
    /// in `ClassItemCtx`.
    pub(in crate::completion) fn build_item(
        &self,
        texts: ClassItemTexts,
        fqn: &str,
        source_tier: char,
        demoted: bool,
        ctor_params: Option<&[ParameterInfo]>,
        is_deprecated: bool,
    ) -> CompletionItem {
        let short_name = crate::util::short_name(fqn);
        let sort_text = class_sort_text(
            short_name,
            fqn,
            &self.quality_prefix,
            source_tier,
            demoted,
            &self.affinity_table,
        );
        let (insert_text, insert_text_format) = if self.is_attribute {
            let snippet = crate::completion::builder::build_attribute_snippet(
                &texts.base_name,
                ctor_params.unwrap_or(&[]),
            );
            if snippet.contains("$0") {
                (snippet, Some(InsertTextFormat::SNIPPET))
            } else {
                // No constructor params — bare name, no snippet syntax.
                (snippet, None)
            }
        } else if self.is_new {
            Backend::build_new_insert(&texts.base_name, ctor_params)
        } else {
            (texts.base_name, None)
        };
        // When the typed prefix is a simple name (no `\`), use the
        // short name as filter_text so the editor's fuzzy scorer
        // ranks candidates by short-name relevance.  With the FQN as
        // filter_text, the editor finds accidental substring hits
        // inside namespace paths (e.g. "Order" matching inside
        // "Mockery\HigherOrderMessage") and may promote them above
        // genuine prefix matches.  In FQN mode with a namespace-
        // qualified prefix (e.g. `use Illuminate\D`), keep the
        // original filter_text so namespace drilling works.
        //
        // In the same non-FQN case, set the label to the short name
        // and show the namespace in `label_details.description`.
        // This gives a clean two-column layout in the editor popup:
        //
        //   Order            App\Models
        //   Order            Luxplus\Database\Model\Orders
        //
        // so users can distinguish same-named classes without the
        // label being a long FQN that the editor truncates.
        let (label, filter_text, label_details) = if self.prefix_has_namespace {
            (fqn.to_string(), texts.filter, None)
        } else {
            let ns = fqn.rsplit_once('\\').map(|(ns, _)| ns.to_string());
            (
                short_name.to_string(),
                short_name.to_string(),
                ns.map(|desc| CompletionItemLabelDetails {
                    detail: None,
                    description: Some(desc),
                }),
            )
        };
        let data = serde_json::to_value(crate::completion::resolve::CompletionItemData {
            class_name: String::new(),
            member_name: fqn.to_string(),
            kind: "class".to_string(),
            uri: self.uri.to_string(),
            extra_class_names: vec![],
        })
        .ok();
        CompletionItem {
            label,
            label_details,
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(fqn.to_string()),
            insert_text: Some(insert_text.clone()),
            insert_text_format,
            filter_text: Some(filter_text),
            sort_text: Some(sort_text),
            tags: if is_deprecated {
                Some(vec![CompletionItemTag::DEPRECATED])
            } else {
                None
            },
            text_edit: self.fqn_replace_range.map(|range| {
                CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: insert_text.clone(),
                })
            }),
            additional_text_edits: texts.use_import.as_ref().and_then(|import_fqn| {
                build_use_edit(import_fqn, &self.use_block, self.file_namespace)
            }),
            data,
            ..CompletionItem::default()
        }
    }
}

impl Backend {
    /// Extract the partial identifier (class name fragment) that the user
    /// is currently typing at the given cursor position.
    ///
    /// Walks backward from the cursor through alphanumeric characters,
    /// underscores, and backslashes (namespace separators).  Returns
    /// `None` if the resulting text starts with `$` (variable context)
    /// or is empty.
    pub fn extract_partial_class_name(content: &str, position: Position) -> Option<String> {
        let lines: Vec<&str> = content.lines().collect();
        if position.line as usize >= lines.len() {
            return None;
        }

        let line = lines[position.line as usize];
        let chars: Vec<char> = line.chars().collect();
        let col = (position.character as usize).min(chars.len());

        // Walk backwards through identifier characters (including `\`)
        let mut i = col;
        while i > 0
            && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '\\')
        {
            i -= 1;
        }

        if i == col {
            // Nothing typed — no partial identifier
            return None;
        }

        // If preceded by `$`, this is a variable, not a class name
        if i > 0 && chars[i - 1] == '$' {
            return None;
        }

        // If preceded by `->` or `::`, member completion handles this
        if i >= 2 && chars[i - 2] == '-' && chars[i - 1] == '>' {
            return None;
        }
        if i >= 2 && chars[i - 2] == ':' && chars[i - 1] == ':' {
            return None;
        }

        // If preceded by `<?`, this is the PHP open tag (`<?php`), not
        // a class/function name — suppress completion entirely.
        if i >= 2 && chars[i - 2] == '<' && chars[i - 1] == '?' {
            return None;
        }

        let partial: String = chars[i..col].iter().collect();
        if partial.is_empty() {
            return None;
        }

        Some(partial)
    }

    /// Build the insert text (and optional format) for a `new` context
    /// class name completion.
    ///
    /// If constructor parameters are available, generates a callable
    /// snippet; otherwise generates `Name()$0`.
    pub(in crate::completion) fn build_new_insert(
        name: &str,
        ctor_params: Option<&[ParameterInfo]>,
    ) -> (String, Option<InsertTextFormat>) {
        if let Some(params) = ctor_params
            && !params.is_empty()
        {
            let snippet = build_callable_snippet(name, params);
            (snippet, Some(InsertTextFormat::SNIPPET))
        } else {
            (format!("{name}()$0"), Some(InsertTextFormat::SNIPPET))
        }
    }

    /// Load the `__construct` parameters for a class, if available.
    ///
    /// Tries `load_stub_class` (which checks `uri_classes_index` first, then
    /// in-memory stubs) to avoid disk I/O.  Returns `None` when the
    /// class cannot be found or has no constructor.
    fn ctor_params_for(&self, class_name: &str) -> Option<Vec<ParameterInfo>> {
        let cls = self.load_stub_class(class_name)?;
        let ctor = cls.get_method_ci("__construct")?;
        Some(ctor.parameters.clone())
    }

    /// Maximum number of class name completions to return.
    ///
    /// After this limit the result is marked `is_incomplete = true` so
    /// the editor re-requests as the user types more characters.
    pub(in crate::completion) const MAX_CLASS_COMPLETIONS: usize = 100;

    /// Build completion items for class, interface, trait, and enum
    /// names.
    ///
    /// Searches five sources in priority order:
    ///   1. File's `use` imports (already imported)
    ///   2. Same-namespace classes (from `uri_classes_index`)
    ///   3. `fqn_uri_index` (discovered / interacted-with classes)
    ///   4. Class index (all autoloaded classes)
    ///   5. Built-in PHP stubs
    ///
    /// Returns `(items, is_incomplete)`.
    pub(crate) fn build_class_name_completions(
        &self,
        params: ClassCompletionParams<'_>,
    ) -> (Vec<CompletionItem>, bool) {
        let ClassCompletionParams {
            file_use_map,
            file_namespace,
            prefix,
            content,
            context,
            position,
            affinity_table_override,
            uri,
        } = params;
        let is_new = context.is_new();
        let is_attribute = context.is_attribute();
        let is_use_import = matches!(context, ClassNameContext::UseImport);
        // In FQN mode (except UseImport), try to shorten references
        // using the file's existing `use` imports.  E.g. if the user
        // has `use Cassandra\Exception;`, typing `Exception\Al` should
        // insert `Exception\AlreadyExistsException` rather than the
        // full FQN.
        let should_shorten_via_imports = !is_use_import;
        let has_leading_backslash = prefix.starts_with('\\');
        let normalized = strip_fqn_prefix(prefix);
        let prefix_lower = normalized.to_lowercase();
        // In UseImport context, always treat the prefix as FQN so that
        // the full qualified name is inserted (not the short name) and
        // no redundant `use` text-edit is generated.
        let is_fqn_prefix = has_leading_backslash || normalized.contains('\\') || is_use_import;

        // When the prefix starts with an alias (e.g. `OA\Re` where
        // `use OpenApi\Attributes as OA`), expand it to the FQN form
        // (`OpenApi\Attributes\Re`) so that `matches_class_prefix` can
        // find classes under the aliased namespace.
        let expanded = expand_alias_prefix(normalized, file_use_map);
        let expanded_lower = expanded.as_deref().map(|s| s.to_lowercase());
        let expanded_prefix_lower = expanded_lower.as_deref();

        // In UseImport context, suppress namespace-relative
        // simplification — `use User;` is wrong even when the cursor
        // file lives in the same namespace as `User`.  Passing `None`
        // makes `class_edit_texts` emit the full FQN.
        let no_namespace: Option<String> = None;
        let effective_namespace = if is_use_import {
            &no_namespace
        } else {
            file_namespace
        };

        // When the user is typing a namespace-qualified reference (e.g.
        // `http\En`, `\App\Models\U`, or `\Demo`), the editor may treat
        // `\` as a word boundary and only replace the text after the
        // last `\`.  Provide an explicit replacement range covering the
        // entire typed prefix so the editor replaces it in full.
        let fqn_replace_range = if is_fqn_prefix {
            Some(Range {
                start: Position {
                    line: position.line,
                    character: position
                        .character
                        .saturating_sub(prefix.chars().count() as u32),
                },
                end: position,
            })
        } else {
            None
        };
        let mut seen_fqns: HashSet<String> = HashSet::new();
        let mut items: Vec<CompletionItem> = Vec::new();

        // Build the affinity table from the file's use-map and namespace,
        // unless the caller provided a pre-built one (e.g. UseImport
        // context where the real use-map differs from file_use_map).
        let affinity_table = affinity_table_override
            .unwrap_or_else(|| build_affinity_table(file_use_map, file_namespace));

        // Extract the short-name portion of the typed prefix for match
        // quality classification.  E.g. `"Order"` from `"App\\Order"`.
        let quality_prefix = match normalized.rfind('\\') {
            Some(pos) => normalized[pos + 1..].to_string(),
            None => normalized.to_string(),
        };

        // Pre-compute the use-block info for alphabetical `use` insertion.
        // In UseImport context, always treat as namespace-qualified so
        // labels remain FQNs (the user is writing a fully-qualified
        // import statement like `use Foo\Bar`).  Also treat a leading
        // backslash (e.g. `\Cassa`) as namespace-qualified so labels
        // stay as FQNs in explicit-global-namespace mode.
        let prefix_has_namespace =
            normalized.contains('\\') || has_leading_backslash || is_use_import;

        let needs_ctor = is_new || is_attribute;
        let ctx = ClassItemCtx {
            is_fqn_prefix,
            is_new,
            is_attribute,
            fqn_replace_range,
            file_use_map,
            use_block: analyze_use_block(content),
            file_namespace: effective_namespace,
            affinity_table,
            quality_prefix,
            prefix_has_namespace,
            uri,
        };

        // Build a reverse lookup: FQN → use-import short name.
        let use_imported_fqns: HashSet<&String> = file_use_map.values().collect();

        // Namespace prefix for same/sub-namespace proximity check.
        let ns_prefix = file_namespace.as_ref().map(|ns| format!("{}\\{}", ns, ""));

        // ── Helper: classify a FQN into source tier and demotion ─────
        //
        // Returns `None` to exclude, or `Some((tier, demoted, deprecated))`.
        //   '0' = use-imported (already in file's use-map)
        //   '1' = same/sub namespace
        //   '2' = everything else
        let classify = |fqn: &str, sn: &str| -> Option<(char, bool, bool)> {
            // Context-aware kind filtering (instantiability, Throwable, etc.)
            if context.is_class_only() {
                match self.check_context_match(fqn, context) {
                    Some(false) => return None,
                    Some(true) => { /* passed */ }
                    None => {
                        // Unloaded class — pass through with heuristic
                        // demotion below.  In narrow contexts, reject
                        // entirely since we can't verify.
                        if context.is_narrow_kind() {
                            return None;
                        }
                    }
                }
            }

            // Determine tier by proximity.
            let tier = if use_imported_fqns.contains(&fqn.to_string()) {
                '0'
            } else {
                let in_same_or_sub_ns = match &ns_prefix {
                    Some(pfx) => fqn.starts_with(pfx.as_str()),
                    None => !fqn.contains('\\'),
                };
                if in_same_or_sub_ns { '1' } else { '2' }
            };

            // Demotion: only for unloaded classes where context-match
            // was unknown.
            let demoted = context.is_class_only()
                && self.check_context_match(fqn, context).is_none()
                && context.likely_mismatch(sn);

            // Deprecation flag from ClassInfo if available.
            let deprecated = self
                .find_class_in_uri_classes_index(fqn)
                .as_ref()
                .is_some_and(|c| c.deprecation_message.is_some());

            Some((tier, demoted, deprecated))
        };

        // ── Pass 1: fqn_uri_index (project + vendor classes) ────────
        // Collect keys first to avoid holding the read lock while
        // calling classify/is_likely_namespace_not_class (which
        // re-acquire the same lock — parking_lot RwLock is not
        // reentrant).
        let fqn_keys: Vec<String> = self
            .fqn_uri_index
            .read()
            .keys()
            .map(str::to_owned)
            .collect();
        for fqn in &fqn_keys {
            let sn = short_name(fqn);
            if !matches_class_prefix(sn, fqn, &prefix_lower, is_fqn_prefix, expanded_prefix_lower) {
                continue;
            }
            if !seen_fqns.insert(fqn.clone()) {
                continue;
            }
            let Some((tier, demoted, deprecated)) = classify(fqn, sn) else {
                continue;
            };
            // Skip namespace aliases.
            if self.is_likely_namespace_not_class(fqn) {
                continue;
            }
            let (mut base_name, filter, mut use_import) = class_edit_texts(
                sn,
                fqn,
                is_fqn_prefix,
                has_leading_backslash,
                effective_namespace,
            );
            let mut was_shortened = false;
            if should_shorten_via_imports
                && let Some(shortened) = shorten_fqn_via_use_map(fqn, file_use_map)
            {
                base_name = shortened;
                use_import = None;
                was_shortened = true;
            }
            // Tier 0/1 never need a use-import.
            if tier <= '1' {
                use_import = None;
            }
            let mut texts = ClassItemTexts {
                base_name,
                filter,
                use_import,
            };
            ctx.apply_import_fixups(&mut texts.base_name, &mut texts.use_import, was_shortened);
            let ctor = if needs_ctor {
                self.ctor_params_for(fqn)
            } else {
                None
            };
            items.push(ctx.build_item(texts, fqn, tier, demoted, ctor.as_deref(), deprecated));
        }

        // ── Pass 2: stub_index (built-in PHP classes) ───────────────
        // Collect keys first to avoid deadlock (same reason as pass 1).
        let stub_keys: Vec<String> = self.stub_index.read().keys().map(str::to_owned).collect();
        for fqn in &stub_keys {
            let sn = short_name(fqn);
            if !matches_class_prefix(sn, fqn, &prefix_lower, is_fqn_prefix, expanded_prefix_lower) {
                continue;
            }
            if !seen_fqns.insert(fqn.to_string()) {
                continue;
            }
            let Some((tier, demoted, deprecated)) = classify(fqn, sn) else {
                continue;
            };
            let (mut base_name, filter, mut use_import) = class_edit_texts(
                sn,
                fqn,
                is_fqn_prefix,
                has_leading_backslash,
                effective_namespace,
            );
            let mut was_shortened = false;
            if should_shorten_via_imports
                && let Some(shortened) = shorten_fqn_via_use_map(fqn, file_use_map)
            {
                base_name = shortened;
                use_import = None;
                was_shortened = true;
            }
            if tier <= '1' {
                use_import = None;
            }
            let mut texts = ClassItemTexts {
                base_name,
                filter,
                use_import,
            };
            ctx.apply_import_fixups(&mut texts.base_name, &mut texts.use_import, was_shortened);
            let ctor = if needs_ctor {
                self.ctor_params_for(fqn)
            } else {
                None
            };
            items.push(ctx.build_item(texts, fqn, tier, demoted, ctor.as_deref(), deprecated));
        }

        // ── Namespace segment items (FQN mode only) ─────────────────
        // When the user is typing a namespace-qualified reference (e.g.
        // `App\`, `\Illuminate\Database\`), inject the distinct
        // next-level namespace segments as MODULE-kind items so the
        // user can drill into the namespace tree incrementally instead
        // of being overwhelmed by hundreds of deeply-nested classes.
        if is_fqn_prefix {
            // Everything up to and including the last `\` in the
            // normalized (no leading `\`) prefix.  For `App\Models\U`
            // this is `App\Models\`; for `App\` it is `App\`.
            let ns_prefix_end = normalized.rfind('\\').map(|p| p + 1).unwrap_or(0);

            // Only inject segments when the prefix actually contains a
            // backslash.  A bare name like `User` in UseImport context
            // has `is_fqn_prefix` true but no namespace to browse.
            if ns_prefix_end > 0 {
                // When the prefix starts with an alias, use the
                // expanded FQN form for namespace segment matching so
                // that `OA\` finds segments under `OpenApi\Attributes\`.
                let raw_ns_prefix = &normalized[..ns_prefix_end];
                let expanded_ns = expand_alias_prefix(raw_ns_prefix, file_use_map);
                let ns_prefix_lower = expanded_ns
                    .as_deref()
                    .unwrap_or(raw_ns_prefix)
                    .to_lowercase();
                // Partial text after the last `\` that the user is
                // still typing (e.g. `U` from `App\Models\U`).  Used
                // to filter segments whose short name doesn't match.
                let after_ns_lower = normalized[ns_prefix_end..].to_lowercase();

                let mut seen_segments: HashSet<String> = HashSet::new();

                for fqn in &seen_fqns {
                    let fqn_lower = fqn.to_lowercase();
                    if !fqn_lower.starts_with(&ns_prefix_lower) {
                        continue;
                    }
                    // Portion of the FQN after the namespace prefix.
                    // PHP namespaces are ASCII so byte offsets match.
                    let rest = &fqn[ns_prefix_end..];
                    if let Some(next_bs) = rest.find('\\') {
                        let segment_short = &rest[..next_bs];
                        // Filter: the segment's short name must start
                        // with whatever the user typed after the last `\`.
                        if !after_ns_lower.is_empty()
                            && !segment_short.to_lowercase().starts_with(&after_ns_lower)
                        {
                            continue;
                        }
                        let segment = fqn[..ns_prefix_end + next_bs].to_string();
                        seen_segments.insert(segment);
                    }
                }

                for segment in &seen_segments {
                    let short = segment.rsplit('\\').next().unwrap_or(segment);

                    // Compute insert text the same way class_edit_texts
                    // does for FQN mode.
                    let (label, insert_ns) = if let Some(ns) = effective_namespace {
                        let ns_with_slash = format!("{}\\", ns);
                        if let Some(relative) = segment.strip_prefix(&ns_with_slash) {
                            (relative.to_string(), relative.to_string())
                        } else if has_leading_backslash {
                            (segment.clone(), format!("\\{}", segment))
                        } else {
                            (segment.clone(), segment.clone())
                        }
                    } else if has_leading_backslash {
                        (segment.clone(), format!("\\{}", segment))
                    } else {
                        (segment.clone(), segment.clone())
                    };

                    let filter = if has_leading_backslash {
                        format!("\\{}", segment)
                    } else {
                        segment.clone()
                    };

                    items.push(CompletionItem {
                        label,
                        kind: Some(CompletionItemKind::MODULE),
                        detail: Some(format!("namespace {}", segment)),
                        insert_text: Some(insert_ns.clone()),
                        filter_text: Some(filter),
                        sort_text: Some(format!("0!_{}", short.to_lowercase())),
                        text_edit: fqn_replace_range.map(|range| {
                            CompletionTextEdit::Edit(TextEdit {
                                range,
                                new_text: insert_ns,
                            })
                        }),
                        ..CompletionItem::default()
                    });
                }
            }
        }

        // Always sort by sort_text so the editor receives items in our
        // intended order.  Editors apply their own fuzzy scoring on top,
        // but many use sort_text as the primary or tie-breaking key when
        // items share the same fuzzy-match quality.  Sending items
        // unsorted lets the editor's internal ordering dominate, which
        // defeats our match-quality / tier / affinity scheme.
        items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));

        // Cap the result set so the client isn't overwhelmed.
        let is_incomplete = items.len() > Self::MAX_CLASS_COMPLETIONS;
        if is_incomplete {
            items.truncate(Self::MAX_CLASS_COMPLETIONS);
        }

        (items, is_incomplete)
    }

    /// Check whether a FQN is likely a namespace (not a class).
    ///
    /// Returns `true` only when we can *confirm* the FQN is a
    /// namespace — i.e. it is NOT a known class, AND we have positive
    /// evidence that it is a namespace (it appears as a namespace in
    /// `namespace_map`, or known classes exist under it as a prefix).
    ///
    /// When we have no information either way, returns `false` (benefit
    /// of the doubt — treat it as a potential class so undiscovered
    /// imports still appear in completions).
    fn is_likely_namespace_not_class(&self, fqn: &str) -> bool {
        // If the FQN is a known class, it's definitely not just a
        // namespace — even if classes also exist under it.
        if self.find_class_in_uri_classes_index(fqn).is_some() {
            return false;
        }
        if self.fqn_uri_index.read().contains_key(fqn) {
            return false;
        }

        if self.stub_index.read().contains_key(fqn) {
            return false;
        }

        // Not a known class. Check for positive namespace evidence.

        // 1. Some open file declares this FQN as its namespace.
        {
            let nmap = self.file_namespaces.read();
            for spans in nmap.values() {
                for span in spans {
                    if span.namespace.as_deref() == Some(fqn) {
                        return true;
                    }
                }
            }
        }

        // 2. Known classes exist under this FQN as a namespace prefix.
        let prefix = format!("{}\\", fqn);
        if self
            .fqn_uri_index
            .read()
            .keys()
            .any(|k| k.starts_with(&prefix))
        {
            return true;
        }

        if self
            .stub_index
            .read()
            .keys()
            .any(|k| k.starts_with(&prefix))
        {
            return true;
        }

        // No evidence either way — benefit of the doubt.
        false
    }

    /// Check whether a class matches the given `ClassNameContext`, or
    /// allow it through if not loaded.
    ///
    /// Returns `Some(true)` when the class is loaded and passes
    /// `context.matches()`, `Some(false)` when loaded but rejected,
    /// and `None` when the class could not be loaded at all.
    ///
    /// Callers should hard-exclude on `Some(false)`, pass through on
    /// `None` (with optional heuristic demotion via
    /// [`ClassNameContext::likely_mismatch`]), and include on
    /// `Some(true)` without demotion.
    fn check_context_match(&self, class_name: &str, context: ClassNameContext) -> Option<bool> {
        // load_stub_class checks uri_classes_index first, then parses in-memory
        // stubs if needed.  No disk I/O.
        if let Some(cls) = self.load_stub_class(class_name) {
            let kind_ok = context.matches(&cls);
            if !kind_ok {
                return Some(false);
            }
            // ThrowNew and Catch require Throwable ancestry.
            if matches!(
                context,
                ClassNameContext::ThrowNew | ClassNameContext::Catch
            ) {
                return Some(self.is_throwable_descendant(class_name, 0));
            }
            return Some(true);
        }
        // Truly unknown.
        None
    }
}

#[cfg(test)]
#[path = "class_completion_tests.rs"]
mod tests;
