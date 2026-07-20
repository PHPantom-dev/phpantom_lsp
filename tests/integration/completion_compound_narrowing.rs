//! Completion after compound-condition and non-variable-subject
//! narrowing (the interactive counterpart of
//! `diagnostics_compound_narrowing`).

use crate::common::create_test_backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Open `text`, request completion at `(line, character)`, and return the
/// method names offered.
async fn completion_methods(text: &str, line: u32, character: u32) -> Vec<String> {
    let backend = create_test_backend();
    let uri = Url::parse("file:///compound.php").unwrap();
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

/// `||` guard clause narrows a property subject for completion.
#[tokio::test]
async fn or_guard_property_completion() {
    let text = concat!(
        "<?php\n",
        "interface Expr {}\n",
        "class StringExpr implements Expr {\n",
        "    public function onlyOnString(): void {}\n",
        "}\n",
        "class Arg {\n",
        "    public Expr $value;\n",
        "}\n",
        "class C {\n",
        "    public function m(Arg $arg): void {\n",
        "        if (! $arg instanceof Arg || ! $arg->value instanceof StringExpr) {\n",
        "            return;\n",
        "        }\n",
        "        $arg->value->\n",
        "    }\n",
        "}\n",
    );
    // Line 13 (0-indexed), after `$arg->value->` = 8 + 13 = 21.
    let methods = completion_methods(text, 13, 21).await;
    assert!(
        methods.iter().any(|m| m == "onlyOnString"),
        "Completion after the `||` guard should offer StringExpr methods, \
         got: {methods:?}"
    );
}

/// An untyped arrow-function parameter narrowed by an earlier `&&`
/// conjunct offers the narrowed type's members for completion.
#[tokio::test]
async fn arrow_fn_param_and_instanceof_completion() {
    let text = concat!(
        "<?php\n",
        "class Collection {\n",
        "    public function contains($x): bool { return true; }\n",
        "}\n",
        "class C {\n",
        "    public function m(): void {\n",
        "        $cb = fn($faqs) => $faqs instanceof Collection && $faqs->\n",
        "    }\n",
        "}\n",
    );
    // Line 6 (0-indexed), after `$faqs->` = column of the last `->`.
    let methods = completion_methods(text, 6, 65).await;
    assert!(
        methods.iter().any(|m| m == "contains"),
        "Completion after `$faqs instanceof Collection && $faqs->` should \
         offer Collection methods, got: {methods:?}"
    );
}

/// Integer-index guard clause narrows the element for completion.
#[tokio::test]
async fn integer_index_guard_completion() {
    let text = concat!(
        "<?php\n",
        "interface Expr {}\n",
        "class StringExpr implements Expr {\n",
        "    public function onlyOnString(): void {}\n",
        "}\n",
        "class C {\n",
        "    /** @param Expr[] $stmts */\n",
        "    public function m(array $stmts): void {\n",
        "        if (! $stmts[0] instanceof StringExpr) {\n",
        "            return;\n",
        "        }\n",
        "        $stmts[0]->\n",
        "    }\n",
        "}\n",
    );
    // Line 11 (0-indexed), after `$stmts[0]->` = 8 + 11 = 19.
    let methods = completion_methods(text, 11, 19).await;
    assert!(
        methods.iter().any(|m| m == "onlyOnString"),
        "Completion after the integer-index guard should offer StringExpr \
         methods, got: {methods:?}"
    );
}
