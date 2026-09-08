#[cfg(test)]
mod tests {
    use crate::common::{create_test_backend, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    async fn format(
        backend: &phpantom_lsp::Backend,
        uri: Url,
        language_id: &str,
        text: &str,
    ) -> Option<Vec<TextEdit>> {
        open_document(backend, &uri, language_id, text).await;

        backend
            .formatting(DocumentFormattingParams {
                text_document: TextDocumentIdentifier { uri },
                options: FormattingOptions::default(),
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_blade_extension_is_a_formatting_no_op() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///view.blade.php").unwrap();
        let text = "@if($x)\necho   'hello' ;\n@endif";

        let result = format(&backend, uri, "php", text).await;

        assert!(
            result.is_none(),
            "Formatting a .blade.php file should be a no-op"
        );
    }

    #[tokio::test]
    async fn test_blade_language_id_without_extension_is_a_formatting_no_op() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///view.php").unwrap();
        let text = "@if($x)\necho   'hello' ;\n@endif";

        let result = format(&backend, uri, "blade", text).await;

        assert!(
            result.is_none(),
            "Formatting a file opened with languageId 'blade' should be a no-op"
        );
    }

    #[tokio::test]
    async fn test_plain_php_file_still_formats() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///Plain.php").unwrap();
        let text = "<?php\necho   'hello' ;  \n";

        let result = format(&backend, uri, "php", text).await;

        assert!(
            result.is_some(),
            "Formatting an ordinary .php file should still produce edits"
        );
    }
}
