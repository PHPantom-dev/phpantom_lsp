//! Owned semantic records for headless, non-LSP consumers.
//!
//! Enable the `semantic-export` Cargo feature to compile this module. Callers
//! supply every document as source text; the exporter performs no workspace
//! discovery and starts no language-server transport.

mod calls;
mod declarations;
mod occurrences;
mod ranges;

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Backend;

use calls::export_calls;
use declarations::export_declarations;
use occurrences::{SpanExportContext, SpanExportOutput, export_span};

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
///
/// The configuration is reusable, the work is not: every export call
/// builds its own project, including the standard-library index, and
/// drops it again. Hand each call as many documents as resolve together
/// rather than calling it once per file.
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

    if let Some(map) = symbol_map.as_deref()
        && let Some(mapped_source) = map.source(&source.source)
    {
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
            mapped_source,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Stays a unit test because the cache window is a private knob: the
    /// public API only exposes the default window.
    #[test]
    fn cache_window_does_not_change_output() {
        let exporter = SemanticExporter::new("/workspace");
        let mut sources = Vec::new();
        for index in 0..8 {
            sources.push(SourceDocument {
                uri: format!("file:///workspace/Class{index}.php"),
                source: format!(
                    "<?php namespace App; class Class{index} {{ public function value(): int {{ return {index}; }} }}"
                ),
            });
            sources.push(SourceDocument {
                uri: format!("file:///workspace/use{index}.php"),
                source: format!("<?php namespace App; (new Class{index}())->value();"),
            });
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
