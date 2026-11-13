//! `textDocument/diagnostic` answers with the diagnostics of the text the
//! editor holds, including a pull sent right after an edit, as Neovim
//! sends one, before the server has re-parsed the edit.

#[cfg(test)]
mod tests {
    use crate::common::{APP_PSR4_COMPOSER, create_psr4_workspace, open_document};
    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::gen_lsp_types::*;

    async fn pull(
        backend: &phpantom_lsp::Backend,
        uri: &Uri,
        previous_result_id: Option<String>,
    ) -> DocumentDiagnosticReport {
        backend
            .diagnostic(DocumentDiagnosticParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                identifier: None,
                previous_result_id,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
    }

    fn full(report: DocumentDiagnosticReport) -> Option<FullDocumentDiagnosticReport> {
        match report {
            DocumentDiagnosticReport::RelatedFullDocumentDiagnosticReport(full) => {
                Some(full.full_document_diagnostic_report)
            }
            DocumentDiagnosticReport::RelatedUnchangedDocumentDiagnosticReport(_) => None,
        }
    }

    #[tokio::test]
    async fn a_pull_right_after_fixing_an_error_no_longer_reports_it() {
        let broken = "<?php\nnamespace App;\nclass Foo {\n";
        let (backend, dir) = create_psr4_workspace(APP_PSR4_COMPOSER, &[("app/Foo.php", broken)]);
        backend.initialized(InitializedParams {}).await;

        let uri = Uri::from_file_path(dir.path().join("app/Foo.php")).unwrap();
        open_document(&backend, &uri, "php", broken).await;
        let before = full(pull(&backend, &uri, None).await).expect("a first pull is full");
        assert!(
            !before.items.is_empty(),
            "the unclosed class is a syntax error"
        );

        backend
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    text_document_identifier: TextDocumentIdentifier { uri: uri.clone() },
                    version: 2,
                },
                content_changes: vec![
                    TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                        TextDocumentContentChangeWholeDocument {
                            text: "<?php\nnamespace App;\nclass Foo {}\n".to_string(),
                        },
                    ),
                ],
            })
            .await;
        let after = full(pull(&backend, &uri, before.result_id.clone()).await)
            .expect("a pull right after the edit must not answer Unchanged");
        assert!(
            after.items.is_empty(),
            "stale diagnostics: {:?}",
            after.items
        );
    }
}
