#[cfg(test)]
mod tests {
    use crate::common::create_test_backend;
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    /// Open a Blade file and collect syntax-error diagnostics for it.
    fn blade_syntax_errors(uri: &str, blade_text: &str) -> Vec<Diagnostic> {
        let backend = phpantom_lsp::Backend::new_test();
        backend.update_ast(uri, blade_text);
        let mut out = Vec::new();
        backend.collect_syntax_error_diagnostics(uri, blade_text, &mut out);
        out
    }

    #[tokio::test]
    async fn test_blade_regression_sentry() {
        let backend = create_test_backend();
        let blade_uri = Url::parse("file:///sentry.blade.php").unwrap();
        let blade_text =
            std::fs::read_to_string("tests/fixtures/blade_regression_1.blade.php").unwrap();

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 1,
                    character: 1,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let _ = backend.goto_definition(params).await.unwrap();

        let (virtual_php, _) = phpantom_lsp::blade::preprocessor::preprocess(&blade_text);
        println!("VIRTUAL PHP SENTRY:\n{}", virtual_php);
    }

    #[tokio::test]
    async fn test_blade_regression_sitemap() {
        let backend = create_test_backend();
        let blade_uri = Url::parse("file:///sitemap.blade.php").unwrap();
        let blade_text =
            std::fs::read_to_string("tests/fixtures/blade_regression_2.blade.php").unwrap();

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: blade_uri.clone(),
                },
                position: Position {
                    line: 1,
                    character: 1,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let _ = backend.goto_definition(params).await.unwrap();

        // Print the preprocessed PHP
        let (virtual_php, _) = phpantom_lsp::blade::preprocessor::preprocess(&blade_text);
        println!("VIRTUAL PHP:\n{}", virtual_php);
    }

    /// A raw `<?php ... ?>` block embedded directly in a Blade template
    /// (not wrapped in `@php`/`@endphp`) must be passed through verbatim.
    /// A string literal that happens to start with `@` (e.g. a JSON-LD
    /// `'@context'` array key) must not be misread as a Blade directive.
    #[tokio::test]
    async fn test_blade_regression_raw_php_tag_with_at_prefixed_string() {
        let blade_text =
            std::fs::read_to_string("tests/fixtures/blade_regression_3.blade.php").unwrap();

        let diags = blade_syntax_errors("file:///schema.blade.php", &blade_text);
        assert!(
            diags.is_empty(),
            "Raw <?php ?> block should not produce syntax errors: {:?}",
            diags
        );
    }

    /// `@switch`/`@case`/`@break`/`@endswitch` must translate to a valid
    /// alternative-syntax `switch`, including when a `@case` argument is a
    /// fully-qualified class constant.
    #[tokio::test]
    async fn test_blade_regression_switch_case_with_class_constant() {
        let blade_text =
            std::fs::read_to_string("tests/fixtures/blade_regression_4.blade.php").unwrap();

        let diags = blade_syntax_errors("file:///membership.blade.php", &blade_text);
        assert!(
            diags.is_empty(),
            "@switch/@case with a class-constant argument should not produce syntax errors: {:?}",
            diags
        );
    }
}
