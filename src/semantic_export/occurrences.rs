//! Occurrence and diagnostic records for one symbol span.

use std::collections::HashSet;

use crate::Backend;
use crate::symbol_map::{
    ClassRefContext, MappedSource, SelfStaticParentKind, SymbolKind, VarDefSite,
};
use crate::types::FileContext;

use super::calls::{
    MemberTargetResolver, declared_constant_symbol, declared_member_kind, member_occurrence_kind,
};
use super::ranges::{enclosing_class, function_fqn, self_reference_name};
use super::{
    ByteRange, ExportDiagnostic, ExportDiagnosticKind, ExportOccurrence, MemberTargetMemo,
    OccurrenceKind, SourceDocument,
};

pub(super) struct SpanExportContext<'a> {
    pub(super) backend: &'a Backend,
    pub(super) source: &'a SourceDocument,
    /// The document text paired with the map the spans came from, so a
    /// range-backed member subject slices the revision it indexes.
    pub(super) mapped_source: MappedSource<'a>,
    pub(super) file_context: &'a FileContext,
    pub(super) variable_definitions: &'a [VarDefSite],
    pub(super) variable_definition_offsets: Option<&'a HashSet<u32>>,
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
pub(super) struct SpanExportOutput {
    pub(super) member_targets: MemberTargetMemo,
    pub(super) occurrences: Vec<ExportOccurrence>,
    pub(super) diagnostics: Vec<ExportDiagnostic>,
}

pub(super) fn export_span(
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
            let subject = subject_text.as_str(export_context.mapped_source);
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
