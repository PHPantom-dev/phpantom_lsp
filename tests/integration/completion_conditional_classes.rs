use crate::common::create_test_backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// `$this->` inside a method of a class declared within a top-level `if`
/// version guard must resolve to the class's own members. Such classes are a
/// common polyfill/compat pattern (e.g. defining a class differently for
/// different runtime versions), and the enclosing-class detection must descend
/// into the conditional block to associate the cursor with the class.
#[tokio::test]
async fn test_completion_this_inside_conditional_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///conditional_class_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "if (\\PHP_VERSION_ID >= 80000) {\n",
        "    class Compat {\n",
        "        public string $label;\n",
        "        public function describe(): string { return ''; }\n",
        "        public function run() {\n",
        "            $this->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 6,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    let items = match result.expect("should return completion results") {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    let labels: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(
        labels.contains(&"describe"),
        "method of conditionally-defined class should complete, got {labels:?}",
    );
    assert!(
        labels.contains(&"label"),
        "property of conditionally-defined class should complete, got {labels:?}",
    );
}
