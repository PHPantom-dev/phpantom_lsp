use std::collections::HashSet;

use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CreateFile,
    CreateFileOptions, DocumentChangeOperation, DocumentChanges, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, Range, ResourceOp, TextDocumentEdit,
    TextEdit, WorkspaceEdit,
};

use crate::Backend;
use crate::framework::{FrameworkReferenceKind, SymfonySymbolKind};

impl Backend {
    pub(super) fn collect_create_symfony_template_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        actions: &mut Vec<CodeActionOrCommand>,
    ) {
        let mut seen = HashSet::new();
        for diagnostic in &params.context.diagnostics {
            if diagnostic.code.as_ref().and_then(|code| match code {
                tower_lsp::lsp_types::NumberOrString::String(code) => Some(code.as_str()),
                tower_lsp::lsp_types::NumberOrString::Number(_) => None,
            }) != Some("unknown_symfony_template")
            {
                continue;
            }

            let Some(reference) =
                self.framework_reference_at_position(uri, content, diagnostic.range.start)
            else {
                continue;
            };
            let FrameworkReferenceKind::SymfonySymbol {
                kind: SymfonySymbolKind::Template,
                name,
                declaration: false,
            } = reference.kind
            else {
                continue;
            };
            let Some(template_uri) = self.symfony_template_uri(&name) else {
                continue;
            };
            if !seen.insert(template_uri.clone()) {
                continue;
            }

            let operations = vec![
                DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                    uri: template_uri.clone(),
                    options: Some(CreateFileOptions {
                        overwrite: Some(false),
                        ignore_if_exists: Some(true),
                    }),
                    annotation_id: None,
                })),
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: template_uri,
                        version: None,
                    },
                    edits: vec![OneOf::Left(TextEdit {
                        range: Range {
                            start: Position::new(0, 0),
                            end: Position::new(0, 0),
                        },
                        new_text: format!("{{# {name} #}}\n"),
                    })],
                }),
            ];
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Create Twig template '{name}'"),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diagnostic.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: None,
                    document_changes: Some(DocumentChanges::Operations(operations)),
                    change_annotations: None,
                }),
                is_preferred: Some(true),
                ..Default::default()
            }));
        }
    }
}
