#[cfg(test)]
mod tests {
    use crate::common::{apply_edits, create_test_backend, open_document};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    async fn format_range(uri: &str, text: &str, start_line: u32, end_line: u32) -> String {
        let backend = create_test_backend();
        let uri = Url::parse(uri).unwrap();
        open_document(&backend, &uri, "php", text).await;

        let edits = backend
            .range_formatting(DocumentRangeFormattingParams {
                text_document: TextDocumentIdentifier { uri },
                range: Range {
                    start: Position::new(start_line, 0),
                    end: Position::new(end_line + 1, 0),
                },
                options: FormattingOptions {
                    tab_size: 4,
                    insert_spaces: true,
                    ..FormattingOptions::default()
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap()
            .unwrap_or_default();
        apply_edits(text, &edits)
    }

    #[tokio::test]
    async fn test_range_formatting_formats_only_the_selected_function() {
        let text = concat!(
            "<?php\n",
            "\n",
            "function a( $x ){\n",
            "return $x;\n",
            "}\n",
            "\n",
            "function b( $y ){\n",
            "return $y;\n",
            "}\n",
        );

        let result = format_range("file:///range.php", text, 6, 8).await;

        assert_eq!(
            result,
            concat!(
                "<?php\n",
                "\n",
                "function a( $x ){\n",
                "return $x;\n",
                "}\n",
                "\n",
                "function b($y)\n",
                "{\n",
                "    return $y;\n",
                "}\n",
            )
        );
    }

    #[tokio::test]
    async fn test_range_formatting_reindents_one_line_of_a_block() {
        let text = concat!(
            "<?php\n",
            "\n",
            "function a($x)\n",
            "{\n",
            "$y = $x;\n",
            "return $y;\n",
            "}\n",
        );

        let result = format_range("file:///range.php", text, 5, 5).await;

        assert_eq!(
            result,
            concat!(
                "<?php\n",
                "\n",
                "function a($x)\n",
                "{\n",
                "$y = $x;\n",
                "    return $y;\n",
                "}\n",
            )
        );
    }

    #[tokio::test]
    async fn test_range_formatting_blade_template() {
        let text = "@if($x)\n<p>a</p>\n<p>b</p>\n@endif\n";

        let result = format_range("file:///resources/views/view.blade.php", text, 2, 2).await;

        assert_eq!(result, "@if($x)\n<p>a</p>\n    <p>b</p>\n@endif\n");
    }

    #[tokio::test]
    async fn test_range_formatting_already_formatted_returns_nothing() {
        let text = "<?php\n\nfunction a($x)\n{\n    return $x;\n}\n";

        let result = format_range("file:///range.php", text, 2, 5).await;

        assert_eq!(result, text);
    }
}
