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

/// Reassigning the base variable ends the narrowing: after `$arg` holds a
/// different object, `$arg->value` is back to what the property declares,
/// so the narrowed type's own members are not offered.
#[tokio::test]
async fn reassigned_base_variable_drops_property_completion() {
    let text = concat!(
        "<?php\n",
        "interface Expr {\n",
        "    public function onEveryExpr(): void;\n",
        "}\n",
        "class StringExpr implements Expr {\n",
        "    public function onEveryExpr(): void {}\n",
        "    public function onlyOnString(): void {}\n",
        "}\n",
        "class Arg {\n",
        "    public Expr $value;\n",
        "}\n",
        "class C {\n",
        "    public function m(Arg $arg, Arg $other): void {\n",
        "        if ($arg->value instanceof StringExpr) {\n",
        "            $arg = $other;\n",
        "            $arg->value->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    // Line 15 (0-indexed), after `$arg->value->` = 12 + 13 = 25.
    let methods = completion_methods(text, 15, 25).await;
    assert!(
        methods.iter().any(|m| m == "onEveryExpr"),
        "The declared property type's members should still be offered, \
         got: {methods:?}"
    );
    assert!(
        !methods.iter().any(|m| m == "onlyOnString"),
        "A check that ran before `$arg` was replaced must not narrow the \
         property after it, got: {methods:?}"
    );
}

/// A property checked by the left operand of `&&` is narrowed in the right
/// one, and a call that changes the object between them ends that.
#[tokio::test]
async fn and_operand_narrows_a_property_for_the_next_operand() {
    let text = concat!(
        "<?php\n",
        "class Cat {\n",
        "    public function purr(): bool { return true; }\n",
        "}\n",
        "class Dog {\n",
        "    public function bark(): bool { return true; }\n",
        "}\n",
        "class C {\n",
        "    public Cat|Dog $pet;\n",
        "    public function reset(): void {}\n",
        "    public function m(): void {\n",
        "        if ($this->pet instanceof Cat && $this->pet->purr()) {}\n",
        "        if ($this->pet instanceof Cat && $this->reset() === null && $this->pet->purr()) {}\n",
        "    }\n",
        "}\n",
    );
    // Line 11, after the second `$this->pet->` = 8 + 45 = 53.
    let methods = completion_methods(text, 11, 53).await;
    assert!(
        methods.iter().any(|m| m == "purr") && !methods.iter().any(|m| m == "bark"),
        "The left operand proved a Cat, got: {methods:?}"
    );
    // Line 12, after the last `$this->pet->` = 8 + 72 = 80.
    let methods = completion_methods(text, 12, 80).await;
    assert!(
        methods.iter().any(|m| m == "purr") && methods.iter().any(|m| m == "bark"),
        "`reset()` returns nothing, so it may have replaced the pet, got: {methods:?}"
    );
}
