/// PHP keyword completions for non-member expression/statement contexts.
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::class_lookup::find_class_at_offset;
use crate::completion::class_completion::ClassNameContext;
use crate::symbol_map::SymbolMap;
use crate::types::{ClassInfo, ClassLikeKind};

/// Cursor context used to gate context-sensitive PHP keywords.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KeywordContext {
    /// Cursor is inside a function-like body (function, method, closure, arrow fn).
    pub in_function_like: bool,
    /// Cursor is inside a breakable construct (`for`/`foreach`/`while`/`do`/`switch`).
    pub in_breakable: bool,
    /// Cursor is inside a loop construct (`for`/`foreach`/`while`/`do`).
    pub in_loop: bool,
    /// Cursor is inside a `switch` body.
    pub in_switch: bool,
    /// Cursor is at top-level (outside classes and functions).
    pub in_top_level: bool,
    /// Cursor is in a class/interface/enum declaration header where `extends` is valid.
    pub in_extends_declaration_header: bool,
    /// Cursor is in a class/enum declaration header where `implements` is valid.
    pub in_implements_declaration_header: bool,
    /// Cursor is in a class-like body (outside method/function scope).
    pub class_body_kind: Option<ClassLikeKind>,
    /// Cursor is right after a class-member modifier chain followed by
    /// whitespace (e.g. `public `, `private static `).
    pub after_member_modifier_chain: bool,
}

/// Core PHP keywords that can be completed in generic code contexts.
///
/// This intentionally excludes type keywords handled by type-hint/docblock
/// completion paths (`int`, `string`, etc.).
const PHP_KEYWORDS: &[&str] = &[
    "abstract",
    "as",
    "break",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "die",
    "do",
    "echo",
    "else",
    "elseif",
    "empty",
    "enum",
    "eval",
    "exit",
    "extends",
    "final",
    "finally",
    "fn",
    "for",
    "foreach",
    "function",
    "global",
    "goto",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "interface",
    "isset",
    "list",
    "match",
    "namespace",
    "new",
    "print",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "return",
    "static",
    "switch",
    "throw",
    "trait",
    "try",
    "unset",
    "use",
    "while",
    "yield",
];

/// Scalar types allowed as enum backing types in `enum Name: …`.
const BACKED_ENUM_TYPES: &[&str] = &["string", "int"];

// ─── Declaration-header detection ───────────────────────────────────────────
//
// The parser does not produce usable AST nodes for incomplete declaration
// headers (e.g. `class Foo ext|` where the user is still typing), so we
// fall back to a lightweight text scan.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclarationHeaderKind {
    Class,
    Interface,
    Enum,
}

/// Return the start index of the current statement-ish segment ending at
/// `offset` (exclusive), using `{`, `}`, `;`, and newlines as hard boundaries.
fn statement_segment_start(chars: &[char], offset: usize) -> usize {
    for i in (0..offset).rev() {
        if matches!(chars[i], '{' | '}' | ';' | '\n' | '\r') {
            return i + 1;
        }
    }
    0
}

/// Extract contiguous ASCII word tokens from a char slice, lowercased.
fn collect_ascii_words(chars: &[char], start: usize, end: usize) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut i = start;
    while i < end {
        if chars[i].is_ascii_alphanumeric() || chars[i] == '_' {
            let j = i;
            i += 1;
            while i < end && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            words.push(chars[j..i].iter().collect::<String>().to_ascii_lowercase());
            continue;
        }
        i += 1;
    }
    words
}

/// Detect whether the cursor is inside a class/interface/enum declaration
/// header (before the opening `{`).
fn declaration_header_kind(content: &str, position: Position) -> Option<DeclarationHeaderKind> {
    let chars: Vec<char> = content.chars().collect();
    let offset = crate::text_position::position_to_char_offset(&chars, position)?;

    let start = statement_segment_start(&chars, offset);
    let words = collect_ascii_words(&chars, start, offset);

    if words.is_empty() {
        return None;
    }

    let mut decl_idx_kind: Option<(usize, DeclarationHeaderKind)> = None;
    for (idx, w) in words.iter().enumerate() {
        let kind = match w.as_str() {
            "class" => Some(DeclarationHeaderKind::Class),
            "interface" => Some(DeclarationHeaderKind::Interface),
            "enum" => Some(DeclarationHeaderKind::Enum),
            _ => None,
        };
        if let Some(kind) = kind {
            decl_idx_kind = Some((idx, kind));
            break;
        }
    }
    let (decl_idx, kind) = decl_idx_kind?;

    // Only declaration modifiers may appear before the declaration keyword.
    let valid_prefix = words[..decl_idx]
        .iter()
        .all(|w| matches!(w.as_str(), "abstract" | "final" | "readonly"));
    if !valid_prefix {
        return None;
    }

    // Need at least the declaration name token after `class|interface|enum`.
    let name_token = words.get(decl_idx + 1)?;

    if name_token.is_empty() {
        return None;
    }

    // Guard out anonymous classes (`new class`).
    if kind == DeclarationHeaderKind::Class
        && decl_idx > 0
        && words.get(decl_idx - 1).is_some_and(|w| w == "new")
    {
        return None;
    }

    Some(kind)
}

/// Check whether the cursor immediately follows a chain of visibility/modifier
/// keywords (e.g. `public static `).
///
/// This uses a text scan because the parser does not produce an AST node for
/// an incomplete member declaration where only modifiers have been typed.
fn is_after_member_modifier_chain(content: &str, position: Position) -> bool {
    let chars: Vec<char> = content.chars().collect();
    let Some(offset) = crate::text_position::position_to_char_offset(&chars, position) else {
        return false;
    };
    if offset == 0 || !chars[offset - 1].is_ascii_whitespace() {
        return false;
    }

    let start = statement_segment_start(&chars, offset);
    let words = collect_ascii_words(&chars, start, offset);

    if words.is_empty() {
        return false;
    }

    words.iter().all(|w| {
        matches!(
            w.as_str(),
            "public" | "protected" | "private" | "static" | "abstract" | "final" | "readonly"
        )
    })
}

// ─── Public entry points ────────────────────────────────────────────────────

/// Build a [`KeywordContext`] from precomputed AST data and the source text.
///
/// AST-derived scope flags (`in_function_like`, `in_breakable`, `in_loop`,
/// `in_switch`, `in_class_like`) come directly from the symbol map and class
/// list.  Declaration-header detection and modifier-chain detection still use
/// text scans because the parser does not produce usable AST nodes for
/// incomplete declaration headers (the user is still typing them).
pub(crate) fn build_keyword_context(
    content: &str,
    position: Position,
    cursor_offset: u32,
    map: Option<&SymbolMap>,
    classes: &[Arc<ClassInfo>],
) -> KeywordContext {
    let decl_kind = declaration_header_kind(content, position);
    let in_function_like = map.is_some_and(|m| m.is_inside_function_like_scope(cursor_offset));
    let in_breakable = map.is_some_and(|m| m.is_inside_breakable_scope(cursor_offset));
    let in_loop = map.is_some_and(|m| m.is_inside_loop_scope(cursor_offset));
    let in_switch = map.is_some_and(|m| m.is_inside_switch_scope(cursor_offset));
    let class_at_cursor = find_class_at_offset(classes, cursor_offset);
    let in_class_like = class_at_cursor.is_some();

    // When the cursor is inside a method/function/closure body that happens
    // to be inside a class, `class_body_kind` is set to `None`.  This is
    // intentional: keyword filtering uses `class_body_kind` to restrict
    // completions to class-member keywords (`public`, `function`, `const`,
    // etc.).  Inside a method body all statement-level keywords should be
    // available instead.  For example, `case` should only appear when
    // `in_switch` is true (the cursor is inside a `switch` statement),
    // not unconditionally just because the enclosing class is an enum.
    let class_body_kind = if in_function_like {
        None
    } else {
        class_at_cursor.map(|c| c.kind)
    };

    // `in_top_level` gates the `namespace` keyword, which PHP only
    // allows at the very top level of a file — not inside functions,
    // classes, or control structures.  `in_breakable` covers loops and
    // `switch`; top-level `if`/`else` blocks are not tracked by the
    // symbol map, but they are rare enough in practice that the slight
    // over-offering of `namespace` there is acceptable.
    let in_top_level = !in_function_like && !in_class_like && !in_breakable;

    let after_member_modifier_chain =
        class_body_kind.is_some() && is_after_member_modifier_chain(content, position);

    KeywordContext {
        in_function_like,
        in_breakable,
        in_loop,
        in_switch,
        in_top_level,
        in_extends_declaration_header: decl_kind.is_some(),
        in_implements_declaration_header: matches!(
            decl_kind,
            Some(DeclarationHeaderKind::Class | DeclarationHeaderKind::Enum)
        ),
        class_body_kind,
        after_member_modifier_chain,
    }
}

/// Detect `enum Name: <partial>` positions and return the typed partial.
///
/// Returns `Some(partial)` when the cursor is in an enum declaration header
/// right after the `:` backing-type separator, `None` otherwise.
pub(crate) fn enum_backing_type_partial(content: &str, position: Position) -> Option<String> {
    let chars: Vec<char> = content.chars().collect();
    let offset = crate::text_position::position_to_char_offset(&chars, position)?;

    // Walk backward through the currently typed token.
    let mut partial_start = offset;
    while partial_start > 0
        && (chars[partial_start - 1].is_ascii_alphanumeric() || chars[partial_start - 1] == '_')
    {
        partial_start -= 1;
    }

    // Reject obvious non-keyword contexts.
    if partial_start > 0 && chars[partial_start - 1] == '$' {
        return None;
    }
    if partial_start >= 2 && chars[partial_start - 2] == '-' && chars[partial_start - 1] == '>' {
        return None;
    }
    if partial_start >= 2 && chars[partial_start - 2] == ':' && chars[partial_start - 1] == ':' {
        return None;
    }

    let partial: String = chars[partial_start..offset].iter().collect();

    // Back up over whitespace before the partial and require `:`.
    let mut i = partial_start;
    while i > 0 && chars[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i == 0 || chars[i - 1] != ':' {
        return None;
    }

    // Only valid inside `enum` declaration headers.
    if !matches!(
        declaration_header_kind(content, position),
        Some(DeclarationHeaderKind::Enum)
    ) {
        return None;
    }

    Some(partial)
}

/// Build keyword completion items for the typed `prefix`.
///
/// Keywords are only shown in unrestricted contexts (`Any`) to avoid
/// leaking into class-only positions such as `new`, `extends`, `implements`,
/// and import/type contexts.
pub(crate) fn build_keyword_completions(
    prefix: &str,
    class_ctx: ClassNameContext,
    ctx: KeywordContext,
) -> Vec<CompletionItem> {
    if !matches!(class_ctx, ClassNameContext::Any) {
        return Vec::new();
    }

    let prefix_lower = prefix.to_lowercase();
    PHP_KEYWORDS
        .iter()
        .enumerate()
        .filter(|(_, keyword)| keyword.starts_with(&prefix_lower))
        .filter(|(_, keyword)| keyword_allowed(keyword, ctx))
        .map(|(idx, keyword)| CompletionItem {
            label: (*keyword).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("PHP keyword".to_string()),
            insert_text: Some((*keyword).to_string()),
            filter_text: Some((*keyword).to_string()),
            sort_text: Some(format!("3_{idx:03}_{keyword}")),
            ..CompletionItem::default()
        })
        .collect()
}

/// Build completion items for enum backing types (`string`, `int`).
pub(crate) fn build_backed_enum_type_completions(prefix: &str) -> Vec<CompletionItem> {
    let prefix_lower = prefix.to_ascii_lowercase();
    BACKED_ENUM_TYPES
        .iter()
        .enumerate()
        .filter(|(_, ty)| ty.starts_with(&prefix_lower))
        .map(|(idx, ty)| CompletionItem {
            label: (*ty).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Enum backing type".to_string()),
            insert_text: Some((*ty).to_string()),
            filter_text: Some((*ty).to_string()),
            sort_text: Some(format!("0_enum_backed_{idx:03}_{ty}")),
            ..CompletionItem::default()
        })
        .collect()
}

fn keyword_allowed(keyword: &&str, ctx: KeywordContext) -> bool {
    if let Some(kind) = ctx.class_body_kind {
        return keyword_allowed_in_class_body(keyword, kind);
    }

    match *keyword {
        "return" | "yield" => ctx.in_function_like,
        "break" => ctx.in_breakable,
        "continue" => ctx.in_loop,
        "case" | "default" => ctx.in_switch,
        "namespace" => ctx.in_top_level,
        "extends" => ctx.in_extends_declaration_header,
        "implements" => ctx.in_implements_declaration_header,
        _ => true,
    }
}

fn keyword_allowed_in_class_body(keyword: &&str, kind: ClassLikeKind) -> bool {
    match kind {
        ClassLikeKind::Class | ClassLikeKind::Trait => matches!(
            *keyword,
            "public"
                | "protected"
                | "private"
                | "static"
                | "final"
                | "abstract"
                | "readonly"
                | "function"
                | "const"
                | "use"
        ),
        ClassLikeKind::Interface => matches!(*keyword, "public" | "function" | "const"),
        ClassLikeKind::Enum => matches!(
            *keyword,
            "public" | "protected" | "private" | "static" | "function" | "const" | "use" | "case"
        ),
    }
}
