//! Integration tests: completion suppression inside non-doc comments.
//!
//! Verifies that the LSP returns **no** completions when the cursor is
//! inside a `//` line comment or a `/* … */` block comment, while still
//! returning completions inside `/** … */` docblocks and normal code.

use crate::common::{create_test_backend, inject_phpstan_diag};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file and request completion at the given line/character.
/// Returns `None` when the server returns `Ok(None)` (i.e. no completions
/// at all), and `Some(items)` otherwise.
async fn complete_at_raw(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Option<Vec<CompletionItem>> {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    match backend.completion(completion_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => Some(items),
        Some(CompletionResponse::List(list)) => Some(list.items),
        None => None,
    }
}

// ─── Line comment suppression ───────────────────────────────────────────────

#[tokio::test]
async fn no_completion_inside_line_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_line.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "// $f->\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 5, 7).await;
    assert!(
        result.is_none(),
        "Should return no completions inside a // line comment, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_completion_inside_line_comment_partial_word() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_line_word.php").unwrap();
    let text = concat!("<?php\n", "// Foo\n",);

    let result = complete_at_raw(&backend, &uri, text, 1, 5).await;
    assert!(
        result.is_none(),
        "Should return no completions for class names inside // comment, got: {:?}",
        result
    );
}

#[tokio::test]
async fn completion_inside_phpstan_ignore_identifier_list() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpstan_ignore_completion.php").unwrap();
    let text = concat!("<?php\n", "// @phpstan-ignore ret\n");
    inject_phpstan_diag(
        &backend,
        uri.as_str(),
        1,
        "Function should return string but returns int.",
        "return.type",
    );

    let result = complete_at_raw(&backend, &uri, text, 1, 22).await;
    let items = result.expect("expected phpstan-ignore completions");
    assert!(
        items.iter().any(|item| item.label == "return.type"),
        "expected return.type completion, got: {:?}",
        items.iter().map(|item| &item.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn no_completion_inside_phpstan_ignore_code_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///phpstan_ignore_reason_completion.php").unwrap();
    let text = concat!(
        "<?php\n",
        "// @phpstan-ignore return.type (accepts broader input)\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 1, 53).await;
    assert!(
        result.is_none(),
        "should not complete inside a per-code phpstan-ignore reason"
    );
}

#[tokio::test]
async fn no_completion_inside_trailing_line_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_trailing.php").unwrap();
    let text = concat!("<?php\n", "$x = 1; // $x->\n",);

    let result = complete_at_raw(&backend, &uri, text, 1, 15).await;
    assert!(
        result.is_none(),
        "Should return no completions inside trailing // comment, got: {:?}",
        result
    );
}

// ─── Block comment suppression ──────────────────────────────────────────────

#[tokio::test]
async fn no_completion_inside_block_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_block.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "/* $f->bar() */\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 4, 7).await;
    assert!(
        result.is_none(),
        "Should return no completions inside a /* */ block comment, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_completion_inside_multiline_block_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_block_multi.php").unwrap();
    let text = concat!("<?php\n", "/*\n", " * Some notes\n", " * Foo\n", " */\n",);

    let result = complete_at_raw(&backend, &uri, text, 3, 5).await;
    assert!(
        result.is_none(),
        "Should return no completions inside a multiline /* */ comment, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_completion_inside_block_comment_at_tag() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_block_tag.php").unwrap();
    // `@param` inside a `/* */` (not `/** */`) should NOT trigger PHPDoc completion
    let text = concat!("<?php\n", "/* @param */\n",);

    let result = complete_at_raw(&backend, &uri, text, 1, 5).await;
    assert!(
        result.is_none(),
        "Should return no completions for @tags inside /* */ comment, got: {:?}",
        result
    );
}

// ─── Docblocks still work ───────────────────────────────────────────────────

#[tokio::test]
async fn completion_still_works_inside_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_docblock.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function foo(): void {}\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 2, 4).await;
    assert!(
        result.is_some(),
        "Should still provide completions inside /** */ docblocks"
    );
    let items = result.unwrap();
    assert!(
        !items.is_empty(),
        "Docblock tag completions should not be empty"
    );
}

// ─── Normal code still works ────────────────────────────────────────────────

#[tokio::test]
async fn completion_still_works_in_normal_code() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_normal.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$f->\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 5, 4).await;
    assert!(
        result.is_some(),
        "Should provide completions in normal code"
    );
    let items = result.unwrap();
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Should suggest 'bar' method in normal code. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn completion_works_after_line_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_after_line.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "// this is a comment\n",
        "$f = new Foo();\n",
        "$f->\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 6, 4).await;
    assert!(
        result.is_some(),
        "Should provide completions on the line after a // comment"
    );
    let items = result.unwrap();
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Should suggest 'bar' after a // comment line. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn completion_works_after_block_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_after_block.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "/* block comment */\n",
        "$f = new Foo();\n",
        "$f->\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 6, 4).await;
    assert!(
        result.is_some(),
        "Should provide completions after a /* */ block comment"
    );
    let items = result.unwrap();
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Should suggest 'bar' after a /* */ comment. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ─── Edge cases: comments inside strings ────────────────────────────────────

#[tokio::test]
async fn completion_works_when_comment_syntax_in_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_in_string.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$s = '// not a comment';\n",
        "$f = new Foo();\n",
        "$f->\n",
    );

    // Cursor is in normal code after a string that contains `//`
    let result = complete_at_raw(&backend, &uri, text, 6, 4).await;
    assert!(
        result.is_some(),
        "Comment syntax inside a string should not suppress later completions"
    );
    let items = result.unwrap();
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Should still suggest 'bar'. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ─── Variable completion inside comments ────────────────────────────────────

#[tokio::test]
async fn no_variable_completion_inside_line_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_var_line.php").unwrap();
    let text = concat!("<?php\n", "$userName = 'Alice';\n", "// $us\n",);

    let result = complete_at_raw(&backend, &uri, text, 2, 5).await;
    assert!(
        result.is_none(),
        "Should not suggest variable names inside // comment, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_variable_completion_inside_block_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_var_block.php").unwrap();
    let text = concat!("<?php\n", "$userName = 'Alice';\n", "/* $us */\n",);

    let result = complete_at_raw(&backend, &uri, text, 2, 5).await;
    assert!(
        result.is_none(),
        "Should not suggest variable names inside /* */ comment, got: {:?}",
        result
    );
}

// ─── Class name completion inside comments ──────────────────────────────────

#[tokio::test]
async fn no_class_name_completion_inside_line_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_class_line.php").unwrap();
    let text = concat!("<?php\n", "class MyService {}\n", "// MyS\n",);

    let result = complete_at_raw(&backend, &uri, text, 2, 5).await;
    assert!(
        result.is_none(),
        "Should not suggest class names inside // comment, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_class_name_completion_inside_block_comment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///comment_class_block.php").unwrap();
    let text = concat!("<?php\n", "class MyService {}\n", "/* MyS */\n",);

    let result = complete_at_raw(&backend, &uri, text, 2, 5).await;
    assert!(
        result.is_none(),
        "Should not suggest class names inside /* */ comment, got: {:?}",
        result
    );
}

// ─── Docblock suppression (non-tag positions) ──────────────────────────────

#[tokio::test]
async fn no_class_completion_inside_docblock_description() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_desc_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {}\n",
        "/**\n",
        " * This class uses Foo\n",
        " */\n",
        "class Bar {}\n",
    );

    // Cursor on "Foo" in the description line — should NOT trigger class completion
    let result = complete_at_raw(&backend, &uri, text, 3, 20).await;
    assert!(
        result.is_none(),
        "Should not suggest class names inside docblock description, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_variable_completion_inside_docblock_description() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_desc_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$userName = 'Alice';\n",
        "/**\n",
        " * Uses $us\n",
        " */\n",
        "function foo(): void {}\n",
    );

    // Cursor on "$us" in the description — should NOT trigger variable completion
    let result = complete_at_raw(&backend, &uri, text, 3, 11).await;
    assert!(
        result.is_none(),
        "Should not suggest variables inside docblock description, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_member_completion_inside_docblock_description() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_desc_member.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "/**\n",
        " * Example: $foo->bar()\n",
        " */\n",
        "function baz(): void {}\n",
    );

    // Cursor after "->" in the description — should NOT trigger member completion
    let result = complete_at_raw(&backend, &uri, text, 5, 20).await;
    assert!(
        result.is_none(),
        "Should not suggest members inside docblock description, got: {:?}",
        result
    );
}

#[tokio::test]
async fn docblock_at_tag_still_completes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_at_tag.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function foo(): void {}\n",
    );

    // Cursor at `@` — PHPDoc tag completion should still fire
    let result = complete_at_raw(&backend, &uri, text, 2, 4).await;
    assert!(
        result.is_some(),
        "Should provide PHPDoc tag completions at @ inside docblock"
    );
    let items = result.unwrap();
    assert!(
        !items.is_empty(),
        "PHPDoc tag completions should not be empty at @"
    );
}

#[tokio::test]
async fn code_after_docblock_still_completes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_then_code.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "/**\n",
        " * A docblock.\n",
        " */\n",
        "$f = new Foo();\n",
        "$f->\n",
    );

    // Cursor in normal code after the docblock — completions should work
    let result = complete_at_raw(&backend, &uri, text, 8, 4).await;
    assert!(
        result.is_some(),
        "Should provide completions in code after a docblock"
    );
    let items = result.unwrap();
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Should suggest 'bar' in code after docblock. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ─── Docblock type completion at tag positions ──────────────────────────────

#[tokio::test]
async fn docblock_param_type_completion() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_param_type.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class UserService {}\n",
        "class UserRepository {}\n",
        "/**\n",
        " * @param User\n",
        " */\n",
        "function foo(UserService $s): void {}\n",
    );

    // Cursor after `@param User` — should offer class name completions
    let result = complete_at_raw(&backend, &uri, text, 4, 14).await;
    assert!(
        result.is_some(),
        "Should provide type completions after @param"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("UserService")),
        "Should suggest UserService after @param User. Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_param_empty_type_completion() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_param_empty_type.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "class MyService {}\n",
        "/**\n",
        " * @param \n",
        " */\n",
        "function foo(MyService $s): void {}\n",
    );

    // Cursor right after `@param ` with empty partial — should still offer types
    let result = complete_at_raw(&backend, &uri, text, 4, 10).await;
    assert!(
        result.is_some(),
        "Should provide type completions after '@param ' with empty partial"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("MyService")),
        "Should suggest MyService after '@param '. Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_return_type_completion() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_return_type.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Collection {}\n",
        "/**\n",
        " * @return Coll\n",
        " */\n",
        "function foo(): Collection {}\n",
    );

    // Cursor after `@return Coll`
    let result = complete_at_raw(&backend, &uri, text, 3, 15).await;
    assert!(
        result.is_some(),
        "Should provide type completions after @return"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("Collection")),
        "Should suggest Collection after @return Coll. Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_throws_type_completion() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_throws_type.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class InvalidArgumentException extends \\Exception {}\n",
        "/**\n",
        " * @throws Invalid\n",
        " */\n",
        "function foo(): void {}\n",
    );

    // Cursor after `@throws Invalid`
    let result = complete_at_raw(&backend, &uri, text, 3, 18).await;
    assert!(
        result.is_some(),
        "Should provide type completions after @throws"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels
            .iter()
            .any(|l| l.contains("InvalidArgumentException")),
        "Should suggest InvalidArgumentException after @throws Invalid. Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_var_type_completion() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_var_type.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class DateTime {}\n",
        "/**\n",
        " * @var Date\n",
        " */\n",
    );

    // Cursor after `@var Date`
    let result = complete_at_raw(&backend, &uri, text, 3, 12).await;
    assert!(
        result.is_some(),
        "Should provide type completions after @var"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("DateTime")),
        "Should suggest DateTime after @var Date. Got: {:?}",
        labels
    );
}

// ─── Docblock variable completion at $parameter positions ───────────────────

#[tokio::test]
async fn docblock_param_variable_completion() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_param_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $\n",
        " */\n",
        "function greet(string $name, int $age): void {}\n",
    );

    // Cursor after `@param string $`
    let result = complete_at_raw(&backend, &uri, text, 2, 18).await;
    assert!(
        result.is_some(),
        "Should provide variable completions after @param Type $"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"$name"),
        "Should suggest $name. Got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"$age"),
        "Should suggest $age. Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_param_variable_partial_filter() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_param_var_filter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $na\n",
        " */\n",
        "function greet(string $name, int $age): void {}\n",
    );

    // Cursor after `@param string $na` — should filter to $name only
    let result = complete_at_raw(&backend, &uri, text, 2, 20).await;
    assert!(
        result.is_some(),
        "Should provide filtered variable completions"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"$name"),
        "Should suggest $name for partial '$na'. Got: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"$age"),
        "Should NOT suggest $age for partial '$na'. Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_param_variable_empty_offers_all() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_param_var_all.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string \n",
        " */\n",
        "function greet(string $name, int $age): void {}\n",
    );

    // Cursor after `@param string ` (no $ yet) — should offer all params
    let result = complete_at_raw(&backend, &uri, text, 2, 17).await;
    assert!(
        result.is_some(),
        "Should provide all variable completions with empty partial"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"$name") && labels.contains(&"$age"),
        "Should suggest both $name and $age. Got: {:?}",
        labels
    );
}

// ─── Docblock union type completion ─────────────────────────────────────────

#[tokio::test]
async fn docblock_union_type_second_member() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_union.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class UserService {}\n",
        "/**\n",
        " * @param string|User\n",
        " */\n",
        "function foo($x): void {}\n",
    );

    // Cursor after `string|User` — partial is "User"
    let result = complete_at_raw(&backend, &uri, text, 3, 21).await;
    assert!(
        result.is_some(),
        "Should provide type completions after pipe in union type"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("UserService")),
        "Should suggest UserService for partial 'User'. Got: {:?}",
        labels
    );
}

// ─── Docblock generic type completion inside angle brackets ─────────────────

#[tokio::test]
async fn docblock_generic_type_inside_angle_brackets() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_generic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class UserModel {}\n",
        "class Collection {}\n",
        "/**\n",
        " * @return Collection<User\n",
        " */\n",
        "function foo() {}\n",
    );

    // Cursor inside `Collection<User` — partial is "User"
    let result = complete_at_raw(&backend, &uri, text, 4, 26).await;
    assert!(
        result.is_some(),
        "Should provide type completions inside generic angle brackets"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("UserModel")),
        "Should suggest UserModel inside Collection<User. Got: {:?}",
        labels
    );
}

// ─── Docblock description still suppressed ──────────────────────────────────

#[tokio::test]
async fn docblock_return_description_no_completion() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_return_desc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {}\n",
        "/**\n",
        " * @return string the Foo name\n",
        " */\n",
        "function foo(): string {}\n",
    );

    // Cursor on "Foo" in the description part after `@return string`
    let result = complete_at_raw(&backend, &uri, text, 3, 27).await;
    assert!(
        result.is_none(),
        "Should NOT provide completions in @return description text, got: {:?}",
        result
    );
}

#[tokio::test]
async fn docblock_param_description_no_completion() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_param_desc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {}\n",
        "/**\n",
        " * @param string $name the Foo owner\n",
        " */\n",
        "function foo(string $name): void {}\n",
    );

    // Cursor on "Foo" in the description after `@param string $name`
    let result = complete_at_raw(&backend, &uri, text, 3, 34).await;
    assert!(
        result.is_none(),
        "Should NOT provide completions in @param description text, got: {:?}",
        result
    );
}

// ─── Variable completion uses text_edit to prevent double-dollar ─────────────

#[tokio::test]
async fn docblock_param_variable_insert_text_always_includes_dollar() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_param_var_insert.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $\n",
        " */\n",
        "function greet(string $name): void {}\n",
    );

    // User already typed `$` — completion must use text_edit with an explicit
    // replacement range covering the `$` prefix to prevent double-dollar in
    // editors like Helix and Neovim.
    let result = complete_at_raw(&backend, &uri, text, 2, 18).await;
    assert!(result.is_some());
    let items = result.unwrap();
    let name_item = items.iter().find(|i| i.label == "$name");
    assert!(name_item.is_some(), "Should have a $name completion item");
    let item = name_item.unwrap();
    match &item.text_edit {
        Some(CompletionTextEdit::Edit(te)) => {
            assert_eq!(te.new_text, "$name", "text_edit new_text should be $name");
            // Range must cover the typed `$` at col 17..18.
            assert_eq!(te.range.start, Position::new(2, 17));
            assert_eq!(te.range.end, Position::new(2, 18));
        }
        other => panic!(
            "Expected text_edit with Edit variant to prevent double-dollar, got: {:?}",
            other
        ),
    }
    assert!(
        item.insert_text.is_none(),
        "insert_text should be None when text_edit is set"
    );
    assert_eq!(
        item.kind,
        Some(CompletionItemKind::VARIABLE),
        "Kind should be VARIABLE"
    );
}

#[tokio::test]
async fn docblock_param_variable_insert_text_includes_dollar_when_not_typed() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_param_var_insert_no_dollar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string \n",
        " */\n",
        "function greet(string $name): void {}\n",
    );

    // User has NOT typed `$` yet — text_edit new_text should still be "$name"
    // and the replacement range should be empty (start == end == cursor).
    let result = complete_at_raw(&backend, &uri, text, 2, 17).await;
    assert!(result.is_some());
    let items = result.unwrap();
    let name_item = items.iter().find(|i| i.label == "$name");
    assert!(name_item.is_some(), "Should have a $name completion item");
    let item = name_item.unwrap();
    match &item.text_edit {
        Some(CompletionTextEdit::Edit(te)) => {
            assert_eq!(
                te.new_text, "$name",
                "text_edit new_text should include the $ prefix"
            );
            // Empty prefix → range start == end == cursor position.
            assert_eq!(te.range.start, Position::new(2, 17));
            assert_eq!(te.range.end, Position::new(2, 17));
        }
        other => panic!("Expected text_edit with Edit variant, got: {:?}", other),
    }
}

// ─── Scalar / built-in type completion in docblocks ─────────────────────────

#[tokio::test]
async fn docblock_param_suggests_scalar_types() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_scalar_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param str\n",
        " */\n",
        "function foo(string $x): void {}\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 2, 13).await;
    assert!(
        result.is_some(),
        "Should provide completions after @param str"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"string"),
        "Should suggest 'string' scalar type after @param str. Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_return_suggests_scalar_types() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_scalar_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @return arr\n",
        " */\n",
        "function foo(): array {}\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 2, 14).await;
    assert!(
        result.is_some(),
        "Should provide completions after @return arr"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"array"),
        "Should suggest 'array' scalar type after @return arr. Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_param_suggests_int_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_scalar_int.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param in\n",
        " */\n",
        "function foo(int $x): void {}\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 2, 12).await;
    assert!(
        result.is_some(),
        "Should provide completions after @param in"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"int"),
        "Should suggest 'int' scalar type. Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_type_empty_partial_suggests_scalars_and_classes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_scalar_all.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "class MyService {}\n",
        "/**\n",
        " * @param \n",
        " */\n",
        "function foo(MyService $s): void {}\n",
    );

    // Empty partial — should offer both scalar types and class names
    let result = complete_at_raw(&backend, &uri, text, 4, 10).await;
    assert!(
        result.is_some(),
        "Should provide completions with empty partial"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"string"),
        "Should suggest 'string' with empty partial. Got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"int"),
        "Should suggest 'int' with empty partial. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.contains("MyService")),
        "Should also suggest class names (App\\MyService). Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_scalar_type_has_keyword_kind() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_scalar_kind.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @return string\n",
        " */\n",
        "function foo(): string {}\n",
    );

    // Partial "string" — the scalar item should have KEYWORD kind
    let result = complete_at_raw(&backend, &uri, text, 2, 17).await;
    assert!(result.is_some());
    let items = result.unwrap();
    let string_item = items.iter().find(|i| i.label == "string");
    assert!(string_item.is_some(), "Should have a 'string' item");
    assert_eq!(
        string_item.unwrap().kind,
        Some(CompletionItemKind::KEYWORD),
        "Scalar types should have KEYWORD kind"
    );
}

#[tokio::test]
async fn docblock_var_suggests_scalar_types() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_scalar_var.php").unwrap();
    let text = concat!("<?php\n", "/**\n", " * @var bo\n", " */\n",);

    let result = complete_at_raw(&backend, &uri, text, 2, 10).await;
    assert!(result.is_some(), "Should provide completions after @var bo");
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"bool"),
        "Should suggest 'bool' after @var bo. Got: {:?}",
        labels
    );
}

#[tokio::test]
async fn docblock_throws_suggests_scalar_types_and_classes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///docblock_throws_scalar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class RuntimeException extends \\Exception {}\n",
        "/**\n",
        " * @throws Run\n",
        " */\n",
        "function foo(): void {}\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 3, 14).await;
    assert!(
        result.is_some(),
        "Should provide completions after @throws Run"
    );
    let items = result.unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("RuntimeException")),
        "Should suggest RuntimeException. Got: {:?}",
        labels
    );
}
