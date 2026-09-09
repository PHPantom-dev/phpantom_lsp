#[cfg(test)]
mod tests {
    use crate::common::{create_test_backend, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    async fn format_with(
        backend: &phpantom_lsp::Backend,
        uri: Url,
        language_id: &str,
        text: &str,
        options: FormattingOptions,
    ) -> Option<Vec<TextEdit>> {
        open_document(backend, &uri, language_id, text).await;

        backend
            .formatting(DocumentFormattingParams {
                text_document: TextDocumentIdentifier { uri },
                options,
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap()
    }

    async fn format(
        backend: &phpantom_lsp::Backend,
        uri: Url,
        language_id: &str,
        text: &str,
    ) -> Option<Vec<TextEdit>> {
        format_with(
            backend,
            uri,
            language_id,
            text,
            FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                ..FormattingOptions::default()
            },
        )
        .await
    }

    /// The single whole-document edit's replacement text.
    fn new_text(edits: Option<Vec<TextEdit>>) -> String {
        let edits = edits.expect("expected edits");
        assert_eq!(edits.len(), 1);
        edits.into_iter().next().unwrap().new_text
    }

    #[tokio::test]
    async fn test_blade_extension_is_reindented() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///resources/views/view.blade.php").unwrap();
        let text = "@if($x)\n<p>hello</p>\n@endif\n";

        let result = format(&backend, uri, "php", text).await;

        assert_eq!(new_text(result), "@if($x)\n    <p>hello</p>\n@endif\n");
    }

    #[tokio::test]
    async fn test_blade_language_id_without_extension_is_reindented() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///resources/views/view.php").unwrap();
        let text = "@if($x)\n<p>hello</p>\n@endif\n";

        let result = format(&backend, uri, "blade", text).await;

        assert_eq!(new_text(result), "@if($x)\n    <p>hello</p>\n@endif\n");
    }

    #[tokio::test]
    async fn test_blade_formatting_never_touches_line_content() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///resources/views/view.blade.php").unwrap();
        // The PHP formatter would rewrite this line; the Blade one may not.
        let text = "@php\necho   'hello' ;\n@endphp\n";

        let result = format(&backend, uri, "php", text).await;

        assert_eq!(new_text(result), "@php\n    echo   'hello' ;\n@endphp\n");
    }

    #[tokio::test]
    async fn test_blade_formatting_honours_editor_indent_options() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///resources/views/view.blade.php").unwrap();
        let text = "<div>\n<p>hello</p>\n</div>\n";

        let result = format_with(
            &backend,
            uri,
            "php",
            text,
            FormattingOptions {
                tab_size: 4,
                insert_spaces: false,
                ..FormattingOptions::default()
            },
        )
        .await;

        assert_eq!(new_text(result), "<div>\n\t<p>hello</p>\n</div>\n");
    }

    #[tokio::test]
    async fn test_already_formatted_blade_is_a_no_op() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///resources/views/view.blade.php").unwrap();
        let text = "<div>\n    <p>hello</p>\n</div>\n";

        let result = format(&backend, uri, "php", text).await;

        assert!(result.is_none(), "{result:?}");
    }

    #[tokio::test]
    async fn test_mail_templates_are_left_alone() {
        let backend = create_test_backend();
        let uri = Url::parse("file:///resources/views/mail/welcome.blade.php").unwrap();
        let text = "<x-mail::message>\n# Hello\n<x-mail::button :url=\"$url\">\nGo\n</x-mail::button>\n</x-mail::message>\n";

        let result = format(&backend, uri, "php", text).await;

        assert!(
            result.is_none(),
            "Markdown mail templates render their indentation: {result:?}"
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
