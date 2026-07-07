use crate::common::create_test_backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// The class under test declares several magic methods plus an ordinary
/// dunder-prefixed method and a couple of regular methods.
const SOURCE: &str = concat!(
    "<?php\n",
    "class X {\n",
    "    public function __invoke(): void {}\n",
    "    public function __test(): void {}\n",
    "    public function __toString(): string { return 'a'; }\n",
    "    public function apply(): void {}\n",
    "    public function zip(): void {}\n",
    "}\n",
    "$x = new X();\n",
    "$x->\n",
);

/// Implemented magic methods are offered in member completion, but sorted
/// after the regular methods so they never appear at the top of the list.
#[tokio::test]
async fn test_magic_methods_offered_but_sorted_last() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///magic_sort.php").unwrap();

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: SOURCE.to_string(),
            },
        })
        .await;

    // Cursor after `$x->` on line 9.
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 4,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let result = backend.completion(params).await.unwrap().unwrap();

    let items = match result {
        CompletionResponse::Array(items)
        | CompletionResponse::List(CompletionList { items, .. }) => items,
    };

    let method_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    // The implemented magic methods are offered.
    for name in ["__invoke", "__toString"] {
        assert!(
            method_names.contains(&name),
            "Implemented magic method {name} should be offered, got: {method_names:?}"
        );
    }
    // Regular methods (including the ordinary dunder method) are offered too.
    for name in ["apply", "zip", "__test"] {
        assert!(
            method_names.contains(&name),
            "Method {name} should be offered, got: {method_names:?}"
        );
    }

    // Every regular method must sort before every magic method, so the magic
    // methods land at the bottom of the popup rather than the top.
    let sort_text = |name: &str| -> String {
        items
            .iter()
            .find(|i| i.filter_text.as_deref() == Some(name))
            .and_then(|i| i.sort_text.clone())
            .unwrap_or_default()
    };
    let last_regular = ["apply", "zip", "__test"]
        .iter()
        .map(|n| sort_text(n))
        .max()
        .unwrap();
    let first_magic = ["__invoke", "__toString"]
        .iter()
        .map(|n| sort_text(n))
        .min()
        .unwrap();
    assert!(
        last_regular < first_magic,
        "All regular methods should sort before the magic methods"
    );
}
