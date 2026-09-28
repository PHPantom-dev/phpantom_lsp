//! Declaration records for the classes, functions, and constants a
//! document declares.

use crate::Backend;
use crate::symbol_map::{SymbolKind, SymbolMap};
use crate::types::{ClassInfo, ClassLikeKind, FileContext, FunctionInfo};

use super::calls::declared_constant_symbol;
use super::ranges::{find_token_between, token_range};
use super::{
    ByteRange, DeclarationKind, ExportDeclaration, ExportRelationship, RelationshipKind,
    SourceDocument,
};

pub(super) fn export_declarations(
    backend: &Backend,
    source: &SourceDocument,
    context: &FileContext,
    symbol_map: Option<&SymbolMap>,
) -> Vec<ExportDeclaration> {
    let mut declarations = Vec::new();
    for class in &context.classes {
        export_class(&source.source, class, &mut declarations);
    }

    let globals = backend
        .symbols
        .uri_globals_index
        .read()
        .get(&source.uri)
        .cloned()
        .unwrap_or_default();
    for fqn in &globals.0 {
        if let Some(function) = function_declared_by(backend, fqn, &source.uri) {
            declarations.push(export_function(&source.source, fqn, &function));
        }
    }
    if let Some(map) = symbol_map {
        for span in &map.spans {
            let SymbolKind::ConstantReference {
                name,
                is_definition: true,
            } = &span.kind
            else {
                continue;
            };
            declarations.push(ExportDeclaration {
                kind: DeclarationKind::Constant,
                name: name.to_string(),
                symbol: declared_constant_symbol(
                    &source.source,
                    context,
                    ByteRange {
                        start: span.start,
                        end: span.end,
                    },
                    name,
                ),
                owner: None,
                range: ByteRange {
                    start: span.start,
                    end: span.end,
                },
                type_annotation: None,
                documentation: None,
                relationships: Vec::new(),
            });
        }
    }
    declarations
}

fn function_declared_by(backend: &Backend, fqn: &str, uri: &str) -> Option<FunctionInfo> {
    if let Some((declaring_uri, function)) = backend.symbols.global_functions.read().get(fqn)
        && declaring_uri == uri
    {
        return Some(function.clone());
    }
    backend
        .symbols
        .duplicate_functions
        .read()
        .get(fqn)
        .and_then(|declarations| declarations.get(uri))
        .cloned()
}

fn export_function(source: &str, fqn: &str, function: &FunctionInfo) -> ExportDeclaration {
    ExportDeclaration {
        kind: DeclarationKind::Function,
        name: function.name.to_string(),
        symbol: fqn.to_string(),
        owner: None,
        range: token_range(source, function.name_offset, function.name.as_ref(), false),
        type_annotation: function.return_type.as_ref().map(ToString::to_string),
        documentation: function.description.clone(),
        relationships: Vec::new(),
    }
}

fn export_class(source: &str, class: &ClassInfo, output: &mut Vec<ExportDeclaration>) {
    let fqn = class.fqn().to_string();
    let mut relationships = Vec::new();
    if let Some(parent) = class.parent_class {
        relationships.push(ExportRelationship {
            kind: RelationshipKind::Extends,
            target: parent.to_string(),
        });
    }
    relationships.extend(class.interfaces.iter().map(|target| ExportRelationship {
        kind: RelationshipKind::Implements,
        target: target.to_string(),
    }));
    relationships.extend(class.used_traits.iter().map(|target| ExportRelationship {
        kind: RelationshipKind::UsesTrait,
        target: target.to_string(),
    }));
    relationships.sort();
    relationships.dedup();

    let class_start = find_token_between(
        source,
        class.keyword_offset,
        class.start_offset,
        class.name.as_ref(),
    );
    output.push(ExportDeclaration {
        kind: match class.kind {
            ClassLikeKind::Class => DeclarationKind::Class,
            ClassLikeKind::Interface => DeclarationKind::Interface,
            ClassLikeKind::Trait => DeclarationKind::Trait,
            ClassLikeKind::Enum => DeclarationKind::Enum,
        },
        name: class.name.to_string(),
        symbol: fqn.clone(),
        owner: None,
        range: token_range(source, class_start, class.name.as_ref(), false),
        type_annotation: None,
        documentation: class.class_docblock.clone(),
        relationships,
    });

    for method in class.methods.iter().filter(|method| !method.is_virtual) {
        output.push(ExportDeclaration {
            kind: DeclarationKind::Method,
            name: method.name.to_string(),
            symbol: format!("{fqn}::{}", method.name),
            owner: Some(fqn.clone()),
            range: token_range(source, method.name_offset, method.name.as_ref(), false),
            type_annotation: method.return_type.as_ref().map(ToString::to_string),
            documentation: method.description.clone(),
            relationships: Vec::new(),
        });
    }
    for property in class
        .properties
        .iter()
        .filter(|property| !property.is_virtual)
    {
        output.push(ExportDeclaration {
            kind: DeclarationKind::Property,
            name: property.name.to_string(),
            symbol: format!("{fqn}::{}", property.name),
            owner: Some(fqn.clone()),
            range: token_range(source, property.name_offset, property.name.as_ref(), true),
            type_annotation: property.type_hint_str(),
            documentation: property.description.clone(),
            relationships: Vec::new(),
        });
    }
    for constant in class
        .constants
        .iter()
        .filter(|constant| !constant.is_virtual)
    {
        output.push(ExportDeclaration {
            kind: if constant.is_enum_case {
                DeclarationKind::EnumCase
            } else {
                DeclarationKind::Constant
            },
            name: constant.name.to_string(),
            symbol: format!("{fqn}::{}", constant.name),
            owner: Some(fqn.clone()),
            range: token_range(source, constant.name_offset, constant.name.as_ref(), false),
            type_annotation: constant.type_hint_str(),
            documentation: constant.description.clone(),
            relationships: Vec::new(),
        });
    }
}
