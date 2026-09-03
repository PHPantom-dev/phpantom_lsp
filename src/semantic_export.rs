//! Owned semantic records for headless, non-LSP consumers.
//!
//! Enable the `semantic-export` Cargo feature to compile this module. Callers
//! supply every document as source text; the exporter performs no workspace
//! discovery and starts no language-server transport.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Backend;
use crate::symbol_map::{ClassRefContext, SelfStaticParentKind, SymbolKind, SymbolMap};
use crate::types::{ClassInfo, ClassLikeKind, FileContext, FunctionInfo};

const RESOLVED_CLASS_CACHE_WINDOW: usize = 512;
const VARIABLE_DEFINITION_INDEX_THRESHOLD: usize = 16;
type MemberTargetMemo = BTreeMap<(String, String, OccurrenceKind), Option<String>>;

/// One PHP document supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDocument {
    /// Stable document URI used to identify and cross-reference the source.
    pub uri: String,
    /// Complete UTF-8 PHP source text.
    pub source: String,
}

/// Invalid caller input rejected before any document is exported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticExportError {
    /// More than one source used the same URI.
    DuplicateUri(String),
}

impl fmt::Display for SemanticExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateUri(uri) => write!(formatter, "duplicate source document URI: {uri}"),
        }
    }
}

impl Error for SemanticExportError {}

/// A half-open byte range in a document's UTF-8 source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ByteRange {
    /// Inclusive byte offset.
    pub start: u32,
    /// Exclusive byte offset.
    pub end: u32,
}

/// Kind of exported declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DeclarationKind {
    /// Class declaration.
    Class,
    /// Interface declaration.
    Interface,
    /// Trait declaration.
    Trait,
    /// Enum declaration.
    Enum,
    /// Enum case declaration.
    EnumCase,
    /// Standalone function declaration.
    Function,
    /// Class method declaration.
    Method,
    /// Class property declaration.
    Property,
    /// Global or class constant declaration.
    Constant,
}

/// Kind of relationship declared by a class-like symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RelationshipKind {
    /// Class inheritance.
    Extends,
    /// Interface implementation or inheritance.
    Implements,
    /// Trait use.
    UsesTrait,
}

/// An owned relationship to another class-like symbol.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExportRelationship {
    /// Relationship kind.
    pub kind: RelationshipKind,
    /// Fully-qualified target name.
    pub target: String,
}

/// An owned declaration record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportDeclaration {
    /// Declaration kind.
    pub kind: DeclarationKind,
    /// Name as declared.
    pub name: String,
    /// Fully-qualified symbol, using `Class::member` for class members.
    pub symbol: String,
    /// Owning class symbol for members.
    pub owner: Option<String>,
    /// Range of the declared name token.
    pub range: ByteRange,
    /// Effective declared type, when available.
    pub type_annotation: Option<String>,
    /// Human-readable docblock description, when available.
    pub documentation: Option<String>,
    /// Declared class relationships.
    pub relationships: Vec<ExportRelationship>,
}

/// Kind of exported occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OccurrenceKind {
    /// Class-like name.
    Class,
    /// Standalone function name.
    Function,
    /// Method name.
    Method,
    /// Property name.
    Property,
    /// Constant name.
    Constant,
    /// Local variable name.
    Variable,
    /// Imported symbol.
    Import,
    /// Type annotation.
    Type,
}

/// An owned symbol occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportOccurrence {
    /// Occurrence kind.
    pub kind: OccurrenceKind,
    /// Range of the occurrence token.
    pub range: ByteRange,
    /// Name as written in source.
    pub name: String,
    /// Fully-qualified target when PHPantom resolved one.
    pub resolved_symbol: Option<String>,
    /// Whether this occurrence is a declaration site.
    pub is_definition: bool,
}

/// Kind of call expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CallKind {
    /// Standalone function call.
    Function,
    /// Instance method call.
    Method,
    /// Static method call.
    StaticMethod,
    /// Class instantiation.
    Constructor,
}

/// An owned call record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportCall {
    /// Call kind.
    pub kind: CallKind,
    /// Normalized call expression used by PHPantom's resolver.
    pub expression: String,
    /// Resolved function, class, or `Class::method` symbol.
    pub resolved_symbol: Option<String>,
    /// Range inside the call's parentheses.
    pub arguments_range: ByteRange,
    /// Ranges of argument expressions in source order.
    pub arguments: Vec<ByteRange>,
}

/// Kind of document diagnostic emitted at the export boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ExportDiagnosticKind {
    /// PHP source could not be parsed completely.
    ParseError,
    /// A class-like name could not be resolved.
    UnresolvedClass,
    /// A standalone function name could not be resolved.
    UnresolvedFunction,
    /// A member target or receiver could not be resolved.
    UnresolvedMember,
    /// A global constant name could not be resolved.
    UnresolvedConstant,
}

/// An owned document diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportDiagnostic {
    /// Diagnostic category.
    pub kind: ExportDiagnosticKind,
    /// Source range associated with the diagnostic.
    pub range: ByteRange,
    /// Human-readable message.
    pub message: String,
}

/// All semantic records for one supplied document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportDocument {
    /// URI supplied with the source document.
    pub uri: String,
    /// Sorted declarations.
    pub declarations: Vec<ExportDeclaration>,
    /// Sorted symbol occurrences.
    pub occurrences: Vec<ExportOccurrence>,
    /// Sorted call expressions.
    pub calls: Vec<ExportCall>,
    /// Sorted parse and resolution diagnostics.
    pub diagnostics: Vec<ExportDiagnostic>,
}

/// Materialized result returned by the batch API.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportBatch {
    /// Documents sorted by URI.
    pub documents: Vec<ExportDocument>,
}

/// Reusable configuration for semantic export.
pub struct SemanticExporter {
    workspace_root: PathBuf,
}

impl SemanticExporter {
    /// Create an exporter rooted at `workspace_root`.
    ///
    /// The root provides project context only. Export never discovers or
    /// reads source files from it; all PHP documents must be supplied by the
    /// caller.
    #[must_use]
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    /// Export all documents into one owned batch.
    ///
    /// This is a convenience wrapper over [`Self::export_stream`].
    pub fn export<I>(&self, sources: I) -> Result<ExportBatch, SemanticExportError>
    where
        I: IntoIterator<Item = SourceDocument>,
    {
        let mut documents = Vec::new();
        self.export_stream(sources, |document| documents.push(document))?;
        Ok(ExportBatch { documents })
    }

    /// Export one deterministic owned document at a time.
    ///
    /// Every source is registered in one shared backend before the first
    /// callback, so cross-document resolution is identical to batch export.
    /// The callback is invoked in URI order.
    pub fn export_stream<I, F>(&self, sources: I, consume: F) -> Result<(), SemanticExportError>
    where
        I: IntoIterator<Item = SourceDocument>,
        F: FnMut(ExportDocument),
    {
        self.export_stream_with_cache_window(sources, RESOLVED_CLASS_CACHE_WINDOW, consume)
    }

    fn export_stream_with_cache_window<I, F>(
        &self,
        sources: I,
        cache_window: usize,
        mut consume: F,
    ) -> Result<(), SemanticExportError>
    where
        I: IntoIterator<Item = SourceDocument>,
        F: FnMut(ExportDocument),
    {
        let mut sources: Vec<_> = sources.into_iter().collect();
        sources.sort_by(|left, right| left.uri.cmp(&right.uri));
        if let Some(duplicate) = sources.windows(2).find(|pair| pair[0].uri == pair[1].uri) {
            return Err(SemanticExportError::DuplicateUri(duplicate[0].uri.clone()));
        }

        let backend = Backend::new_headless();
        *backend.workspace.workspace_root.write() = Some(self.workspace_root.clone());

        for source in &sources {
            backend.update_ast(&source.uri, &source.source);
        }

        for (index, source) in sources.into_iter().enumerate() {
            consume(export_document(&backend, &source));
            if cache_window != 0 && index.saturating_add(1) % cache_window == 0 {
                backend.resolved_class_cache.write().clear();
            }
        }
        Ok(())
    }
}

fn export_document(backend: &Backend, source: &SourceDocument) -> ExportDocument {
    let _parse_guard = crate::parser::with_parse_cache(&source.source);
    let _class_guard =
        crate::virtual_members::with_active_resolved_class_cache(&backend.resolved_class_cache);
    let _chain_guard = crate::type_engine::resolver::with_chain_resolution_cache();
    let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();

    let context = backend.file_context(&source.uri);
    let symbol_map = backend.symbol_maps.read().get(&source.uri).cloned();
    let mut declarations = export_declarations(backend, source, &context, symbol_map.as_deref());
    let mut output = SpanExportOutput::default();

    if let Some(map) = symbol_map.as_deref() {
        let variable_definition_offsets =
            (map.var_defs.len() > VARIABLE_DEFINITION_INDEX_THRESHOLD).then(|| {
                map.var_defs
                    .iter()
                    .map(|definition| definition.offset)
                    .collect()
            });
        let export_context = SpanExportContext {
            backend,
            source,
            file_context: &context,
            variable_definitions: &map.var_defs,
            variable_definition_offsets: variable_definition_offsets.as_ref(),
        };
        for span in &map.spans {
            export_span(&export_context, span, &mut output);
        }
    }

    if let Some(errors) = backend.parse_errors.read().get(&source.uri) {
        output
            .diagnostics
            .extend(errors.iter().map(|(message, start, end)| ExportDiagnostic {
                kind: ExportDiagnosticKind::ParseError,
                range: ByteRange {
                    start: *start,
                    end: (*end).max(*start),
                },
                message: message.clone(),
            }));
    }

    let mut calls = symbol_map.as_deref().map_or_else(Vec::new, |map| {
        export_calls(backend, source, &context, map, &mut output.member_targets)
    });
    let SpanExportOutput {
        mut occurrences,
        mut diagnostics,
        ..
    } = output;

    declarations.sort_by(|left, right| {
        left.range
            .cmp(&right.range)
            .then_with(|| left.symbol.cmp(&right.symbol))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    declarations.dedup();
    occurrences.sort_by(|left, right| {
        left.range
            .cmp(&right.range)
            .then_with(|| left.resolved_symbol.cmp(&right.resolved_symbol))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    occurrences.dedup();
    calls.sort_by(|left, right| {
        left.arguments_range
            .cmp(&right.arguments_range)
            .then_with(|| left.expression.cmp(&right.expression))
    });
    calls.dedup();
    diagnostics.sort_by(|left, right| {
        left.range
            .cmp(&right.range)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics.dedup();

    ExportDocument {
        uri: source.uri.clone(),
        declarations,
        occurrences,
        calls,
        diagnostics,
    }
}

fn export_declarations(
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

struct SpanExportContext<'a> {
    backend: &'a Backend,
    source: &'a SourceDocument,
    file_context: &'a FileContext,
    variable_definitions: &'a [crate::symbol_map::VarDefSite],
    variable_definition_offsets: Option<&'a HashSet<u32>>,
}

impl SpanExportContext<'_> {
    fn is_variable_definition(&self, offset: u32) -> bool {
        self.variable_definition_offsets.map_or_else(
            || {
                self.variable_definitions
                    .iter()
                    .any(|definition| definition.offset == offset)
            },
            |offsets| offsets.contains(&offset),
        )
    }
}

#[derive(Default)]
struct SpanExportOutput {
    member_targets: MemberTargetMemo,
    occurrences: Vec<ExportOccurrence>,
    diagnostics: Vec<ExportDiagnostic>,
}

fn export_span(
    export_context: &SpanExportContext<'_>,
    span: &crate::symbol_map::SymbolSpan,
    output: &mut SpanExportOutput,
) {
    let backend = export_context.backend;
    let source = export_context.source;
    let context = export_context.file_context;
    let range = ByteRange {
        start: span.start,
        end: span.end,
    };
    match &span.kind {
        SymbolKind::ClassReference {
            name,
            is_fqn,
            context: class_context,
        } => {
            let written = name.to_string();
            let resolved = if *is_fqn {
                backend.find_or_load_class(name.trim_start_matches('\\'))
            } else {
                let loader = backend.class_loader(context);
                loader(name)
            };
            let symbol = resolved.as_ref().map(|class| class.fqn().to_string());
            output.occurrences.push(ExportOccurrence {
                kind: if *class_context == ClassRefContext::UseImport {
                    OccurrenceKind::Import
                } else if *class_context == ClassRefContext::TypeHint {
                    OccurrenceKind::Type
                } else {
                    OccurrenceKind::Class
                },
                range,
                name: written.clone(),
                resolved_symbol: symbol,
                is_definition: false,
            });
            if resolved.is_none()
                && !matches!(
                    class_context,
                    ClassRefContext::TypeOperatorOperand | ClassRefContext::DocblockSee
                )
            {
                output.diagnostics.push(ExportDiagnostic {
                    kind: ExportDiagnosticKind::UnresolvedClass,
                    range,
                    message: format!("unresolved class-like symbol `{written}`"),
                });
            }
        }
        SymbolKind::ClassDeclaration { name } => {
            if let Some(class) = enclosing_class(&context.classes, span.start) {
                output.occurrences.push(ExportOccurrence {
                    kind: OccurrenceKind::Class,
                    range,
                    name: name.to_string(),
                    resolved_symbol: Some(class.fqn().to_string()),
                    is_definition: true,
                });
            }
        }
        SymbolKind::FunctionCall {
            name,
            is_definition,
            is_docblock_reference,
        } => {
            let loader = backend.function_loader(context);
            let function = loader(name, span.start);
            let symbol = function.as_ref().map(function_fqn);
            output.occurrences.push(ExportOccurrence {
                kind: OccurrenceKind::Function,
                range,
                name: name.to_string(),
                resolved_symbol: symbol,
                is_definition: *is_definition,
            });
            if !is_definition && function.is_none() && !is_docblock_reference {
                output.diagnostics.push(ExportDiagnostic {
                    kind: ExportDiagnosticKind::UnresolvedFunction,
                    range,
                    message: format!("unresolved function `{name}`"),
                });
            }
        }
        SymbolKind::MemberAccess {
            subject_text,
            member_name,
            is_static,
            is_method_call,
            docblock_ref,
            is_array_callable,
            ..
        } => {
            let kind = member_occurrence_kind(&source.source, range, *is_static, *is_method_call);
            let subject = subject_text.as_str(&source.source);
            let target = MemberTargetResolver {
                backend,
                context,
                source: &source.source,
                memo: &mut output.member_targets,
            }
            .resolve(subject, *is_static, span.start, member_name, kind);
            output.occurrences.push(ExportOccurrence {
                kind,
                range,
                name: member_name.to_string(),
                resolved_symbol: target.clone(),
                is_definition: false,
            });
            if target.is_none() && !docblock_ref.tolerates_missing_target() && !is_array_callable {
                output.diagnostics.push(ExportDiagnostic {
                    kind: ExportDiagnosticKind::UnresolvedMember,
                    range,
                    message: format!("unresolved member `{member_name}` on `{subject}`"),
                });
            }
        }
        SymbolKind::MemberDeclaration { name, is_static } => {
            if let Some(class) = enclosing_class(&context.classes, span.start) {
                let kind = declared_member_kind(class, name, *is_static);
                output.occurrences.push(ExportOccurrence {
                    kind,
                    range,
                    name: name.to_string(),
                    resolved_symbol: Some(format!("{}::{name}", class.fqn())),
                    is_definition: true,
                });
            }
        }
        SymbolKind::ConstantReference {
            name,
            is_definition,
        } => {
            let loader = backend.constant_loader(context);
            let resolved = loader(name, span.start);
            let symbol = if *is_definition {
                Some(declared_constant_symbol(
                    &source.source,
                    context,
                    range,
                    name,
                ))
            } else {
                resolved
                    .as_ref()
                    .map(|_| context.resolve_name_at(name, span.start))
            };
            output.occurrences.push(ExportOccurrence {
                kind: OccurrenceKind::Constant,
                range,
                name: name.to_string(),
                resolved_symbol: symbol,
                is_definition: *is_definition,
            });
            if !is_definition && resolved.is_none() {
                output.diagnostics.push(ExportDiagnostic {
                    kind: ExportDiagnosticKind::UnresolvedConstant,
                    range,
                    message: format!("unresolved constant `{name}`"),
                });
            }
        }
        SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => {
            output.occurrences.push(ExportOccurrence {
                kind: OccurrenceKind::Variable,
                range,
                name: name.to_string(),
                resolved_symbol: None,
                is_definition: export_context.is_variable_definition(span.start),
            });
        }
        SymbolKind::SelfStaticParent(keyword) => {
            let class = enclosing_class(&context.classes, span.start);
            let symbol = class.and_then(|class| match keyword {
                SelfStaticParentKind::Parent => class.parent_class.map(|name| name.to_string()),
                _ => Some(class.fqn().to_string()),
            });
            output.occurrences.push(ExportOccurrence {
                kind: OccurrenceKind::Class,
                range,
                name: self_reference_name(*keyword).to_string(),
                resolved_symbol: symbol,
                is_definition: false,
            });
        }
        _ => {}
    }
}

fn export_calls(
    backend: &Backend,
    source: &SourceDocument,
    context: &FileContext,
    map: &SymbolMap,
    member_targets: &mut MemberTargetMemo,
) -> Vec<ExportCall> {
    map.call_sites
        .iter()
        .map(|call| {
            let (kind, resolved_symbol) = resolve_call(
                backend,
                context,
                &source.source,
                &call.call_expression,
                call.args_start,
                member_targets,
            );
            let arguments = call
                .arg_offsets
                .iter()
                .enumerate()
                .filter_map(|(index, start)| {
                    let end = call
                        .comma_offsets
                        .get(index)
                        .copied()
                        .unwrap_or(call.args_end);
                    let is_named = call
                        .named_arg_indices
                        .binary_search(&(index as u32))
                        .is_ok();
                    argument_expression_range(&source.source, *start, end, is_named)
                })
                .collect();
            ExportCall {
                kind,
                expression: call.call_expression.clone(),
                resolved_symbol,
                arguments_range: ByteRange {
                    start: call.args_start,
                    end: call.args_end,
                },
                arguments,
            }
        })
        .collect()
}

fn resolve_call(
    backend: &Backend,
    context: &FileContext,
    source: &str,
    expression: &str,
    offset: u32,
    member_targets: &mut MemberTargetMemo,
) -> (CallKind, Option<String>) {
    if let Some(class_name) = expression.strip_prefix("new ") {
        let loader = backend.class_loader(context);
        return (
            CallKind::Constructor,
            loader(class_name).map(|class| class.fqn().to_string()),
        );
    }
    if let Some((subject, method)) = expression.rsplit_once("->") {
        return (
            CallKind::Method,
            MemberTargetResolver {
                backend,
                context,
                source,
                memo: member_targets,
            }
            .resolve(subject, false, offset, method, OccurrenceKind::Method),
        );
    }
    if let Some((subject, method)) = expression.rsplit_once("::") {
        return (
            CallKind::StaticMethod,
            MemberTargetResolver {
                backend,
                context,
                source,
                memo: member_targets,
            }
            .resolve(subject, true, offset, method, OccurrenceKind::Method),
        );
    }
    let loader = backend.function_loader(context);
    (
        CallKind::Function,
        loader(expression, offset).as_ref().map(function_fqn),
    )
}

struct MemberTargetResolver<'a> {
    backend: &'a Backend,
    context: &'a FileContext,
    source: &'a str,
    memo: &'a mut MemberTargetMemo,
}

impl MemberTargetResolver<'_> {
    fn resolve(
        &mut self,
        subject: &str,
        is_static: bool,
        offset: u32,
        member_name: &str,
        kind: OccurrenceKind,
    ) -> Option<String> {
        let owner = resolve_member_owner(
            self.backend,
            self.context,
            self.source,
            subject,
            is_static,
            offset,
        )?;
        let key = (owner.clone(), member_name.to_string(), kind);
        let declaring_owner = self
            .memo
            .entry(key)
            .or_insert_with(|| {
                resolve_declaring_member_owner(
                    self.backend,
                    self.context,
                    &owner,
                    member_name,
                    kind,
                )
            })
            .clone();
        if let Some(declaring_owner) = declaring_owner {
            return Some(format!("{declaring_owner}::{member_name}"));
        }

        let loader = self.backend.class_loader(self.context);
        let class = loader(&owner)?;
        let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
            &class,
            &loader,
            Some(&self.backend.resolved_class_cache),
        );
        member_exists(&resolved, member_name, kind).then(|| format!("{owner}::{member_name}"))
    }
}

fn resolve_member_owner(
    backend: &Backend,
    context: &FileContext,
    source: &str,
    subject: &str,
    is_static: bool,
    offset: u32,
) -> Option<String> {
    let class_loader = backend.class_loader(context);
    let function_loader = backend.function_loader(context);
    let resolution_context = crate::type_engine::subject_resolution::SubjectResolutionCtx {
        local_classes: &context.classes,
        use_map: &context.use_map,
        namespace: &context.namespace,
        content: source,
        class_loader: &class_loader,
        backend: Some(backend),
        function_loader: &function_loader,
    };
    let resolved = crate::type_engine::subject_resolution::resolve_subject_type(
        subject,
        is_static,
        offset,
        &resolution_context,
    )?;
    let names = resolved.top_level_class_names();
    (names.len() == 1).then(|| names[0].trim_start_matches('\\').to_string())
}

fn resolve_declaring_member_owner(
    backend: &Backend,
    context: &FileContext,
    owner: &str,
    member_name: &str,
    kind: OccurrenceKind,
) -> Option<String> {
    let class_loader = backend.class_loader(context);
    let class = class_loader(owner)?;
    let mut visited = BTreeSet::new();
    find_declaring_member(&class, member_name, kind, &class_loader, &mut visited, 0)
}

fn find_declaring_member(
    class: &ClassInfo,
    member_name: &str,
    kind: OccurrenceKind,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Option<String> {
    const MAX_DEPTH: usize = 64;
    if depth > MAX_DEPTH || !visited.insert(class.fqn().to_string().to_ascii_lowercase()) {
        return None;
    }
    if member_exists(class, member_name, kind) {
        return Some(class.fqn().to_string());
    }
    for trait_name in &class.used_traits {
        if let Some(trait_class) = class_loader(trait_name)
            && let Some(owner) = find_declaring_member(
                &trait_class,
                member_name,
                kind,
                class_loader,
                visited,
                depth + 1,
            )
        {
            return Some(owner);
        }
    }
    if let Some(parent_name) = class.parent_class
        && let Some(parent) = class_loader(&parent_name)
        && let Some(owner) =
            find_declaring_member(&parent, member_name, kind, class_loader, visited, depth + 1)
    {
        return Some(owner);
    }
    if matches!(kind, OccurrenceKind::Method | OccurrenceKind::Constant) {
        for interface_name in &class.interfaces {
            if let Some(interface) = class_loader(interface_name)
                && let Some(owner) = find_declaring_member(
                    &interface,
                    member_name,
                    kind,
                    class_loader,
                    visited,
                    depth + 1,
                )
            {
                return Some(owner);
            }
        }
    }
    None
}

fn member_exists(class: &ClassInfo, member_name: &str, kind: OccurrenceKind) -> bool {
    match kind {
        OccurrenceKind::Method => class
            .methods
            .iter()
            .any(|member| member.name.eq_ignore_ascii_case(member_name)),
        OccurrenceKind::Property => class
            .properties
            .iter()
            .any(|member| member.name.as_str() == member_name),
        OccurrenceKind::Constant => class
            .constants
            .iter()
            .any(|member| member.name.as_str() == member_name),
        _ => false,
    }
}

fn member_occurrence_kind(
    source: &str,
    range: ByteRange,
    is_static: bool,
    is_method_call: bool,
) -> OccurrenceKind {
    if is_method_call {
        OccurrenceKind::Method
    } else if is_static
        && source
            .get(range.start as usize..range.end as usize)
            .is_some_and(|text| !text.starts_with('$'))
    {
        OccurrenceKind::Constant
    } else {
        OccurrenceKind::Property
    }
}

fn declared_member_kind(class: &ClassInfo, name: &str, is_static: bool) -> OccurrenceKind {
    if class
        .methods
        .iter()
        .any(|member| member.name.eq_ignore_ascii_case(name))
    {
        OccurrenceKind::Method
    } else if class
        .properties
        .iter()
        .any(|member| member.name.as_str() == name)
    {
        OccurrenceKind::Property
    } else if is_static {
        OccurrenceKind::Constant
    } else {
        OccurrenceKind::Property
    }
}

fn declared_constant_symbol(
    source: &str,
    context: &FileContext,
    range: ByteRange,
    name: &str,
) -> String {
    let is_define_string = source
        .as_bytes()
        .get(range.end as usize)
        .is_some_and(|byte| matches!(byte, b'\'' | b'"'));
    if is_define_string {
        name.trim_start_matches('\\').to_string()
    } else {
        context.resolve_name_at(name, range.start)
    }
}

fn function_fqn(function: &FunctionInfo) -> String {
    function.namespace.as_ref().map_or_else(
        || function.name.to_string(),
        |namespace| format!("{namespace}\\{}", function.name),
    )
}

fn self_reference_name(kind: SelfStaticParentKind) -> &'static str {
    match kind {
        SelfStaticParentKind::Self_ => "self",
        SelfStaticParentKind::Static => "static",
        SelfStaticParentKind::Parent => "parent",
        SelfStaticParentKind::This => "$this",
    }
}

fn enclosing_class(classes: &[Arc<ClassInfo>], offset: u32) -> Option<&ClassInfo> {
    classes
        .iter()
        .filter(|class| class.decl_start_offset <= offset && offset <= class.end_offset)
        .min_by_key(|class| class.end_offset.saturating_sub(class.decl_start_offset))
        .map(AsRef::as_ref)
}

fn trim_range(source: &str, start: u32, end: u32) -> Option<ByteRange> {
    let mut start = usize::try_from(start).ok()?;
    let mut end = usize::try_from(end).ok()?;
    if start > end || end > source.len() {
        return None;
    }
    while start < end && source.as_bytes()[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && source.as_bytes()[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    Some(ByteRange {
        start: u32::try_from(start).unwrap_or(u32::MAX),
        end: u32::try_from(end).unwrap_or(u32::MAX),
    })
}

fn argument_expression_range(
    source: &str,
    start: u32,
    end: u32,
    is_named: bool,
) -> Option<ByteRange> {
    let start = if is_named {
        let start_index = usize::try_from(start).ok()?;
        let end_index = usize::try_from(end).ok()?;
        let argument = source.get(start_index..end_index)?;
        let colon = argument.find(':')?;
        u32::try_from(start_index.saturating_add(colon).saturating_add(1)).ok()?
    } else {
        start
    };
    trim_range(source, start, end).filter(|range| range.start < range.end)
}

fn token_range(source: &str, offset: u32, name: &str, includes_dollar: bool) -> ByteRange {
    let start = usize::try_from(offset).unwrap_or(0).min(source.len());
    let expected = if includes_dollar {
        format!("${name}")
    } else {
        name.to_string()
    };
    let token_start = if source[start..].starts_with(&expected) {
        start
    } else {
        source[start..]
            .find(&expected)
            .map_or(start, |relative| start.saturating_add(relative))
    };
    ByteRange {
        start: u32::try_from(token_start).unwrap_or(u32::MAX),
        end: u32::try_from(token_start.saturating_add(expected.len())).unwrap_or(u32::MAX),
    }
}

fn find_token_between(source: &str, start: u32, end: u32, token: &str) -> u32 {
    let start = usize::try_from(start).unwrap_or(0).min(source.len());
    let end = usize::try_from(end)
        .unwrap_or(source.len())
        .min(source.len());
    source[start..end]
        .find(token)
        .and_then(|relative| u32::try_from(start.saturating_add(relative)).ok())
        .unwrap_or_else(|| u32::try_from(start).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(uri: &str, text: &str) -> SourceDocument {
        SourceDocument {
            uri: uri.to_string(),
            source: text.to_string(),
        }
    }

    #[test]
    fn resolves_across_documents_and_keeps_owned_results() {
        let batch = SemanticExporter::new("/workspace")
            .export([
                source(
                    "file:///workspace/use.php",
                    "<?php namespace App; $user = new User(); $user->name();",
                ),
                source(
                    "file:///workspace/User.php",
                    "<?php namespace App; class User { public function name(): string {} }",
                ),
            ])
            .unwrap();

        let use_document = batch
            .documents
            .iter()
            .find(|document| document.uri.ends_with("/use.php"))
            .unwrap();
        assert!(use_document.occurrences.iter().any(|occurrence| {
            occurrence.resolved_symbol.as_deref() == Some("App\\User::name")
        }));
        assert!(use_document.diagnostics.is_empty());
    }

    #[test]
    fn malformed_document_is_reported_without_stopping_the_batch() {
        let batch = SemanticExporter::new("/workspace")
            .export([
                source("file:///workspace/broken.php", "<?php class Broken {"),
                source("file:///workspace/good.php", "<?php class Good {}"),
            ])
            .unwrap();

        assert_eq!(batch.documents.len(), 2);
        assert!(
            batch.documents[0]
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == ExportDiagnosticKind::ParseError)
        );
        assert!(
            batch.documents[1]
                .declarations
                .iter()
                .any(|declaration| declaration.symbol == "Good")
        );
    }

    #[test]
    fn batch_and_streaming_outputs_are_identical_and_sorted() {
        let exporter = SemanticExporter::new("/workspace");
        let sources = vec![
            source("file:///workspace/z.php", "<?php function zed() {}"),
            source("file:///workspace/a.php", "<?php function alpha() {}"),
        ];
        let batch = exporter.export(sources.clone()).unwrap();
        let mut streamed = Vec::new();
        exporter
            .export_stream(sources, |document| streamed.push(document))
            .unwrap();

        assert_eq!(batch.documents, streamed);
        assert_eq!(batch.documents[0].uri, "file:///workspace/a.php");
    }

    #[test]
    fn repeated_exports_are_deterministic() {
        let exporter = SemanticExporter::new("/workspace");
        let sources = vec![
            source(
                "file:///workspace/a.php",
                "<?php namespace App; class A { public function go(): void {} }",
            ),
            source(
                "file:///workspace/b.php",
                "<?php namespace App; (new A())->go();",
            ),
        ];

        assert_eq!(
            exporter.export(sources.clone()).unwrap(),
            exporter.export(sources).unwrap()
        );
    }

    #[test]
    fn duplicate_uris_are_rejected_before_streaming() {
        let exporter = SemanticExporter::new("/workspace");
        let sources = [
            source("file:///workspace/a.php", "<?php class First {}"),
            source("file:///workspace/a.php", "<?php class Second {}"),
        ];
        let mut streamed = Vec::new();

        let error = exporter
            .export_stream(sources, |document| streamed.push(document))
            .unwrap_err();

        assert_eq!(
            error,
            SemanticExportError::DuplicateUri("file:///workspace/a.php".to_string())
        );
        assert!(streamed.is_empty());
    }

    #[test]
    fn named_argument_ranges_cover_only_expressions() {
        let source_text = "<?php function use_it($first, $second) {} $value = 1; use_it(first: $value, second: 'x:y');";
        let batch = SemanticExporter::new("/workspace")
            .export([source("file:///workspace/calls.php", source_text)])
            .unwrap();
        let call = batch.documents[0]
            .calls
            .iter()
            .find(|call| call.expression == "use_it")
            .unwrap();
        let argument_texts: Vec<_> = call
            .arguments
            .iter()
            .map(|range| &source_text[range.start as usize..range.end as usize])
            .collect();

        assert_eq!(argument_texts, ["$value", "'x:y'"]);
    }

    #[test]
    fn duplicate_global_constants_are_exported_from_each_document() {
        let batch = SemanticExporter::new("/workspace")
            .export([
                source(
                    "file:///workspace/a.php",
                    "<?php namespace App; const SHARED = 1;",
                ),
                source("file:///workspace/b.php", "<?php define('SHARED', 2);"),
            ])
            .unwrap();

        assert!(batch.documents[0].declarations.iter().any(|declaration| {
            declaration.kind == DeclarationKind::Constant
                && declaration.name == "SHARED"
                && declaration.symbol == "App\\SHARED"
        }));
        assert!(batch.documents[1].declarations.iter().any(|declaration| {
            declaration.kind == DeclarationKind::Constant
                && declaration.name == "SHARED"
                && declaration.symbol == "SHARED"
        }));
    }

    #[test]
    fn define_inside_namespace_stays_global() {
        let batch = SemanticExporter::new("/workspace")
            .export([source(
                "file:///workspace/constants.php",
                "<?php namespace App; define('GLOBAL_NAME', 1);",
            )])
            .unwrap();

        assert!(batch.documents[0].declarations.iter().any(|declaration| {
            declaration.kind == DeclarationKind::Constant && declaration.symbol == "GLOBAL_NAME"
        }));
        assert!(batch.documents[0].occurrences.iter().any(|occurrence| {
            occurrence.kind == OccurrenceKind::Constant
                && occurrence.is_definition
                && occurrence.resolved_symbol.as_deref() == Some("GLOBAL_NAME")
        }));
    }

    #[test]
    fn cache_window_does_not_change_output() {
        let exporter = SemanticExporter::new("/workspace");
        let mut sources = Vec::new();
        for index in 0..8 {
            sources.push(source(
                &format!("file:///workspace/Class{index}.php"),
                &format!(
                    "<?php namespace App; class Class{index} {{ public function value(): int {{ return {index}; }} }}"
                ),
            ));
            sources.push(source(
                &format!("file:///workspace/use{index}.php"),
                &format!("<?php namespace App; (new Class{index}())->value();"),
            ));
        }

        let export_with_window = |window| {
            let mut documents = Vec::new();
            exporter
                .export_stream_with_cache_window(sources.clone(), window, |document| {
                    documents.push(document);
                })
                .unwrap();
            documents
        };

        assert_eq!(export_with_window(0), export_with_window(1));
        assert_eq!(export_with_window(1), export_with_window(4));
        assert_eq!(export_with_window(4), export_with_window(512));
    }
}
