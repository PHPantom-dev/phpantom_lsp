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

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::types::{
    ClassInfo, ClassLikeKind, ConstantInfo, FunctionInfo, MethodInfo, PropertyInfo, Visibility,
};
use crate::util::{LineIndex, short_name};

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
        if let Some(classes) = self.uri_classes_index.read().get(uri).cloned() {
            for class in &classes {
                if let Some(sym) = class_to_symbol(class, &idx) {
                    symbols.push(sym);
                }
            }
        }

        // ── Standalone functions ────────────────────────────────────
        {
            let fmap = self.global_functions.read();
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
            let dmap = self.global_defines.read();
            for (name, info) in dmap.iter() {
                if info.file_uri == uri && info.name_offset > 0 {
                    let pos = idx.position(info.name_offset as usize);
                    let name_end = idx.position(info.name_offset as usize + name.len());
                    let range = Range::new(pos, name_end);
                    symbols.push(DocumentSymbol {
                        name: name.clone(),
                        detail: info.value.clone(),
                        kind: SymbolKind::CONSTANT,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: range,
                        children: None,
                    });
                }
            }
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
            Some(DocumentSymbolResponse::Nested(symbols))
        }
    }
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
        ClassLikeKind::Class => SymbolKind::CLASS,
        ClassLikeKind::Interface => SymbolKind::INTERFACE,
        ClassLikeKind::Trait => SymbolKind::CLASS, // no dedicated trait kind in LSP
        ClassLikeKind::Enum => SymbolKind::ENUM,
    };

    let range_start = idx.position(class.keyword_offset as usize);
    let range_end = idx.position(class.end_offset as usize);
    let full_range = Range::new(range_start, range_end);

    // Selection range covers just the class name.
    let name_start = idx.position(class.keyword_offset as usize);
    let selection_range = if class.keyword_offset > 0 {
        // Find the actual name token after the keyword. We use
        // keyword_offset as a reasonable approximation for the start.
        // The name appears shortly after the keyword; use the name
        // length to compute the selection end.
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
        if let Some(sym) = constant_to_symbol(constant, idx, class.kind == ClassLikeKind::Enum) {
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
        if let Some(sym) = method_to_symbol(method, idx) {
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
        Some(vec![SymbolTag::DEPRECATED])
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
    let Some(paren_close) = crate::util::find_matching_forward(content, paren_open, b'(', b')')
    else {
        return fallback;
    };
    // After the parameter list, the declaration ends at the body's closing
    // brace or at the `;` of an abstract/interface method.
    for (i, ch) in content[paren_close + 1..].char_indices() {
        match ch {
            '{' => {
                let brace_open = paren_close + 1 + i;
                return crate::util::find_matching_forward(content, brace_open, b'{', b'}')
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
    crate::util::find_semicolon_balanced(&content[name_offset..])
        .map(|p| name_offset + p + 1)
        .unwrap_or(fallback)
}

/// Convert a `MethodInfo` to a `DocumentSymbol`.
#[allow(deprecated)]
fn method_to_symbol(method: &MethodInfo, idx: &LineIndex<'_>) -> Option<DocumentSymbol> {
    if method.name_offset == 0 {
        return None;
    }

    let pos = idx.position(method.name_offset as usize);
    let name_end = idx.position(method.name_offset as usize + method.name.len());
    let selection_range = Range::new(pos, name_end);

    // The full range must enclose the whole declaration (signature and
    // body), with the selection range (the name) nested inside it.
    let decl_end = callable_declaration_end(
        idx.content(),
        method.name_offset as usize,
        method.name.len(),
    );
    let range = Range::new(pos, idx.position(decl_end));

    let detail = build_method_detail(method);
    let tags = if method.deprecation_message.is_some() {
        Some(vec![SymbolTag::DEPRECATED])
    } else {
        None
    };

    let kind = if method.name == "__construct" {
        SymbolKind::CONSTRUCTOR
    } else {
        SymbolKind::METHOD
    };

    Some(DocumentSymbol {
        name: method.name.to_string(),
        detail,
        kind,
        tags,
        deprecated: None,
        range,
        selection_range,
        children: None,
    })
}

/// Convert a `PropertyInfo` to a `DocumentSymbol`.
#[allow(deprecated)]
fn property_to_symbol(prop: &PropertyInfo, idx: &LineIndex<'_>) -> Option<DocumentSymbol> {
    if prop.name_offset == 0 {
        return None;
    }

    // The name_offset points to the `$` of the property name.
    let dollar_name_len = prop.name.len() + 1; // `$` + name
    let pos = idx.position(prop.name_offset as usize);
    let name_end = idx.position(prop.name_offset as usize + dollar_name_len);
    let selection_range = Range::new(pos, name_end);
    let decl_end =
        statement_declaration_end(idx.content(), prop.name_offset as usize, dollar_name_len);
    let range = Range::new(pos, idx.position(decl_end));

    let detail = prop.type_hint_str();
    let tags = if prop.deprecation_message.is_some() {
        Some(vec![SymbolTag::DEPRECATED])
    } else {
        None
    };

    Some(DocumentSymbol {
        name: format!("${}", prop.name),
        detail,
        kind: SymbolKind::PROPERTY,
        tags,
        deprecated: None,
        range,
        selection_range,
        children: None,
    })
}

/// Convert a `ConstantInfo` to a `DocumentSymbol`.
#[allow(deprecated)]
fn constant_to_symbol(
    constant: &ConstantInfo,
    idx: &LineIndex<'_>,
    is_enum: bool,
) -> Option<DocumentSymbol> {
    if constant.name_offset == 0 {
        return None;
    }

    let pos = idx.position(constant.name_offset as usize);
    let name_end = idx.position(constant.name_offset as usize + constant.name.len());
    let selection_range = Range::new(pos, name_end);
    let decl_end = statement_declaration_end(
        idx.content(),
        constant.name_offset as usize,
        constant.name.len(),
    );
    let range = Range::new(pos, idx.position(decl_end));

    let kind = if constant.is_enum_case {
        SymbolKind::ENUM_MEMBER
    } else {
        SymbolKind::CONSTANT
    };

    let detail = if constant.is_enum_case {
        constant.enum_value.clone()
    } else {
        constant.type_hint_str().or_else(|| constant.value.clone())
    };

    let tags = if constant.deprecation_message.is_some() {
        Some(vec![SymbolTag::DEPRECATED])
    } else {
        None
    };

    // For enum cases within an enum, show as ENUM_MEMBER.
    // For regular constants, show as CONSTANT.
    let _ = is_enum;

    Some(DocumentSymbol {
        name: constant.name.to_string(),
        detail,
        kind,
        tags,
        deprecated: None,
        range,
        selection_range,
        children: None,
    })
}

/// Convert a `FunctionInfo` to a `DocumentSymbol`.
#[allow(deprecated)]
fn function_to_symbol(func: &FunctionInfo, idx: &LineIndex<'_>) -> Option<DocumentSymbol> {
    if func.name_offset == 0 {
        return None;
    }

    let pos = idx.position(func.name_offset as usize);
    let name_end = idx.position(func.name_offset as usize + func.name.len());
    let selection_range = Range::new(pos, name_end);
    let decl_end =
        callable_declaration_end(idx.content(), func.name_offset as usize, func.name.len());
    let range = Range::new(pos, idx.position(decl_end));

    let detail = build_function_detail(func);
    let tags = if func.deprecation_message.is_some() {
        Some(vec![SymbolTag::DEPRECATED])
    } else {
        None
    };

    Some(DocumentSymbol {
        name: func.name.to_string(),
        detail,
        kind: SymbolKind::FUNCTION,
        tags,
        deprecated: None,
        range,
        selection_range,
        children: None,
    })
}

// ── Detail string builders ──────────────────────────────────────────

/// Build a detail string for a class (e.g. "extends BaseClass implements Foo, Bar").
fn build_class_detail(class: &ClassInfo) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

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

/// Build a detail string for a method showing its signature.
fn build_method_detail(method: &MethodInfo) -> Option<String> {
    let mut detail = String::new();

    // Visibility prefix.
    match method.visibility {
        Visibility::Public => {}
        Visibility::Protected => detail.push_str("protected "),
        Visibility::Private => detail.push_str("private "),
    }

    if method.is_static {
        detail.push_str("static ");
    }

    // Parameter list.
    detail.push('(');
    let params: Vec<String> = method
        .parameters
        .iter()
        .map(|p| {
            let mut s = String::new();
            if let Some(ref t) = p.type_hint {
                s.push_str(&t.to_string());
                s.push(' ');
            }
            if p.is_variadic {
                s.push_str("...");
            }
            s.push_str(&p.name);
            s
        })
        .collect();
    detail.push_str(&params.join(", "));
    detail.push(')');

    // Return type.
    if let Some(ref ret) = method.return_type {
        detail.push_str(": ");
        detail.push_str(&ret.to_string());
    }

    Some(detail)
}

/// Build a detail string for a standalone function showing its signature.
fn build_function_detail(func: &FunctionInfo) -> Option<String> {
    let mut detail = String::new();

    detail.push('(');
    let params: Vec<String> = func
        .parameters
        .iter()
        .map(|p| {
            let mut s = String::new();
            if let Some(ref t) = p.type_hint {
                s.push_str(&t.to_string());
                s.push(' ');
            }
            if p.is_variadic {
                s.push_str("...");
            }
            s.push_str(&p.name);
            s
        })
        .collect();
    detail.push_str(&params.join(", "));
    detail.push(')');

    if let Some(ref ret) = func.return_type {
        detail.push_str(": ");
        detail.push_str(&ret.to_string());
    }

    Some(detail)
}

/// Find the start of a class/interface/trait/enum name token after the
/// keyword at `keyword_offset`. Scans forward past whitespace to find
/// the identifier.
fn find_name_after_keyword(content: &str, keyword_offset: usize) -> usize {
    let bytes = content.as_bytes();
    let mut i = keyword_offset;

    // Skip the keyword itself (class, interface, trait, enum).
    while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
        i += 1;
    }

    // Skip whitespace between keyword and name.
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atom::atom;
    use crate::php_type::PhpType;
    use crate::types::{ClassLikeKind, MethodInfo, ParameterInfo, Visibility};

    #[test]
    fn short_name_extracts_last_segment() {
        assert_eq!(short_name("Foo\\Bar\\Baz"), "Baz");
        assert_eq!(short_name("Simple"), "Simple");
        assert_eq!(short_name(""), "");
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
        let method = MethodInfo {
            name: crate::atom::atom("foo"),
            name_offset: 0,
            parameters: vec![],
            return_type: Some(PhpType::parse("void")),
            native_return_type: None,
            description: None,
            return_description: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            is_static: false,
            visibility: Visibility::Public,
            conditional_return: None,
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: vec![],
            template_param_bounds: Default::default(),
            template_bindings: vec![],
            has_scope_attribute: false,
            is_abstract: false,
            is_virtual: false,
            type_assertions: vec![],
            throws: vec![],
            if_this_is: None,
        };
        let detail = build_method_detail(&method);
        assert_eq!(detail, Some("(): void".to_string()));
    }

    #[test]
    fn build_method_detail_with_params() {
        let method = MethodInfo {
            name: crate::atom::atom("process"),
            name_offset: 0,
            parameters: vec![
                ParameterInfo {
                    name: crate::atom::atom("$input"),
                    is_required: true,
                    type_hint: Some(PhpType::parse("string")),
                    native_type_hint: None,
                    description: None,
                    default_value: None,
                    is_variadic: false,
                    is_reference: false,
                    closure_this_type: None,
                },
                ParameterInfo {
                    name: crate::atom::atom("$items"),
                    is_required: false,
                    type_hint: Some(PhpType::parse("array")),
                    native_type_hint: None,
                    description: None,
                    default_value: Some("[]".to_string()),
                    is_variadic: true,
                    is_reference: false,
                    closure_this_type: None,
                },
            ],
            return_type: Some(PhpType::parse("int")),
            native_return_type: None,
            description: None,
            return_description: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            is_static: true,
            visibility: Visibility::Protected,
            conditional_return: None,
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: vec![],
            template_param_bounds: Default::default(),
            template_bindings: vec![],
            has_scope_attribute: false,
            is_abstract: false,
            is_virtual: false,
            type_assertions: vec![],
            throws: vec![],
            if_this_is: None,
        };
        let detail = build_method_detail(&method);
        assert_eq!(
            detail,
            Some("protected static (string $input, array ...$items): int".to_string())
        );
    }

    #[test]
    fn build_class_detail_with_parent_and_interfaces() {
        let class = ClassInfo {
            kind: ClassLikeKind::Class,
            name: crate::atom::atom("Foo"),
            methods: Default::default(),
            method_index: Default::default(),
            indexed_method_count: 0,
            properties: Default::default(),
            constants: Default::default(),
            start_offset: 0,
            end_offset: 0,
            keyword_offset: 0,
            decl_start_offset: 0,
            parent_class: Some(atom("Bar")),
            interfaces: vec![atom("Baz"), atom("Qux")],
            used_traits: vec![],
            mixins: vec![],
            mixin_generics: vec![],
            is_final: false,
            is_abstract: false,
            deprecation_message: None,
            deprecated_replacement: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            template_params: vec![],
            template_param_bounds: Default::default(),
            template_param_defaults: Default::default(),
            extends_generics: vec![],
            implements_generics: vec![],
            use_generics: vec![],
            type_aliases: Default::default(),
            trait_precedences: vec![],
            trait_aliases: vec![],
            class_docblock: None,
            file_namespace: None,
            backed_type: None,
            attribute_targets: 0,
            laravel: None,
        };
        let detail = build_class_detail(&class);
        assert_eq!(detail, Some("extends Bar implements Baz, Qux".to_string()));
    }

    #[test]
    fn build_class_detail_interface_uses_extends() {
        let class = ClassInfo {
            kind: ClassLikeKind::Interface,
            name: crate::atom::atom("Foo"),
            methods: Default::default(),
            method_index: Default::default(),
            indexed_method_count: 0,
            properties: Default::default(),
            constants: Default::default(),
            start_offset: 0,
            end_offset: 0,
            keyword_offset: 0,
            decl_start_offset: 0,
            parent_class: None,
            interfaces: vec![atom("Bar")],
            used_traits: vec![],
            mixins: vec![],
            mixin_generics: vec![],
            is_final: false,
            is_abstract: false,
            deprecation_message: None,
            deprecated_replacement: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            template_params: vec![],
            template_param_bounds: Default::default(),
            template_param_defaults: Default::default(),
            extends_generics: vec![],
            implements_generics: vec![],
            use_generics: vec![],
            type_aliases: Default::default(),
            trait_precedences: vec![],
            trait_aliases: vec![],
            class_docblock: None,
            file_namespace: None,
            backed_type: None,
            attribute_targets: 0,
            laravel: None,
        };
        let detail = build_class_detail(&class);
        assert_eq!(detail, Some("extends Bar".to_string()));
    }

    #[test]
    fn function_detail_no_params_no_return() {
        let func = FunctionInfo {
            name: crate::atom::atom("noop"),
            name_offset: 0,
            parameters: vec![],
            return_type: None,
            native_return_type: None,
            description: None,
            return_description: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            namespace: None,
            conditional_return: None,
            type_assertions: vec![],
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: vec![],
            template_param_bounds: Default::default(),
            template_bindings: vec![],
            throws: Vec::new(),
            is_polyfill: false,
            overloads: vec![],
        };
        let detail = build_function_detail(&func);
        assert_eq!(detail, Some("()".to_string()));
    }
}
