//! Completion after narrowing carried by a boolean variable (the
//! interactive counterpart of `diagnostics_assertion_variables`).

use crate::common::create_test_backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Open `text`, request completion at `(line, character)`, and return the
/// method names offered.
async fn completion_methods(text: &str, line: u32, character: u32) -> Vec<String> {
    let backend = create_test_backend();
    let uri = Url::parse("file:///assertion_variable.php").unwrap();
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
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    match result {
        Some(CompletionResponse::Array(items)) => items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .filter_map(|i| i.filter_text.clone())
            .collect(),
        _ => Vec::new(),
    }
}

/// The boolean carries the check into the ternary's then-branch.
#[tokio::test]
async fn assertion_variable_completion_in_ternary() {
    let text = concat!(
        "<?php\n",
        "interface Renderable {}\n",
        "class HtmlString implements Renderable {\n",
        "    public function toHtml(): string { return ''; }\n",
        "}\n",
        "class C {\n",
        "    public function m(Renderable $raw): string {\n",
        "        $isHtml = $raw instanceof HtmlString;\n",
        "        $out = $isHtml ? $raw->toHtml() : '';\n",
        "        return $out;\n",
        "    }\n",
        "}\n",
    );
    // Line 8 (0-indexed), after `$raw->` = 25 + 6 = 31.
    let methods = completion_methods(text, 8, 31).await;
    assert!(
        methods.iter().any(|m| m == "toHtml"),
        "Completion in the ternary's then-branch should offer HtmlString \
         methods, got: {methods:?}"
    );
}

/// The boolean carries the check into an `if` body.
#[tokio::test]
async fn assertion_variable_completion_in_if_body() {
    let text = concat!(
        "<?php\n",
        "interface Renderable {}\n",
        "class HtmlString implements Renderable {\n",
        "    public function toHtml(): string { return ''; }\n",
        "}\n",
        "class C {\n",
        "    public function m(Renderable $raw): string {\n",
        "        $isHtml = $raw instanceof HtmlString;\n",
        "        if ($isHtml) {\n",
        "            $raw->\n",
        "        }\n",
        "        return '';\n",
        "    }\n",
        "}\n",
    );
    // Line 9 (0-indexed), after `$raw->` = 12 + 6 = 18.
    let methods = completion_methods(text, 9, 18).await;
    assert!(
        methods.iter().any(|m| m == "toHtml"),
        "Completion inside `if ($isHtml)` should offer HtmlString methods, \
         got: {methods:?}"
    );
}
