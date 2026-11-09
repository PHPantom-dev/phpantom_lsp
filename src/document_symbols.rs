//! Document Symbols (`textDocument/documentSymbol`).
//!
//! Returns a hierarchical tree of symbols for the current file so that
//! editors can display an outline view, breadcrumbs, and go-to-symbol
//! within a file.
//!
//! The handler builds the tree from two data sources:
//!
//! 1. **`uri_classes_index`** — provides `ClassInfo` records for every class,
//!    interface, trait, and enum in the file. Each class's methods,
//!    properties, and constants become child symbols.
//!
//! 2. **`global_functions`** — provides `FunctionInfo` records keyed by
//!    name with associated file URIs. We filter for entries belonging
//!    to the current file.
//!
//! 3. **`global_defines`** — provides `DefineInfo` records for
//!    `define()` / top-level `const` declarations.
//!
//! For a Blade template those three describe the virtual PHP the
//! preprocessor emits, so the tree they build is translated back to the
//! template's own coordinates and the template's own landmarks (its
//! sections, stacks, and component tags) are added on top. See
//! [`crate::blade::outline`].

use std::fmt::Write;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::blade::outline::OutlineEntry;
use crate::php_type::PhpType;
use crate::text_position::LineIndex;
use crate::types::{
    ClassInfo, ClassLikeKind, ConstantInfo, FunctionInfo, MethodInfo, ParameterInfo, PropertyInfo,
    Visibility,
};
use crate::util::short_name;

impl Backend {
    /// Build the `DocumentSymbol` tree for a single file.
    ///
    /// Returns `None` when the file has no symbols at all.
    #[allow(deprecated)] // DocumentSymbol::deprecated is deprecated in the LSP types crate
    pub fn handle_document_symbol(
        &self,
        uri: &str,
        content: &str,
    ) -> Option<DocumentSymbolResponse> {
        // Precompute line starts once. Every symbol converts several byte
        // offsets to positions, and a file with many declarations would
        // otherwise rescan from the start for each, making the outline O(n²).
        let idx = LineIndex::new(content);

        let mut symbols: Vec<DocumentSymbol> = Vec::new();

        // ── Classes, interfaces, traits, enums ──────────────────────
        if let Some(classes) = self.symbols.uri_classes_index.read().get(uri).cloned() {
            for class in &classes {
                if let Some(sym) = class_to_symbol(class, &idx) {
                    symbols.push(sym);
                }
            }
        }

        // ── Standalone functions ────────────────────────────────────
        {
            let fmap = self.symbols.global_functions.read();
            for (_name, (file_uri, func)) in fmap.iter() {
                if file_uri == uri
                    && let Some(sym) = function_to_symbol(func, &idx)
                {
                    symbols.push(sym);
                }
            }
        }

        // ── Global defines / constants ──────────────────────────────
        {
            let dmap = self.symbols.global_defines.read();
            for (name, info) in dmap.iter() {
                if info.file_uri == uri && info.name_offset > 0 {
                    // A namespaced `const` is indexed fully-qualified, but
                    // the source only spells its last segment, so the range
                    // has to be measured against that.
                    let short = crate::util::short_name(name);
                    let pos = idx.position(info.name_offset as usize);
                    let name_end = idx.position(info.name_offset as usize + short.len());
                    let range = Range::new(pos, name_end);
                    symbols.push(DocumentSymbol {
                        name: short.to_string(),
                        detail: info.value.clone(),
                        kind: SymbolKind::Constant,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: range,
                        children: None,
                    });
                }
            }
        }

        if self.is_php_template(uri) {
            return self.blade_document_symbols(uri, symbols);
        }

        // Sort by position so the outline matches source order.
        symbols.sort_by(|a, b| {
            a.range
                .start
                .line
                .cmp(&b.range.start.line)
                .then(a.range.start.character.cmp(&b.range.start.character))
        });

        if symbols.is_empty() {
            None
        } else {
            Some(DocumentSymbolResponse::DocumentSymbolList(symbols))
        }
    }

    /// The outline of a Blade template: `php_symbols`, built from the
    /// virtual PHP, translated back to the template, with the template's
    /// own sections, stacks, and component tags added.
    ///
    /// Everything is then nested by containment, so a component tag
    /// written inside a `@section` (or a class declared inside a `@php`
    /// block inside one) is listed under it.
    fn blade_document_symbols(
        &self,
        uri: &str,
        php_symbols: Vec<DocumentSymbol>,
    ) -> Option<DocumentSymbolResponse> {
        let content = self.get_file_content_arc(uri)?;
        let idx = LineIndex::new(&content);

        let mut symbols: Vec<DocumentSymbol> = php_symbols
            .into_iter()
            .filter_map(|symbol| self.translate_symbol(uri, symbol))
            .collect();
        if self.is_blade_file(uri) {
            symbols.extend(
                self.blade_outline(&content)
                    .into_iter()
                    .map(|entry| entry_to_symbol(entry, &idx)),
            );
        }

        if symbols.is_empty() {
            return None;
        }
        Some(DocumentSymbolResponse::DocumentSymbolList(
            nest_by_containment(symbols),
        ))
    }

    /// Translate a symbol built from the virtual PHP back to the
    /// template, dropping it when either of its ranges lands in the
    /// preprocessor's prologue: that code stands behind no template text,
    /// so there is nowhere in the file to list it.
    fn translate_symbol(&self, uri: &str, symbol: DocumentSymbol) -> Option<DocumentSymbol> {
        let range = self.try_translate_template_range(uri, symbol.range)?;
        let selection_range = self.try_translate_template_range(uri, symbol.selection_range)?;
        let children = symbol.children.map(|children| {
            children
                .into_iter()
                .filter_map(|child| self.translate_symbol(uri, child))
                .collect()
        });
        Some(DocumentSymbol {
            range,
            selection_range,
            children,
            ..symbol
        })
    }
}

/// Build the symbol for one Blade outline entry.
#[allow(deprecated)]
fn entry_to_symbol(entry: OutlineEntry, idx: &LineIndex<'_>) -> DocumentSymbol {
    DocumentSymbol {
        name: entry.name,
        detail: entry.detail,
        kind: entry.kind,
        tags: None,
        deprecated: None,
        range: Range::new(idx.position(entry.span.start), idx.position(entry.span.end)),
        selection_range: Range::new(
            idx.position(entry.selection.start),
            idx.position(entry.selection.end),
        ),
        children: None,
    }
}

/// Nest a flat list of symbols so that each one becomes a child of the
/// innermost symbol whose range encloses it.
///
/// Symbols that overlap without enclosing (a directive block opened
/// inside a component tag and closed outside it, which Blade itself
/// tolerates) end up as siblings rather than nested, which is what the
/// source says: neither contains the other.
fn nest_by_containment(mut symbols: Vec<DocumentSymbol>) -> Vec<DocumentSymbol> {
    symbols.sort_by_key(|symbol| {
        let start = (symbol.range.start.line, symbol.range.start.character);
        let end = (symbol.range.end.line, symbol.range.end.character);
        // The enclosing symbol comes first: same start, later end.
        (start, std::cmp::Reverse(end))
    });

    let mut roots: Vec<DocumentSymbol> = Vec::new();
    let mut open: Vec<DocumentSymbol> = Vec::new();
    for symbol in symbols {
        while open
            .last()
            .is_some_and(|parent| !encloses(&parent.range, &symbol.range))
        {
            let closed = open.pop().expect("the loop only runs with a last entry");
            attach(closed, &mut open, &mut roots);
        }
        open.push(symbol);
    }
    while let Some(closed) = open.pop() {
        attach(closed, &mut open, &mut roots);
    }
    roots
}

/// Add a finished symbol to the innermost symbol still open around it,
/// or to the top level when there is none.
fn attach(symbol: DocumentSymbol, open: &mut [DocumentSymbol], roots: &mut Vec<DocumentSymbol>) {
    match open.last_mut() {
        Some(parent) => parent.children.get_or_insert_default().push(symbol),
        None => roots.push(symbol),
    }
}

/// Whether `outer` covers all of `inner`.
fn encloses(outer: &Range, inner: &Range) -> bool {
    let position = |p: &Position| (p.line, p.character);
    position(&outer.start) <= position(&inner.start) && position(&inner.end) <= position(&outer.end)
}

// ── Converters ──────────────────────────────────────────────────────

/// Convert a `ClassInfo` to a `DocumentSymbol` with nested children
/// for methods, properties, and constants.
#[allow(deprecated)]
fn class_to_symbol(class: &ClassInfo, idx: &LineIndex<'_>) -> Option<DocumentSymbol> {
    // Skip anonymous classes (no meaningful name to display).
    if class.name.is_empty() {
        return None;
    }

    let kind = match class.kind {
        ClassLikeKind::Class => SymbolKind::Class,
        ClassLikeKind::Interface => SymbolKind::Interface,
        ClassLikeKind::Trait => SymbolKind::Class, // no dedicated trait kind in LSP
        ClassLikeKind::Enum => SymbolKind::Enum,
    };

    let range_start = idx.position(declaration_start(
        idx.content(),
        class.keyword_offset as usize,
    ));
    let range_end = idx.position(class.end_offset as usize);
    let full_range = Range::new(range_start, range_end);

    // Selection range covers just the class name.
    let name_start = idx.position(class.keyword_offset as usize);
    let selection_range = if class.keyword_offset > 0 {
        let name_offset = find_name_after_keyword(idx.content(), class.keyword_offset as usize);
        let ns = idx.position(name_offset);
        let ne = idx.position(name_offset + class.name.len());
        Range::new(ns, ne)
    } else {
        Range::new(name_start, name_start)
    };

    let mut children: Vec<DocumentSymbol> = Vec::new();

    // Constants and enum cases.
    for constant in &class.constants {
        if constant.is_virtual {
            continue;
        }
        if let Some(sym) = constant_to_symbol(constant, idx) {
            children.push(sym);
        }
    }

    // Properties.
    for prop in &class.properties {
        if prop.is_virtual {
            continue;
        }
        if let Some(sym) = property_to_symbol(prop, idx) {
            children.push(sym);
        }
    }

    // Methods.
    for method in &class.methods {
        if method.is_virtual {
            continue;
        }
        if let Some(sym) = method_to_symbol(method, class.kind, idx) {
            children.push(sym);
        }
    }

    // Sort children by position.
    children.sort_by(|a, b| {
        a.range
            .start
            .line
            .cmp(&b.range.start.line)
            .then(a.range.start.character.cmp(&b.range.start.character))
    });

    let detail = build_class_detail(class);
    let tags = if class.deprecation_message.is_some() {
        Some(vec![SymbolTag::Deprecated])
    } else {
        None
    };

    Some(DocumentSymbol {
        name: class.name.to_string(),
        detail,
        kind,
        tags,
        deprecated: None,
        range: full_range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    })
}

/// Compute the full declaration range for a callable (method or
/// function) starting at `name_offset`.
///
/// Finds the parameter list, then the body `{…}` (returning the offset
/// just past the matching `}`) or the `;` of an abstract/interface
/// declaration. Falls back to the name-only end when the structure can
/// not be located.
fn callable_declaration_end(content: &str, name_offset: usize, name_len: usize) -> usize {
    let fallback = name_offset + name_len;
    let Some(rel_paren) = content[name_offset..].find('(') else {
        return fallback;
    };
    let paren_open = name_offset + rel_paren;
    let Some(paren_close) =
        crate::text_scan::find_matching_forward(content, paren_open, b'(', b')')
    else {
        return fallback;
    };
    // After the parameter list, the declaration ends at the body's closing
    // brace or at the `;` of an abstract/interface method.
    for (i, ch) in content[paren_close + 1..].char_indices() {
        match ch {
            '{' => {
                let brace_open = paren_close + 1 + i;
                return crate::text_scan::find_matching_forward(content, brace_open, b'{', b'}')
                    .map(|c| c + 1)
                    .unwrap_or(fallback);
            }
            ';' => return paren_close + 1 + i + 1,
            _ => {}
        }
    }
    fallback
}

/// Compute the full declaration range for a property or constant, which
/// ends at its terminating `;`.
fn statement_declaration_end(content: &str, name_offset: usize, name_len: usize) -> usize {
    let fallback = name_offset + name_len;
    crate::text_scan::find_semicolon_balanced(&content[name_offset..])
        .map(|p| name_offset + p + 1)
        .unwrap_or(fallback)
}

/// Build a leaf `DocumentSymbol` for a member declaration.
///
/// The selection range covers the name, which is what an editor
/// highlights and jumps to; the full range covers the whole declaration
/// from its first modifier, so folding the outline entry folds the member
/// and a client that labels entries with their source text (Zed) shows
/// `private function charge` rather than a bare `charge`.
/// `declaration_end` says where that declaration finishes, since a
/// callable ends at its body's brace and a property or constant at its `;`.
///
/// A member with no recorded offset was synthesised rather than written,
/// so it has no place in the outline and yields `None`.
#[allow(deprecated)] // DocumentSymbol::deprecated is deprecated in the LSP types crate
#[allow(clippy::too_many_arguments)]
fn member_symbol(
    (name, name_offset, name_len): (String, u32, usize),
    kind: SymbolKind,
    detail: Option<String>,
    deprecated: bool,
    idx: &LineIndex<'_>,
    declaration_end: fn(&str, usize, usize) -> usize,
) -> Option<DocumentSymbol> {
    if name_offset == 0 {
        return None;
    }
    let name_start = idx.position(name_offset as usize);
    let selection_range = Range::new(name_start, idx.position(name_offset as usize + name_len));
    let decl_start = declaration_start(idx.content(), name_offset as usize);
    let decl_end = declaration_end(idx.content(), name_offset as usize, name_len);
    Some(DocumentSymbol {
        name,
        detail,
        kind,
        tags: deprecated.then(|| vec![SymbolTag::Deprecated]),
        deprecated: None,
        range: Range::new(idx.position(decl_start), idx.position(decl_end)),
        selection_range,
        children: None,
    })
}

/// Convert a `MethodInfo` to a `DocumentSymbol`.
#[allow(deprecated)]
fn method_to_symbol(
    method: &MethodInfo,
    owner_kind: ClassLikeKind,
    idx: &LineIndex<'_>,
) -> Option<DocumentSymbol> {
    let kind = if method.name == "__construct" {
        SymbolKind::Constructor
    } else {
        SymbolKind::Method
    };
    member_symbol(
        (
            method.name.to_string(),
            method.name_offset,
            method.name.len(),
        ),
        kind,
        build_method_detail(method, owner_kind),
        method.deprecation_message().is_some(),
        idx,
        callable_declaration_end,
    )
}

/// Convert a `PropertyInfo` to a `DocumentSymbol`.
#[allow(deprecated)]
fn property_to_symbol(prop: &PropertyInfo, idx: &LineIndex<'_>) -> Option<DocumentSymbol> {
    member_symbol(
        // The name offset points at the `$`, which is part of the name.
        (
            format!("${}", prop.name),
            prop.name_offset,
            prop.name.len() + 1,
        ),
        SymbolKind::Property,
        Some(build_property_detail(prop)),
        prop.deprecation_message.is_some(),
        idx,
        statement_declaration_end,
    )
}

/// Convert a `ConstantInfo` to a `DocumentSymbol`.
#[allow(deprecated)]
fn constant_to_symbol(constant: &ConstantInfo, idx: &LineIndex<'_>) -> Option<DocumentSymbol> {
    let (kind, detail) = if constant.is_enum_case {
        (SymbolKind::EnumMember, constant.enum_value.clone())
    } else {
        (SymbolKind::Constant, Some(build_constant_detail(constant)))
    };

    member_symbol(
        (
            constant.name.to_string(),
            constant.name_offset,
            constant.name.len(),
        ),
        kind,
        detail,
        constant.deprecation_message.is_some(),
        idx,
        statement_declaration_end,
    )
}

/// Convert a `FunctionInfo` to a `DocumentSymbol`.
#[allow(deprecated)]
fn function_to_symbol(func: &FunctionInfo, idx: &LineIndex<'_>) -> Option<DocumentSymbol> {
    member_symbol(
        (func.name.to_string(), func.name_offset, func.name.len()),
        SymbolKind::Function,
        build_function_detail(func),
        func.deprecation_message.is_some(),
        idx,
        callable_declaration_end,
    )
}

// ── Detail string builders ──────────────────────────────────────────

// Every detail string leads with the declaration's modifiers, visibility
// spelled out even when public, the way PHP tooling conventionally fills
// this field. The protocol has no structured field for visibility, so
// clients that group or order an outline by it read it from here.

fn visibility_keyword(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Private => "private",
    }
}

/// Build a detail string for a class
/// (e.g. "final extends BaseClass implements Foo, Bar").
fn build_class_detail(class: &ClassInfo) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    // Interfaces, traits and enums take no class modifiers; an enum is
    // recorded as final, but that is implicit, not something written.
    if class.kind == ClassLikeKind::Class {
        if class.is_abstract {
            parts.push("abstract".to_string());
        }
        if class.is_final {
            parts.push("final".to_string());
        }
        if class.is_readonly {
            parts.push("readonly".to_string());
        }
    }

    if let Some(ref parent) = class.parent_class {
        parts.push(format!("extends {}", short_name(parent)));
    }

    if !class.interfaces.is_empty() {
        let ifaces: Vec<&str> = class.interfaces.iter().map(|i| short_name(i)).collect();
        let keyword = if class.kind == ClassLikeKind::Interface {
            "extends"
        } else {
            "implements"
        };
        parts.push(format!("{} {}", keyword, ifaces.join(", ")));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Build a detail string for a method: its modifiers, then its signature
/// (e.g. "abstract protected static (int $id): Foo").
fn build_method_detail(method: &MethodInfo, owner_kind: ClassLikeKind) -> Option<String> {
    let mut detail = String::new();

    // Interface methods are abstract implicitly; PHP forbids writing it.
    if method.is_abstract && owner_kind != ClassLikeKind::Interface {
        detail.push_str("abstract ");
    }
    if method.is_final {
        detail.push_str("final ");
    }
    detail.push_str(visibility_keyword(method.visibility));
    detail.push(' ');
    if method.is_static {
        detail.push_str("static ");
    }

    push_signature(&mut detail, &method.parameters, method.return_type.as_ref());
    Some(detail)
}

/// Build a detail string for a property: its modifiers, then its type
/// (e.g. "private readonly Gateway").
fn build_property_detail(prop: &PropertyInfo) -> String {
    let mut detail = visibility_keyword(prop.visibility).to_string();
    if prop.is_static {
        detail.push_str(" static");
    }
    if prop.is_readonly {
        detail.push_str(" readonly");
    }
    if let Some(type_hint) = prop.type_hint_str() {
        detail.push(' ');
        detail.push_str(&type_hint);
    }
    detail
}

/// Build a detail string for a class constant: its visibility, then its
/// type, or its value when it has none (e.g. "public int", "private = 60").
fn build_constant_detail(constant: &ConstantInfo) -> String {
    let mut detail = visibility_keyword(constant.visibility).to_string();
    if let Some(type_hint) = constant.type_hint_str() {
        detail.push(' ');
        detail.push_str(&type_hint);
    } else if let Some(value) = &constant.value {
        detail.push_str(" = ");
        detail.push_str(value);
    }
    detail
}

/// Build a detail string for a standalone function showing its signature.
fn build_function_detail(func: &FunctionInfo) -> Option<String> {
    let mut detail = String::new();
    push_signature(&mut detail, &func.parameters, func.return_type.as_ref());
    Some(detail)
}

/// Append `(Type $a, ...$b): Ret` to `detail`.
fn push_signature(detail: &mut String, params: &[ParameterInfo], return_type: Option<&PhpType>) {
    detail.push('(');
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            detail.push_str(", ");
        }
        if let Some(t) = &p.type_hint {
            let _ = write!(detail, "{t} ");
        }
        if p.is_variadic {
            detail.push_str("...");
        }
        detail.push_str(&p.name);
    }
    detail.push(')');
    if let Some(ret) = return_type {
        let _ = write!(detail, ": {ret}");
    }
}

/// Keywords that can open a declaration the outline lists.
const DECLARATION_KEYWORDS: &[&str] = &[
    "abstract",
    "case",
    "const",
    "final",
    "function",
    "private",
    "protected",
    "public",
    "readonly",
    "static",
    "var",
];

/// Byte offset where the declaration whose name (or class keyword) sits
/// at `anchor` begins: its first modifier or keyword on the same line,
/// so `public static function foo` starts at `public`.
///
/// Scans back over the modifiers and any type between them and the name
/// (including DNF groups and `private(set)`), stopping at anything a
/// declaration cannot contain, such as a line break, `;`, a brace, the
/// `(` or `,` before a promoted constructor property, or the `]` of an
/// attribute. Returns `anchor` when no keyword precedes it there.
///
/// Only the same line is considered: a modifier on an earlier line is
/// rare, and a range starting on the name's own line is what clients that
/// label outline entries from the source text expect.
fn declaration_start(content: &str, anchor: usize) -> usize {
    let bytes = content.as_bytes();
    let is_word_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80;

    let mut start = anchor.min(bytes.len());
    let mut paren_depth = 0u32;
    while start > 0 {
        match bytes[start - 1] {
            b')' => paren_depth += 1,
            b'(' if paren_depth > 0 => paren_depth -= 1,
            b' ' | b'\t' | b'?' | b'|' | b'&' | b'\\' => {}
            b if is_word_byte(b) => {}
            _ => break,
        }
        start -= 1;
    }

    // The scan can take in a stray word (the `php` of `<?php function f`),
    // so the declaration starts at the first keyword, not the first word.
    let mut i = start;
    while i < anchor {
        if !is_word_byte(bytes[i]) {
            i += 1;
            continue;
        }
        let word_start = i;
        while i < anchor && is_word_byte(bytes[i]) {
            i += 1;
        }
        let word = &bytes[word_start..i];
        if DECLARATION_KEYWORDS
            .iter()
            .any(|kw| word.eq_ignore_ascii_case(kw.as_bytes()))
        {
            return word_start;
        }
    }
    anchor
}

/// Find the start of a class/interface/trait/enum name token after the
/// keyword at `keyword_offset`. Scans forward past whitespace to find
/// the identifier.
fn find_name_after_keyword(content: &str, keyword_offset: usize) -> usize {
    let bytes = content.as_bytes();
    let mut i = keyword_offset;

    while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
        i += 1;
    }

    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;
    use crate::test_fixtures::{make_class, make_method, make_method_with_params, make_param};

    #[test]
    fn short_name_extracts_last_segment() {
        assert_eq!(short_name("Foo\\Bar\\Baz"), "Baz");
        assert_eq!(short_name("Simple"), "Simple");
        assert_eq!(short_name(""), "");
    }

    // ── Nesting by containment ──────────────────────────────────────

    /// A symbol named `name` spanning `start`..`end` on line 0.
    #[allow(deprecated)]
    fn symbol(name: &str, start: u32, end: u32) -> DocumentSymbol {
        let range = Range::new(Position::new(0, start), Position::new(0, end));
        DocumentSymbol {
            name: name.to_string(),
            detail: None,
            kind: SymbolKind::Namespace,
            tags: None,
            deprecated: None,
            range,
            selection_range: range,
            children: None,
        }
    }

    /// The tree as `(name, depth)` pairs, in outline order.
    fn tree(symbols: &[DocumentSymbol]) -> Vec<(String, usize)> {
        fn walk(symbols: &[DocumentSymbol], depth: usize, out: &mut Vec<(String, usize)>) {
            for symbol in symbols {
                out.push((symbol.name.clone(), depth));
                if let Some(children) = &symbol.children {
                    walk(children, depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(symbols, 0, &mut out);
        out
    }

    #[test]
    fn an_enclosed_symbol_becomes_a_child() {
        let nested = nest_by_containment(vec![
            symbol("inner", 2, 4),
            symbol("outer", 0, 10),
            symbol("after", 6, 8),
        ]);
        assert_eq!(
            tree(&nested),
            [
                ("outer".to_string(), 0),
                ("inner".to_string(), 1),
                ("after".to_string(), 1),
            ]
        );
    }

    #[test]
    fn a_symbol_reaching_past_its_neighbour_stays_a_sibling() {
        let nested = nest_by_containment(vec![symbol("first", 0, 6), symbol("second", 4, 10)]);
        assert_eq!(
            tree(&nested),
            [("first".to_string(), 0), ("second".to_string(), 0)]
        );
    }

    #[test]
    fn find_name_after_keyword_skips_keyword_and_whitespace() {
        let content = "class  MyClass extends Base {";
        let offset = 0; // points to 'class'
        let name_offset = find_name_after_keyword(content, offset);
        assert_eq!(&content[name_offset..name_offset + 7], "MyClass");
    }

    #[test]
    fn build_method_detail_simple() {
        let method = make_method("foo", Some("void"));
        let detail = build_method_detail(&method, ClassLikeKind::Class);
        assert_eq!(detail, Some("public (): void".to_string()));
    }

    #[test]
    fn build_method_detail_abstract_and_final() {
        let mut method = make_method("run", None);
        method.is_abstract = true;
        assert_eq!(
            build_method_detail(&method, ClassLikeKind::Class),
            Some("abstract public ()".to_string())
        );
        // An interface method is abstract without saying so.
        assert_eq!(
            build_method_detail(&method, ClassLikeKind::Interface),
            Some("public ()".to_string())
        );

        method.is_abstract = false;
        method.is_final = true;
        method.visibility = Visibility::Protected;
        assert_eq!(
            build_method_detail(&method, ClassLikeKind::Class),
            Some("final protected ()".to_string())
        );
    }

    #[test]
    fn class_detail_leads_with_modifiers() {
        let class = ClassInfo {
            is_final: true,
            is_readonly: true,
            parent_class: Some(atom("Base")),
            ..make_class("Foo")
        };
        assert_eq!(
            build_class_detail(&class),
            Some("final readonly extends Base".to_string())
        );

        // Enums are recorded as final, but nobody writes it.
        let enum_class = ClassInfo {
            kind: ClassLikeKind::Enum,
            is_final: true,
            ..make_class("Suit")
        };
        assert_eq!(build_class_detail(&enum_class), None);
    }

    /// The text from `declaration_start` to `anchor`, where `anchor` is
    /// the start of `name` in `line`.
    fn declaration_prefix<'a>(line: &'a str, name: &str) -> &'a str {
        let anchor = line.rfind(name).expect("name is in the line");
        &line[declaration_start(line, anchor)..anchor]
    }

    #[test]
    fn declaration_start_takes_in_the_modifiers() {
        assert_eq!(
            declaration_prefix("    public static function &build(", "build"),
            "public static function &"
        );
        assert_eq!(
            declaration_prefix("    final public const int LIMIT = 3;", "LIMIT"),
            "final public const int "
        );
        assert_eq!(
            declaration_prefix("    public private(set) (A&B)|null $x;", "$x"),
            "public private(set) (A&B)|null "
        );
        assert_eq!(declaration_prefix("    case Hearts;", "Hearts"), "case ");
        assert_eq!(
            declaration_prefix("final readonly class Point", "class"),
            "final readonly "
        );
    }

    #[test]
    fn declaration_start_stops_at_the_enclosing_construct() {
        // A promoted property starts after the constructor's `(` or `,`.
        assert_eq!(
            declaration_prefix(
                "    public function __construct(private int $a, protected ?B $b)",
                "$b"
            ),
            "protected ?B "
        );
        assert_eq!(
            declaration_prefix("#[Pure] public function f()", "f"),
            "public function "
        );
        assert_eq!(declaration_prefix("$x = 1; function g()", "g"), "function ");
        assert_eq!(declaration_prefix("<?php function h()", "h"), "function ");
        // Nothing to take in: the range starts at the name.
        assert_eq!(declaration_prefix("    $second;", "$second"), "");
        assert_eq!(declaration_prefix("class Plain", "class"), "");
    }

    #[test]
    fn build_method_detail_with_params() {
        let mut items = make_param("$items", Some("array"), false);
        items.is_variadic = true;
        let mut method = make_method_with_params(
            "process",
            Some("int"),
            vec![make_param("$input", Some("string"), true), items],
        );
        method.is_static = true;
        method.visibility = Visibility::Protected;
        let detail = build_method_detail(&method, ClassLikeKind::Class);
        assert_eq!(
            detail,
            Some("protected static (string $input, array ...$items): int".to_string())
        );
    }

    #[test]
    fn build_class_detail_with_parent_and_interfaces() {
        let class = ClassInfo {
            parent_class: Some(atom("Bar")),
            interfaces: vec![atom("Baz"), atom("Qux")],
            ..make_class("Foo")
        };
        let detail = build_class_detail(&class);
        assert_eq!(detail, Some("extends Bar implements Baz, Qux".to_string()));
    }

    #[test]
    fn build_class_detail_interface_uses_extends() {
        let class = ClassInfo {
            kind: ClassLikeKind::Interface,
            interfaces: vec![atom("Bar")],
            ..make_class("Foo")
        };
        let detail = build_class_detail(&class);
        assert_eq!(detail, Some("extends Bar".to_string()));
    }

    #[test]
    fn function_detail_no_params_no_return() {
        let func = FunctionInfo {
            name: atom("noop"),
            ..Default::default()
        };
        let detail = build_function_detail(&func);
        assert_eq!(detail, Some("()".to_string()));
    }
}
