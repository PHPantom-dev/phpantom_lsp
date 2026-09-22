use crate::common::{complete_at, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Find the docblock generation item (the snippet item with filter_text "/**").
fn find_docblock_item(items: &[CompletionItem]) -> Option<&CompletionItem> {
    items.iter().find(|i| {
        i.filter_text.as_deref() == Some("/**")
            && i.insert_text_format == Some(InsertTextFormat::SNIPPET)
    })
}

/// Extract the snippet text from the completion item's text_edit.
fn snippet_text(item: &CompletionItem) -> &str {
    match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => &edit.new_text,
        _ => "",
    }
}

// ─── Basic trigger: fully-typed function (no enrichment needed) ─────────────

/// Typing `/**` above a function whose params and return are all scalars
/// should produce a summary-only skeleton (no @param, no @return).
#[tokio::test]
async fn generates_summary_only_for_fully_typed_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function greet(string $name, int $age): string {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>(),
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        !snippet.contains("@param"),
        "Fully-typed scalar params should NOT get @param, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@return"),
        "Scalar return type should NOT get @return, got:\n{}",
        snippet
    );
    assert!(snippet.starts_with("/**"), "Should start with /**");
    assert!(
        snippet.contains("${1}"),
        "Should have summary tab stop, got:\n{}",
        snippet
    );
}

/// Typing `/**` above a void function with fully-typed params should not
/// include @return, and scalar params should not get @param.
#[tokio::test]
async fn generates_summary_only_for_void_function_with_scalar_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_void.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function doStuff(int $count): void {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock generation item");

    let snippet = snippet_text(item.unwrap());
    assert!(
        !snippet.contains("@param"),
        "Scalar int param should NOT get @param, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@return"),
        "Void function should not have @return, got:\n{}",
        snippet
    );
}

/// Typing `/**` above a method with no params and no return type should
/// produce a docblock with @return (missing return type needs enrichment).
#[tokio::test]
async fn generates_return_for_missing_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_no_params.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "    function bar() {}\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock generation item");

    let snippet = snippet_text(item.unwrap());
    assert!(
        !snippet.contains("@param"),
        "No params means no @param tags"
    );
    assert!(
        snippet.contains("@return"),
        "Missing return type should get @return, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("mixed"),
        "Missing return type placeholder should be mixed, got:\n{}",
        snippet
    );
    assert!(snippet.starts_with("/**"), "Should start with /**");
    assert!(
        snippet.ends_with(" */"),
        "Should end with ' */', got:\n{}",
        snippet
    );
}

// ─── Untyped params get @param with mixed placeholder ───────────────────────

/// Untyped parameters should get @param with ${mixed} placeholder.
#[tokio::test]
async fn generates_param_for_untyped_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_untyped.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function process($data, string $name): void {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock generation item");

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "Untyped param should get @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$data"),
        "Should reference \\$data (escaped), got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("mixed"),
        "Untyped param should have mixed placeholder, got:\n{}",
        snippet
    );
    // The typed string $name should NOT get a @param tag.
    assert!(
        !snippet.contains("\\$name"),
        "Fully-typed $name should NOT appear in @param, got:\n{}",
        snippet
    );
}

// ─── Array params/return get enrichment ─────────────────────────────────────

/// `array` params and return types should get @param/@return enrichment.
#[tokio::test]
async fn generates_tags_for_array_params_and_return() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_array.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function process(array $items): array {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock generation item");

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "array param should get @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$items"),
        "Should reference \\$items (escaped), got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("@return"),
        "array return should get @return, got:\n{}",
        snippet
    );
}

// ─── Class-like declarations ────────────────────────────────────────────────

/// Typing `/**` above a class should produce a simple summary block.
#[tokio::test]
async fn generates_docblock_for_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_class.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "class MyService {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for class"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        !snippet.contains("@param"),
        "Class docblock should not have @param"
    );
    assert!(
        !snippet.contains("@return"),
        "Class docblock should not have @return"
    );
    assert!(snippet.starts_with("/**"), "Should start with /**");
}

/// Typing `/**` above an interface.
#[tokio::test]
async fn generates_docblock_for_interface() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_interface.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "interface Renderable {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for interface"
    );
}

/// Typing `/**` above an enum.
#[tokio::test]
async fn generates_docblock_for_enum() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_enum.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "enum Status: string {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for enum"
    );
}

/// Typing `/**` above a trait.
#[tokio::test]
async fn generates_docblock_for_trait() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_trait.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "trait Cacheable {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for trait"
    );
}

// ─── Properties ─────────────────────────────────────────────────────────────

/// Typed property should always include @var with the native type.
#[tokio::test]
async fn generates_var_for_typed_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_prop_typed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "    public string $name;\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for typed property"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@var string"),
        "Typed property should have @var string, got:\n{}",
        snippet
    );
}

/// Typing `/**` above an untyped property should include @var placeholder.
#[tokio::test]
async fn generates_docblock_for_untyped_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_prop_untyped.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "    public $name;\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for untyped property"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@var"),
        "Untyped property should include @var, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("mixed"),
        "Untyped property should have mixed placeholder, got:\n{}",
        snippet
    );
}

// ─── Constants ──────────────────────────────────────────────────────────────

/// Typing `/**` above an untyped constant should include @var with mixed.
#[tokio::test]
async fn generates_docblock_for_constant() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "    const MAX = 100;\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for constant"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@var"),
        "Constant should include @var, got:\n{}",
        snippet
    );
}

/// Typed constant should include the concrete type in @var.
#[tokio::test]
async fn generates_docblock_for_typed_constant() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_const_typed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "    public const int MAX = 100;\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for typed constant"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@var int"),
        "Typed constant should have @var int, got:\n{}",
        snippet
    );
}

// ─── Parameter types: scalars are skipped ───────────────────────────────────

/// Nullable parameter and return types are fully expressed — no enrichment.
#[tokio::test]
async fn skips_nullable_param_and_return() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_nullable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function test(?string $name): ?int {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some());

    let snippet = snippet_text(item.unwrap());
    assert!(
        !snippet.contains("@param"),
        "Nullable type is fully expressed, should NOT get @param, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@return"),
        "Nullable return is fully expressed, should NOT get @return, got:\n{}",
        snippet
    );
}

/// Union type params and return are fully expressed — no enrichment.
#[tokio::test]
async fn skips_union_param_and_return() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_union.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function test(string|int $value): string|false {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some());

    let snippet = snippet_text(item.unwrap());
    assert!(
        !snippet.contains("@param"),
        "Union type is fully expressed, should NOT get @param, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@return"),
        "Union return is fully expressed, should NOT get @return, got:\n{}",
        snippet
    );
}

/// Union types containing `array` should get @param enrichment.
#[tokio::test]
async fn generates_param_for_union_with_array() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_union_array.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function call(string $endpoint, array|string $params, array $headers): void {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some());

    let snippet = snippet_text(item.unwrap());
    // array|string contains array → needs enrichment.
    assert!(
        snippet.contains("@param"),
        "Union containing array should get @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("array<${1:mixed}>|string"),
        "Should enrich array part in union type, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$params"),
        "Should reference \\$params (escaped), got:\n{}",
        snippet
    );
    // bare `array` also needs enrichment.
    assert!(
        snippet.contains("\\$headers"),
        "Bare array param should get @param too, got:\n{}",
        snippet
    );
    // string $endpoint is scalar → no @param.
    assert!(
        !snippet.contains("\\$endpoint"),
        "Scalar string param should NOT appear, got:\n{}",
        snippet
    );
}

/// `Closure` param should get @param with callable signature placeholder.
#[tokio::test]
async fn generates_param_for_closure() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_closure.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function remember(Closure $callback): void {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some());

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "Closure param should get @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("Closure()"),
        "Should contain callable signature placeholder, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$callback"),
        "Should reference \\$callback (escaped), got:\n{}",
        snippet
    );
}

/// `callable` param should get @param with callable signature placeholder.
#[tokio::test]
async fn generates_param_for_callable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_callable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function register(callable $handler): void {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some());

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "callable param should get @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("callable()"),
        "Should contain callable signature placeholder, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$handler"),
        "Should reference \\$handler (escaped), got:\n{}",
        snippet
    );
}

/// Blank separator line between @param and @throws groups.
#[tokio::test]
async fn blank_separator_between_param_and_throws() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_sep.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class NotFoundException extends \\Exception {}\n",
        "/**\n",
        "function find(array $exclude): string {\n",
        "    throw new NotFoundException('not found');\n",
        "    return 'found';\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock generation item");

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "Should have @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("@throws"),
        "Should have @throws, got:\n{}",
        snippet
    );

    // There should be a blank `*` line between @param and @throws.
    let lines: Vec<&str> = snippet.lines().collect();
    let param_idx = lines.iter().position(|l| l.contains("@param")).unwrap();
    let throws_idx = lines.iter().position(|l| l.contains("@throws")).unwrap();
    assert_eq!(
        throws_idx,
        param_idx + 2,
        "@throws should be separated from @param by one blank * line, got:\n{}",
        snippet
    );
    assert_eq!(
        lines[param_idx + 1].trim(),
        "*",
        "Separator should be a bare *, got:\n{}",
        snippet
    );
}

/// Variadic params with scalar types should not get @param.
#[tokio::test]
async fn skips_variadic_scalar_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_variadic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function test(string $first, int ...$rest): void {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some());

    let snippet = snippet_text(item.unwrap());
    assert!(
        !snippet.contains("@param"),
        "Scalar variadic params should NOT get @param, got:\n{}",
        snippet
    );
}

// ─── Snippet format ─────────────────────────────────────────────────────────

/// The generated snippet uses tab stops (at least ${1} for summary).
#[tokio::test]
async fn snippet_has_tab_stops() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_tabstops.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "function test(string $name): int {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items).unwrap();

    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));

    let snippet = snippet_text(item);
    // Should contain at least ${1} for the summary line.
    assert!(
        snippet.contains("${1}"),
        "Should have tab stop for summary, got:\n{}",
        snippet
    );
}

/// The text_edit range covers exactly the `/**` text (no auto-close).
#[tokio::test]
async fn text_edit_covers_trigger() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_range.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "function test(): void {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items).unwrap();

    match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => {
            assert_eq!(edit.range.start.line, 1);
            assert_eq!(edit.range.start.character, 0);
            assert_eq!(edit.range.end.line, 1);
            assert_eq!(edit.range.end.character, 3);
        }
        other => panic!("Expected a TextEdit, got: {:?}", other),
    }
}

/// VS Code auto-closes `/**` into `/** */` — completion should still fire.
#[tokio::test]
async fn generates_docblock_with_autoclosed_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_vscode.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/** */\n",
        "function process($data, array $items): void {}\n",
    );
    // Cursor right after `/**` on line 1, col 3.
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock even with auto-closed /** */, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>(),
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "Should have @param tags, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$data"),
        "Should reference \\$data (escaped), got:\n{}",
        snippet
    );
}

/// When `/** */` is auto-closed, the text_edit range covers the full `/** */`.
#[tokio::test]
async fn text_edit_covers_autoclosed_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_range_ac.php").unwrap();
    let text = concat!("<?php\n", "/** */\n", "function test(): void {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items).unwrap();

    match &item.text_edit {
        Some(CompletionTextEdit::Edit(edit)) => {
            assert_eq!(edit.range.start.line, 1);
            assert_eq!(edit.range.start.character, 0);
            assert_eq!(edit.range.end.line, 1);
            // Should cover the entire `/** */` (6 characters), not just `/**`.
            assert_eq!(
                edit.range.end.character, 6,
                "Range should cover the auto-closed */ too"
            );
        }
        other => panic!("Expected a TextEdit, got: {:?}", other),
    }
}

/// Indented `/** */` auto-close inside a class should work and cover
/// the full `/** */` range.
#[tokio::test]
async fn generates_docblock_with_indented_autoclosed_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_vscode_indent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /** */\n",
        "    public function bar($x): void {}\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock for indented auto-closed block"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "Should have @param for untyped $x, got:\n{}",
        snippet
    );

    match &item.unwrap().text_edit {
        Some(CompletionTextEdit::Edit(edit)) => {
            assert_eq!(edit.range.start.line, 2);
            assert_eq!(edit.range.start.character, 4);
            assert_eq!(edit.range.end.line, 2);
            assert_eq!(
                edit.range.end.character, 10,
                "Range should cover `    /** */` from the `/**` start to end of line"
            );
        }
        other => panic!("Expected a TextEdit, got: {:?}", other),
    }
}

/// Existing single-line docblock like `/** @var int */` should NOT trigger.
#[tokio::test]
async fn no_generation_for_existing_single_line_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_existing_sl.php").unwrap();
    let text = concat!("<?php\n", "/** @var int */\n", "public $count = 0;\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_none(),
        "Should NOT generate for `/** @var int */` (has content between /** and */)"
    );
}

// ─── Indentation ────────────────────────────────────────────────────────────

/// Indented `/**` inside a class should produce correctly indented lines.
#[tokio::test]
async fn preserves_indentation_inside_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_indent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "    public function bar(string $x): int {}\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items).unwrap();

    let snippet = snippet_text(item);
    // Snippet should have at least 3 lines: /**, summary, */
    let lines: Vec<&str> = snippet.lines().collect();
    assert!(
        lines.len() >= 3,
        "Snippet should have at least 3 lines, got {} lines:\n{}",
        lines.len(),
        snippet
    );

    // The snippet itself does NOT include the base indent — the editor
    // auto-indents continuation lines to match the text-edit range.
    // The closing line should be ` */` (single space prefix).
    let last = lines.last().unwrap();
    assert_eq!(
        *last, " */",
        "Last line should be ' */' (no base indent), got: {:?}",
        last,
    );
}

// ─── Suppression ────────────────────────────────────────────────────────────

/// `/**` followed by `*/` on the same line should NOT trigger generation
/// (it's a single-line doc comment like `/** @var int */`).
#[tokio::test]
async fn no_generation_for_single_line_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_single_line.php").unwrap();
    let text = concat!("<?php\n", "/** @var int */\n", "public $count = 0;\n",);
    // Cursor right after /** on line 1.
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_none(),
        "Should not generate a docblock when */ is on the same line"
    );
}

/// `/**` with existing docblock continuation lines should NOT trigger.
#[tokio::test]
async fn no_generation_inside_existing_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_existing.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * Existing documentation.\n",
        " */\n",
        "function test(): void {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_none(),
        "Should not generate a docblock inside an existing one"
    );
}

/// `/**` with code before it on the same line should NOT trigger.
#[tokio::test]
async fn no_generation_with_code_before() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_code_before.php").unwrap();
    let text = concat!("<?php\n", "$x = /**\n", "function test(): void {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 8).await;
    let item = find_docblock_item(&items);
    assert!(item.is_none(), "Should not generate when code precedes /**");
}

// ─── Override detection removed (no special treatment) ──────────────────────

/// Method overriding a parent now DOES get a docblock — no special
/// treatment for overrides.
#[tokio::test]
async fn generates_docblock_for_override_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Child extends Base {\n",
        "    /**\n",
        "    public function getName(): string { return 'child'; }\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 5, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Override method should now produce a docblock (no special treatment)"
    );
}

// ─── Throws ─────────────────────────────────────────────────────────────────

/// Function with throw statements should include @throws in the generated block.
#[tokio::test]
async fn generates_throws_tags() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_throws.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class NotFoundException extends \\Exception {}\n",
        "/**\n",
        "function find(int $id): string {\n",
        "    if ($id < 0) {\n",
        "        throw new NotFoundException('not found');\n",
        "    }\n",
        "    return 'found';\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock generation item");

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@throws NotFoundException"),
        "Should include @throws for uncaught exception, got:\n{}",
        snippet,
    );
}

// ─── Abstract methods ───────────────────────────────────────────────────────

/// Abstract method with scalar types should produce summary-only.
#[tokio::test]
async fn generates_docblock_for_abstract_method_summary_only() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_abstract.php").unwrap();
    let text = concat!(
        "<?php\n",
        "abstract class Shape {\n",
        "    /**\n",
        "    abstract public function area(float $radius): float;\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock for abstract method"
    );

    let snippet = snippet_text(item.unwrap());
    // float is scalar — no enrichment needed.
    assert!(
        !snippet.contains("@param"),
        "Scalar float param should NOT get @param, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@return"),
        "Scalar float return should NOT get @return, got:\n{}",
        snippet
    );
}

/// Abstract method with untyped params should produce @param.
#[tokio::test]
async fn generates_docblock_for_abstract_method_with_untyped_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_abstract2.php").unwrap();
    let text = concat!(
        "<?php\n",
        "abstract class Shape {\n",
        "    /**\n",
        "    abstract public function compute($radius): float;\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some());

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "Untyped param should get @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$radius"),
        "Should reference \\$radius (escaped), got:\n{}",
        snippet
    );
}

// ─── Static methods ─────────────────────────────────────────────────────────

/// Static method with scalar types should produce summary-only.
#[tokio::test]
async fn generates_docblock_for_static_method_summary_only() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    /**\n",
        "    public static function create(string $type): self {}\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock for static method"
    );

    let snippet = snippet_text(item.unwrap());
    // string is scalar, self is a built-in type — no enrichment needed.
    assert!(
        !snippet.contains("@param"),
        "Scalar string param should NOT get @param, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@return"),
        "self is a built-in type — should NOT get @return, got:\n{}",
        snippet
    );
}

// ─── Constructor promotion ──────────────────────────────────────────────────

/// Constructor with promoted scalar params should produce summary-only.
/// Constructors never get @return.
#[tokio::test]
async fn generates_docblock_for_constructor_with_promotion() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_ctor.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    /**\n",
        "    public function __construct(\n",
        "        private string $name,\n",
        "        private int $age,\n",
        "    ) {}\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock for constructor");

    let snippet = snippet_text(item.unwrap());
    // Scalar types — no @param needed. Constructors — no @return.
    assert!(
        !snippet.contains("@param"),
        "Scalar promoted params should NOT get @param, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@return"),
        "Constructor should never get @return, got:\n{}",
        snippet
    );
}

/// Constructor with untyped params should get @param but never @return.
/// Typing `/**` above a promoted property inside a constructor parameter
/// list should produce a single-line `/** @var Type */` at the correct
/// indentation (matching the `/**` trigger column, not the function).
#[tokio::test]
async fn generates_var_for_promoted_property_inside_constructor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_promoted_inline.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function __construct(\n",
        "        /**\n",
        "        public array $name,\n",
        "    ) {}\n",
        "}\n",
    );
    // Trigger is at line 3, col 11 (right after `/**`).
    let items = complete_at(&backend, &uri, text, 3, 11).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock for promoted property"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@var"),
        "Promoted property should have @var, got:\n{}",
        snippet
    );

    // Verify the text edit range starts at the `/**` column, not column 0.
    let edit = match item.unwrap().text_edit.as_ref().unwrap() {
        CompletionTextEdit::Edit(e) => e,
        _ => panic!("Expected a simple TextEdit"),
    };
    assert_eq!(
        edit.range.start.character, 8,
        "Edit should start at column 8 (the `/**` position), got: {}",
        edit.range.start.character
    );
}

#[tokio::test]
async fn constructor_gets_param_but_never_return() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_ctor2.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    /**\n",
        "    public function __construct($name) {}\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock for constructor");

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "Untyped param should get @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$name"),
        "Should reference \\$name (escaped), got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@return"),
        "Constructor should NEVER get @return, got:\n{}",
        snippet
    );
}

// ─── Readonly property ──────────────────────────────────────────────────────

/// Readonly typed property should include @var with the native type.
#[tokio::test]
async fn generates_var_for_readonly_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_readonly.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "    public readonly string $name;\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock for readonly property"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@var string"),
        "Typed readonly property should have @var string, got:\n{}",
        snippet
    );
}

// ─── Standalone function at file level ──────────────────────────────────────

/// A standalone function with array param gets @param enrichment,
/// but scalar params and return are skipped.
#[tokio::test]
async fn generates_docblock_for_standalone_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_standalone.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function calculateTotal(array $items, float $taxRate = 0.0): float {\n",
        "    return 0.0;\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock for standalone function"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "array param should get @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$items"),
        "Should reference \\$items (escaped), got:\n{}",
        snippet
    );
    // float $taxRate is scalar — no @param.
    assert!(
        !snippet.contains("\\$taxRate"),
        "Scalar float param should NOT appear in @param, got:\n{}",
        snippet
    );
    // float return is scalar — no @return.
    assert!(
        !snippet.contains("@return"),
        "Scalar float return should NOT get @return, got:\n{}",
        snippet
    );
}

// ─── Pre-select ─────────────────────────────────────────────────────────────

/// The docblock item should be pre-selected.
#[tokio::test]
async fn docblock_item_is_preselected() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_preselect.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "function test(): void {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items).unwrap();
    assert_eq!(
        item.preselect,
        Some(true),
        "Docblock item should be pre-selected"
    );
}

// ─── Abstract class ─────────────────────────────────────────────────────────

/// `abstract class` should be classified as ClassLike.
#[tokio::test]
async fn generates_docblock_for_abstract_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_abstract_class.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "abstract class Shape {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock for abstract class"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        !snippet.contains("@param"),
        "Class docblock should not have @param"
    );
    assert!(
        !snippet.contains("@return"),
        "Class docblock should not have @return"
    );
}

// ─── Final class ────────────────────────────────────────────────────────────

/// `final class` should be classified as ClassLike.
#[tokio::test]
async fn generates_docblock_for_final_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_final_class.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "final class Singleton {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock for final class");
}

// ─── Reference params ───────────────────────────────────────────────────────

/// By-reference params with scalar types should not get @param.
#[tokio::test]
async fn skips_reference_params_with_scalar_types() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_ref.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        "function swap(int &$a, int &$b): void {}\n",
    );
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some());

    let snippet = snippet_text(item.unwrap());
    assert!(
        !snippet.contains("@param"),
        "Scalar by-reference params should NOT get @param, got:\n{}",
        snippet
    );
}

/// By-reference params without types should get @param.
#[tokio::test]
async fn generates_param_for_untyped_reference_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_ref2.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "function swap(&$a, &$b): void {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some());

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "Untyped reference params should get @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$a"),
        "Should reference \\$a (escaped), got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$b"),
        "Should reference \\$b (escaped), got:\n{}",
        snippet
    );
}

// ─── Paramless void produces summary skeleton ───────────────────────────────

/// A paramless void function produces a summary-only skeleton.
#[tokio::test]
async fn paramless_void_produces_summary_skeleton() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_pv.php").unwrap();
    let text = concat!("<?php\n", "/**\n", "function noop(): void {}\n",);
    let items = complete_at(&backend, &uri, text, 1, 3).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Paramless void function should still produce a docblock"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(snippet.starts_with("/**"));
    assert!(snippet.contains("${1}"), "Should have summary tab stop");
    assert!(!snippet.contains("@param"));
    assert!(!snippet.contains("@return"));
}

// ═════════════════════════════════════════════════════════════════════════════
//  onTypeFormatting tests
// ═════════════════════════════════════════════════════════════════════════════
//
// These test the `textDocument/onTypeFormatting` path that fires when the
// editor auto-closes `/**` into `/** */` or a multi-line block and the
// user presses Enter.

/// Helper: open a file and request on-type formatting at the given position.
async fn format_on_enter(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<TextEdit> {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = DocumentOnTypeFormattingParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        ch: "\n".to_string(),
        options: FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            ..FormattingOptions::default()
        },
    };

    backend
        .on_type_formatting(params)
        .await
        .unwrap()
        .unwrap_or_default()
}

// ─── onTypeFormatting: function with untyped params ─────────────────────────

/// Function with untyped params gets @param enrichment via onTypeFormatting.
#[tokio::test]
async fn on_enter_generates_param_for_untyped() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_func.php").unwrap();
    let text = "<?php\n/** */\nfunction greet($name, $age): string {}\n";
    let edits = format_on_enter(&backend, &uri, text, 1, 4).await;
    assert!(
        !edits.is_empty(),
        "Should produce edits for empty docblock above function"
    );

    let new_text = &edits[0].new_text;
    assert!(
        new_text.contains("@param"),
        "Untyped params should get @param, got:\n{}",
        new_text
    );
    assert!(
        new_text.contains("$name"),
        "Should contain $name, got:\n{}",
        new_text
    );
    assert!(
        new_text.contains("$age"),
        "Should contain $age, got:\n{}",
        new_text
    );
}

/// Function with fully-typed scalar params produces summary-only via onTypeFormatting.
#[tokio::test]
async fn on_enter_generates_summary_for_fully_typed() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_func_typed.php").unwrap();
    let text = "<?php\n/** */\nfunction greet(string $name, int $age): string {}\n";
    let edits = format_on_enter(&backend, &uri, text, 1, 4).await;
    assert!(
        !edits.is_empty(),
        "Should produce edits for empty docblock above function"
    );

    let new_text = &edits[0].new_text;
    assert!(
        !new_text.contains("@param"),
        "Scalar params should NOT get @param, got:\n{}",
        new_text
    );
    assert!(
        !new_text.contains("@return"),
        "Scalar return should NOT get @return, got:\n{}",
        new_text
    );
}

/// Multi-line auto-generated block with untyped param.
#[tokio::test]
async fn on_enter_generates_docblock_for_multiline_empty() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_multi.php").unwrap();
    let text = "<?php\n/**\n * \n */\nfunction test($x): bool {}\n";
    // Cursor on line 2 (the ` * ` line).
    let edits = format_on_enter(&backend, &uri, text, 2, 3).await;
    assert!(
        !edits.is_empty(),
        "Should produce edits for multi-line empty docblock"
    );

    let new_text = &edits[0].new_text;
    assert!(
        new_text.contains("@param"),
        "Untyped param should get @param, got:\n{}",
        new_text
    );
    assert!(
        new_text.contains("$x"),
        "Should reference $x, got:\n{}",
        new_text
    );
}

// ─── onTypeFormatting: void function ────────────────────────────────────────

/// Void function with scalar params produces summary-only.
#[tokio::test]
async fn on_enter_summary_for_void_with_scalar_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_void.php").unwrap();
    let text = "<?php\n/** */\nfunction doStuff(int $n): void {}\n";
    let edits = format_on_enter(&backend, &uri, text, 1, 4).await;
    assert!(!edits.is_empty());

    let new_text = &edits[0].new_text;
    assert!(
        !new_text.contains("@param"),
        "Scalar int param should NOT get @param, got:\n{}",
        new_text
    );
    assert!(
        !new_text.contains("@return"),
        "Void function should not have @return, got:\n{}",
        new_text
    );
}

// ─── onTypeFormatting: class ────────────────────────────────────────────────

/// Empty docblock above a class.
#[tokio::test]
async fn on_enter_generates_docblock_for_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_class.php").unwrap();
    let text = "<?php\n/** */\nclass MyService {}\n";
    let edits = format_on_enter(&backend, &uri, text, 1, 4).await;
    assert!(!edits.is_empty(), "Should produce edits for class docblock");

    let new_text = &edits[0].new_text;
    assert!(!new_text.contains("@param"), "Class should not have @param");
    assert!(
        !new_text.contains("@return"),
        "Class should not have @return"
    );
    assert!(new_text.contains("/**"), "Should contain opening /**");
    assert!(new_text.contains("*/"), "Should contain closing */");
}

// ─── onTypeFormatting: property ─────────────────────────────────────────────

/// Untyped property should get @var with mixed.
#[tokio::test]
async fn on_enter_generates_var_for_untyped_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_prop.php").unwrap();
    let text = "<?php\nclass Foo {\n    /** */\n    public $name;\n}\n";
    let edits = format_on_enter(&backend, &uri, text, 2, 7).await;
    assert!(!edits.is_empty());

    let new_text = &edits[0].new_text;
    assert!(
        new_text.contains("@var"),
        "Untyped property should have @var, got:\n{}",
        new_text
    );
    assert!(
        new_text.contains("mixed"),
        "Untyped property should have mixed, got:\n{}",
        new_text
    );
}

/// Typed property should get @var with the native type.
#[tokio::test]
async fn on_enter_generates_var_for_typed_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_prop_typed.php").unwrap();
    let text = "<?php\nclass Foo {\n    /** */\n    public string $name;\n}\n";
    let edits = format_on_enter(&backend, &uri, text, 2, 7).await;
    assert!(!edits.is_empty());

    let new_text = &edits[0].new_text;
    assert!(
        new_text.contains("@var string"),
        "Typed property should have @var string, got:\n{}",
        new_text
    );
}

/// On-enter above a promoted property inside a constructor should produce
/// a single-line `/** @var Type */` at the correct indentation.
#[tokio::test]
async fn on_enter_generates_var_for_promoted_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_promoted.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function __construct(\n",
        "        /** */\n",
        "        public array $name,\n",
        "    ) {}\n",
        "}\n",
    );
    let edits = format_on_enter(&backend, &uri, text, 3, 11).await;
    assert!(
        !edits.is_empty(),
        "Should produce edits for promoted property"
    );

    let new_text = &edits[0].new_text;
    assert!(
        new_text.contains("@var"),
        "Promoted property should have @var, got:\n{}",
        new_text
    );
    // Must preserve the 8-space indentation of the parameter list.
    assert!(
        new_text.starts_with("        /** @var"),
        "Should have 8-space indent, got:\n{:?}",
        new_text
    );
}

/// Zed places `/** */` at the function-level indent (4 spaces) even when
/// the declaration below is at parameter-level indent (8 spaces).  The
/// generated docblock must use the declaration's indentation.
#[tokio::test]
async fn on_enter_uses_declaration_indent_not_block_indent() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_promoted_misindent.php").unwrap();
    // Simulate Zed: `/** */` at 4-space indent, declaration at 8-space.
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function __construct(\n",
        "    /** */\n",
        "        public array $name,\n",
        "    ) {}\n",
        "}\n",
    );
    let edits = format_on_enter(&backend, &uri, text, 3, 7).await;
    assert!(!edits.is_empty(), "Should produce edits");

    let new_text = &edits[0].new_text;
    assert!(
        new_text.contains("@var"),
        "Should have @var, got:\n{}",
        new_text
    );
    // The generated line must match the declaration's 8-space indent,
    // NOT the 4-space indent of the misplaced `/** */`.
    assert!(
        new_text.starts_with("        /** @var"),
        "Should use declaration indent (8 spaces), got:\n{:?}",
        new_text
    );
}

// ─── onTypeFormatting: indentation ──────────────────────────────────────────

/// Generated block inside a class should be correctly indented.
#[tokio::test]
async fn on_enter_preserves_indentation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_indent.php").unwrap();
    let text = "<?php\nclass Foo {\n    /** */\n    public function bar($x): int {}\n}\n";
    let edits = format_on_enter(&backend, &uri, text, 2, 7).await;
    assert!(!edits.is_empty());

    let new_text = &edits[0].new_text;
    // The generated block should use `    ` (4-space) indentation.
    assert!(
        new_text.contains("    /**"),
        "Opening should be indented, got:\n{}",
        new_text
    );
    assert!(
        new_text.contains("     * @param") || new_text.contains("    * @param"),
        "Tags should be indented, got:\n{}",
        new_text
    );
    assert!(
        new_text.contains("    */"),
        "Closing should be indented, got:\n{}",
        new_text
    );
}

// ─── onTypeFormatting: suppression ──────────────────────────────────────────

/// A docblock that already has content should NOT be replaced.
#[tokio::test]
async fn on_enter_does_not_replace_existing_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_existing.php").unwrap();
    let text = "<?php\n/**\n * This function does stuff.\n */\nfunction test(): void {}\n";
    // Cursor on the comment-content line.
    let edits = format_on_enter(&backend, &uri, text, 2, 5).await;
    assert!(
        edits.is_empty(),
        "Should not replace an existing docblock with content"
    );
}

// ─── onTypeFormatting: override now generates ───────────────────────────────

/// Override method now gets a docblock via onTypeFormatting (no special treatment).
#[tokio::test]
async fn on_enter_generates_for_override() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Child extends Base {\n",
        "    /** */\n",
        "    public function getName(): string { return 'child'; }\n",
        "}\n",
    );
    // Cursor on line 5 (the `/** */` line inside Child).
    let edits = format_on_enter(&backend, &uri, text, 5, 7).await;
    assert!(
        !edits.is_empty(),
        "Override method should now produce edits (no special treatment)"
    );
}

// ─── onTypeFormatting: constant ─────────────────────────────────────────────

/// Constant with type should include @var.
#[tokio::test]
async fn on_enter_generates_var_for_typed_constant() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_const.php").unwrap();
    let text = "<?php\nclass Foo {\n    /** */\n    public const int MAX = 100;\n}\n";
    let edits = format_on_enter(&backend, &uri, text, 2, 7).await;
    assert!(!edits.is_empty());

    let new_text = &edits[0].new_text;
    assert!(
        new_text.contains("@var int"),
        "Typed constant should have @var int, got:\n{}",
        new_text
    );
}

// ─── onTypeFormatting: edit range ───────────────────────────────────────────

/// The edit range should cover the entire original docblock.
#[tokio::test]
async fn on_enter_edit_covers_original_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_range.php").unwrap();
    let text = "<?php\n/** */\nfunction test(): void {}\n";
    let edits = format_on_enter(&backend, &uri, text, 1, 4).await;
    assert!(!edits.is_empty());

    let range = &edits[0].range;
    // Should start at line 1 (the `/** */` line).
    assert_eq!(range.start.line, 1, "Edit should start at the /** line");
    // Should end at line 2 (the line after `/** */`).
    assert_eq!(
        range.end.line, 2,
        "Edit should end after the closing */ line"
    );
}

// ─── Attribute skipping ─────────────────────────────────────────────────────

#[tokio::test]
async fn generates_return_when_attribute_before_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_attr_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "    #[Override]\n",
        "    public function toArray(): array {}\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock generation item");

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@return"),
        "array return type should get @return, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@param"),
        "No params means no @param tags, got:\n{}",
        snippet
    );
}

#[tokio::test]
async fn generates_param_when_multiline_attribute_before_declaration() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_attr_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "    #[Route(\n",
        "        path: '/api/users',\n",
        "        methods: ['GET']\n",
        "    )]\n",
        "    public function getUsers(array $filters): array {}\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock generation item");

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@param"),
        "array param should get @param, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("\\$filters"),
        "Should reference \\$filters, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("@return"),
        "array return type should get @return, got:\n{}",
        snippet
    );
}

#[tokio::test]
async fn generates_docblock_when_multiple_attributes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_attr_multiple.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "    #[Override]\n",
        "    #[Deprecated]\n",
        "    public function process(): void {}\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock generation item");

    let snippet = snippet_text(item.unwrap());
    // void return + no untyped params → summary-only skeleton.
    assert!(
        snippet.contains("${1}"),
        "Should have summary tab stop, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@param"),
        "No params means no @param, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("@return"),
        "void should not get @return, got:\n{}",
        snippet
    );
}

// ─── Variable assignments ───────────────────────────────────────────────────

/// Typing `/**` above a variable assignment should produce a single-line
/// `/** @var Type */` with a placeholder for the type.
/// Inside a function body, `/**` should NOT generate a full docblock.
/// The `@` tag completion is more appropriate there because the user
/// might want `@var`, `@throws`, `@todo`, etc.
#[tokio::test]
async fn no_generation_inside_function_body_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_var_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    /**\n",
        "    $items = [''];\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_none(),
        "Should NOT produce a docblock for variable assignment inside function body"
    );
}

/// On-enter inside a function body should NOT generate a docblock either.
#[tokio::test]
async fn on_enter_no_generation_inside_function_body_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_var_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    /** */\n",
        "    $items = [''];\n",
        "}\n",
    );
    let edits = format_on_enter(&backend, &uri, text, 2, 7).await;
    assert!(
        edits.is_empty(),
        "Should NOT produce edits for variable assignment inside function body"
    );
}

/// Inside a nested scope, `/**` above a variable should NOT generate.
#[tokio::test]
async fn no_generation_inside_nested_scope_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_var_nested.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function demo() {\n",
        "    if (true) {\n",
        "        /**\n",
        "        $data = [];\n",
        "    }\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 3, 11).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_none(),
        "Should NOT produce a docblock for nested variable inside function body"
    );
}

// ─── Constructor-inferred type in @var ──────────────────────────────────────

/// Typing `/**` above an untyped property whose type is inferred from a
/// constructor assignment (`$this->repo = new Repository()`) should use
/// the inferred class name instead of `mixed`.
#[tokio::test]
async fn generates_var_with_constructor_inferred_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_prop_inferred.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Repository {\n",                             // 1
        "    public function findById(int $id): void {}\n", // 2
        "}\n",                                              // 3
        "\n",                                               // 4
        "class Service {\n",                                // 5
        "    /** */\n",                                     // 6
        "    private $repository;\n",                       // 7
        "\n",                                               // 8
        "    public function __construct() {\n",            // 9
        "        $this->repository = new Repository();\n",  // 10
        "    }\n",                                          // 11
        "}\n",                                              // 12
    );
    let items = complete_at(&backend, &uri, text, 6, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for constructor-inferred property"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@var"),
        "Should include @var tag, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("Repository"),
        "Should use inferred type Repository instead of mixed, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("mixed"),
        "Should NOT fall back to mixed when constructor-inferred type is available, got:\n{}",
        snippet
    );
}

/// Same as above but for a promoted constructor parameter with a
/// `new ClassName()` default value.
#[tokio::test]
async fn generates_var_with_promoted_param_new_default_inferred_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_prop_promoted_inferred.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class Repository {\n",                              // 1
        "    public function findById(int $id): void {}\n",  // 2
        "}\n",                                               // 3
        "\n",                                                // 4
        "class Service {\n",                                 // 5
        "    public function __construct(\n",                // 6
        "        /** */\n",                                  // 7
        "        private $repository = new Repository(),\n", // 8
        "    ) {}\n",                                        // 9
        "}\n",                                               // 10
    );
    let items = complete_at(&backend, &uri, text, 7, 11).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for promoted param with new default"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("@var"),
        "Should include @var tag, got:\n{}",
        snippet
    );
    assert!(
        snippet.contains("Repository"),
        "Should use inferred type Repository instead of mixed, got:\n{}",
        snippet
    );
}

/// When a property has an explicit native type, the generated @var should
/// use that type even if the constructor assigns a different class.
#[tokio::test]
async fn generates_var_with_native_type_ignores_constructor_assignment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_prop_native_wins.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // 0
        "class Dog { public function bark(): void {} }\n", // 1
        "class Cat { public function meow(): void {} }\n", // 2
        "\n",                                              // 3
        "class Shelter {\n",                               // 4
        "    /** */\n",                                    // 5
        "    private Dog $pet;\n",                         // 6
        "\n",                                              // 7
        "    public function __construct() {\n",           // 8
        "        $this->pet = new Cat();\n",               // 9
        "    }\n",                                         // 10
        "}\n",                                             // 11
    );
    let items = complete_at(&backend, &uri, text, 5, 7).await;
    let item = find_docblock_item(&items);
    assert!(
        item.is_some(),
        "Should produce a docblock generation item for typed property"
    );

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("Dog"),
        "Should use native type Dog, got:\n{}",
        snippet
    );
    assert!(
        !snippet.contains("Cat"),
        "Should NOT use constructor-assigned Cat when native type exists, got:\n{}",
        snippet
    );
}

/// On-enter formatting for an untyped property with constructor-inferred type.
#[tokio::test]
async fn on_enter_generates_var_with_constructor_inferred_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///ot_prop_inferred.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Repository {\n",                             // 1
        "    public function findById(int $id): void {}\n", // 2
        "}\n",                                              // 3
        "\n",                                               // 4
        "class Service {\n",                                // 5
        "    /** */\n",                                     // 6
        "    private $repository;\n",                       // 7
        "\n",                                               // 8
        "    public function __construct() {\n",            // 9
        "        $this->repository = new Repository();\n",  // 10
        "    }\n",                                          // 11
        "}\n",                                              // 12
    );
    let edits = format_on_enter(&backend, &uri, text, 6, 9).await;
    assert!(!edits.is_empty(), "Should produce edits");

    let new_text = &edits[0].new_text;
    assert!(
        new_text.contains("@var"),
        "Should have @var tag, got:\n{}",
        new_text
    );
    assert!(
        new_text.contains("Repository"),
        "Should use inferred type Repository instead of mixed, got:\n{}",
        new_text
    );
    assert!(
        !new_text.contains("mixed"),
        "Should NOT fall back to mixed, got:\n{}",
        new_text
    );
}

/// Untyped property with NO constructor assignment should still get `mixed`.
#[tokio::test]
async fn generates_var_mixed_when_no_constructor_inference() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_prop_no_infer.php").unwrap();
    let text = concat!(
        "<?php\n",              // 0
        "class Service {\n",    // 1
        "    /** */\n",         // 2
        "    private $data;\n", // 3
        "}\n",                  // 4
    );
    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_docblock_item(&items);
    assert!(item.is_some(), "Should produce a docblock generation item");

    let snippet = snippet_text(item.unwrap());
    assert!(
        snippet.contains("mixed"),
        "Untyped property without inference should fall back to mixed, got:\n{}",
        snippet
    );
}
