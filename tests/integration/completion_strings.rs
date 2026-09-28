//! Integration tests: completion suppression inside string literals.
//!
//! Verifies that the LSP returns **no** completions when the cursor is
//! inside a plain string literal (single-quoted, nowdoc, or non-interpolation
//! positions in double-quoted strings), while still returning completions
//! inside PHP interpolation contexts (`{$expr->}`, `"$var->"`, heredocs
//! with interpolation).

use crate::common::{complete_at_raw, create_test_backend};
use tower_lsp::lsp_types::*;

// ═══════════════════════════════════════════════════════════════════════════
//  Single-quoted strings — always suppress
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn no_completion_inside_single_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = 'hello ';\n",
    );

    // Cursor inside the single-quoted string (line 5, col 10: between 'hello' and closing ')
    let result = complete_at_raw(&backend, &uri, text, 5, 10).await;
    assert!(
        result.is_none(),
        "Should suppress completion inside single-quoted string, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_completion_for_class_name_inside_single_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_single_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$s = 'Foo';\n",
    );

    // Cursor at 'Foo' inside the string — should NOT offer class completions
    let result = complete_at_raw(&backend, &uri, text, 4, 8).await;
    assert!(
        result.is_none(),
        "Should suppress class name completion inside single-quoted string, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_completion_for_dollar_inside_single_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_single_dollar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = '$f->';\n",
    );

    // Cursor right after `->` inside the single-quoted string (no interpolation)
    let result = complete_at_raw(&backend, &uri, text, 5, 10).await;
    assert!(
        result.is_none(),
        "Should suppress completion inside single-quoted string even with $f->, got: {:?}",
        result
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Double-quoted strings — suppress plain text, allow interpolation
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn no_completion_for_plain_text_in_double_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_double_plain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = \"hello world\";\n",
    );

    // Cursor inside "hello world" at a plain text position
    let result = complete_at_raw(&backend, &uri, text, 5, 10).await;
    assert!(
        result.is_none(),
        "Should suppress completion inside plain text in double-quoted string, got: {:?}",
        result
    );
}

#[tokio::test]
async fn completion_works_with_braces_in_double_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_double_brace.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public string $name = '';\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = \"hello {$f->}\";\n",
    );

    // Cursor after `$f->` inside `{$f->}` — line 6, col 18
    // "hello {$f->}" — positions: $ at 13, f at 14, - at 15, > at 16, } at 17, " at 18
    // We want cursor right after `->` which is col 17 (before })
    let result = complete_at_raw(&backend, &uri, text, 6, 17).await;
    assert!(
        result.is_some(),
        "Should allow completion inside {{$f->}} interpolation in double-quoted string"
    );
    let items = result.unwrap();
    let has_name = items.iter().any(|i| i.label == "name");
    assert!(
        has_name,
        "Should suggest 'name' property. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    // Brace interpolation allows full expressions — methods should appear.
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Brace interpolation should include methods. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn completion_works_with_simple_interpolation_in_double_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_double_simple.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public string $name = '';\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = \"hello $f->\";\n",
    );

    // Cursor right after `$f->` (simple interpolation without braces)
    // Line 6: $s = "hello $f->"; — the -> ends at col 16
    let result = complete_at_raw(&backend, &uri, text, 6, 16).await;
    assert!(
        result.is_some(),
        "Should allow completion for simple interpolation $f-> in double-quoted string"
    );
    let items = result.unwrap();
    let has_name = items.iter().any(|i| i.label == "name");
    assert!(
        has_name,
        "Should suggest 'name' property. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    // Simple interpolation only supports property access in PHP —
    // methods should be filtered out.
    let has_method = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::METHOD));
    assert!(
        !has_method,
        "Simple interpolation should NOT include methods. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn simple_interpolation_filters_methods_but_brace_does_not() {
    // Same class, same variable — verify that `"$f->"` shows only
    // properties while `"{$f->}"` shows both properties and methods.
    let backend = create_test_backend();

    let uri_simple = Url::parse("file:///str_filter_simple.php").unwrap();
    let text_simple = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public string $title = '';\n",
        "    public int $count = 0;\n",
        "    public function render(): string { return ''; }\n",
        "    public function reset(): void {}\n",
        "}\n",
        "$w = new Widget();\n",
        "$s = \"val: $w->\";\n",
    );

    // Simple: $s = "val: $w->"
    // $=0 s=1 ' '=2 ==3 ' '=4 "=5 v=6 a=7 l=8 :=9 ' '=10 $=11 w=12 -=13 >=14 "=15
    let simple_items = complete_at_raw(&backend, &uri_simple, text_simple, 8, 15)
        .await
        .expect("Simple interpolation should return items");
    let simple_labels: Vec<&str> = simple_items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        simple_labels.contains(&"title"),
        "Simple: should include 'title' property. Got: {:?}",
        simple_labels
    );
    assert!(
        simple_labels.contains(&"count"),
        "Simple: should include 'count' property. Got: {:?}",
        simple_labels
    );
    assert!(
        !simple_items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::METHOD)),
        "Simple: should NOT include any methods. Got: {:?}",
        simple_labels
    );

    let uri_brace = Url::parse("file:///str_filter_brace.php").unwrap();
    let text_brace = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public string $title = '';\n",
        "    public int $count = 0;\n",
        "    public function render(): string { return ''; }\n",
        "    public function reset(): void {}\n",
        "}\n",
        "$w = new Widget();\n",
        "$s = \"val: {$w->}\";\n",
    );

    // Brace: $s = "val: {$w->}"
    // $=0 s=1 ' '=2 ==3 ' '=4 "=5 v=6 a=7 l=8 :=9 ' '=10 {=11 $=12 w=13 -=14 >=15 }=16 "=17
    let brace_items = complete_at_raw(&backend, &uri_brace, text_brace, 8, 16)
        .await
        .expect("Brace interpolation should return items");
    let brace_labels: Vec<&str> = brace_items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        brace_labels.contains(&"title"),
        "Brace: should include 'title' property. Got: {:?}",
        brace_labels
    );
    assert!(
        brace_items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::METHOD)),
        "Brace: should include methods. Got: {:?}",
        brace_labels
    );
    assert!(
        brace_labels.iter().any(|l| l.starts_with("render")),
        "Brace: should include 'render' method. Got: {:?}",
        brace_labels
    );
}

#[tokio::test]
async fn completion_works_with_nullsafe_simple_interpolation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_double_nullsafe.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public string $name = '';\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = \"hello $f?->\";\n",
    );

    // Cursor right after `$f?->` — line 6, col 17
    let result = complete_at_raw(&backend, &uri, text, 6, 17).await;
    assert!(
        result.is_some(),
        "Should allow completion for nullsafe interpolation $f?-> in double-quoted string"
    );
    let items = result.unwrap();
    let has_name = items.iter().any(|i| i.label == "name");
    assert!(
        has_name,
        "Should suggest 'name' property. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    // Nullsafe simple interpolation is still simple — methods filtered.
    let has_method = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::METHOD));
    assert!(
        !has_method,
        "Nullsafe simple interpolation should NOT include methods. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Heredoc — suppress plain text, allow interpolation
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn no_completion_for_plain_text_in_heredoc() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_heredoc_plain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = <<<EOT\n",
        "hello world\n",
        "EOT;\n",
    );

    // Cursor inside heredoc plain text (line 6, col 5)
    let result = complete_at_raw(&backend, &uri, text, 6, 5).await;
    assert!(
        result.is_none(),
        "Should suppress completion inside plain text in heredoc, got: {:?}",
        result
    );
}

#[tokio::test]
async fn completion_works_with_braces_in_heredoc() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_heredoc_brace.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public string $name = '';\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = <<<EOT\n",
        "hello {$f->}\n",
        "EOT;\n",
    );

    // Cursor after `$f->` inside `{$f->}` — line 7, col 11
    // "hello {$f->}" — { at 6, $ at 7, f at 8, - at 9, > at 10, } at 11
    let result = complete_at_raw(&backend, &uri, text, 7, 11).await;
    assert!(
        result.is_some(),
        "Should allow completion inside {{$f->}} interpolation in heredoc"
    );
    let items = result.unwrap();
    let has_name = items.iter().any(|i| i.label == "name");
    assert!(
        has_name,
        "Should suggest 'name' property inside heredoc interpolation. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    // Brace interpolation in heredoc should include methods too.
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Brace interpolation in heredoc should include methods. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn completion_works_with_simple_interpolation_in_heredoc() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_heredoc_simple.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public string $name = '';\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = <<<EOT\n",
        "hello $f->\n",
        "EOT;\n",
    );

    // Cursor right after `$f->` — line 7, col 10
    let result = complete_at_raw(&backend, &uri, text, 7, 10).await;
    assert!(
        result.is_some(),
        "Should allow completion for simple interpolation $f-> in heredoc"
    );
    let items = result.unwrap();
    let has_name = items.iter().any(|i| i.label == "name");
    assert!(
        has_name,
        "Should suggest 'name' property inside heredoc simple interpolation. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    // Simple interpolation in heredoc — methods filtered.
    let has_method = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::METHOD));
    assert!(
        !has_method,
        "Simple interpolation in heredoc should NOT include methods. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Nowdoc — always suppress (no interpolation)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn no_completion_inside_nowdoc() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_nowdoc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = <<<'EOT'\n",
        "hello world\n",
        "EOT;\n",
    );

    // Cursor inside nowdoc (line 6, col 5)
    let result = complete_at_raw(&backend, &uri, text, 6, 5).await;
    assert!(
        result.is_none(),
        "Should suppress completion inside nowdoc, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_completion_for_dollar_arrow_inside_nowdoc() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_nowdoc_arrow.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = <<<'EOT'\n",
        "$f->\n",
        "EOT;\n",
    );

    // Cursor after `$f->` inside nowdoc — no interpolation in nowdoc
    let result = complete_at_raw(&backend, &uri, text, 6, 4).await;
    assert!(
        result.is_none(),
        "Should suppress completion inside nowdoc even with $f->, got: {:?}",
        result
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Normal code — still works after strings
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn completion_works_after_single_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_after_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$s = 'hello';\n",
        "$f = new Foo();\n",
        "$f->\n",
    );

    // Cursor after `$f->` in normal code — line 6, col 4
    let result = complete_at_raw(&backend, &uri, text, 6, 4).await;
    assert!(
        result.is_some(),
        "Should still offer completions in normal code after a string"
    );
    let items = result.unwrap();
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Should suggest 'bar'. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn completion_works_after_double_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_after_double.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$s = \"hello\";\n",
        "$f = new Foo();\n",
        "$f->\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 6, 4).await;
    assert!(
        result.is_some(),
        "Should still offer completions in normal code after a double-quoted string"
    );
    let items = result.unwrap();
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Should suggest 'bar'. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn completion_works_after_heredoc() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_after_heredoc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$s = <<<EOT\n",
        "hello\n",
        "EOT;\n",
        "$f = new Foo();\n",
        "$f->\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 8, 4).await;
    assert!(
        result.is_some(),
        "Should still offer completions in normal code after a heredoc"
    );
    let items = result.unwrap();
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Should suggest 'bar'. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn completion_works_after_nowdoc() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_after_nowdoc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$s = <<<'EOT'\n",
        "hello\n",
        "EOT;\n",
        "$f = new Foo();\n",
        "$f->\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 8, 4).await;
    assert!(
        result.is_some(),
        "Should still offer completions in normal code after a nowdoc"
    );
    let items = result.unwrap();
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Should suggest 'bar'. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn variable_completion_inside_double_quoted_string_after_dollar_var() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_double_var_no_arrow.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = \"hello $f\";\n",
    );

    // Cursor right after `$f` — this is a valid interpolation site in a
    // double-quoted string (`"hello $f"` interpolates `$f`), so variable
    // completion should be allowed.
    // Line 5: $s = "hello $f";
    // $=0 s=1 ' '=2 ==3 ' '=4 "=5 h=6 e=7 l=8 l=9 o=10 ' '=11 $=12 f=13 "=14
    let result = complete_at_raw(&backend, &uri, text, 5, 14).await;
    assert!(
        result.is_some(),
        "Should allow completion for $var inside double-quoted string (interpolation site)"
    );
}

#[tokio::test]
async fn variable_completion_inside_double_quoted_string_bare_dollar() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_double_bare_dollar.php").unwrap();
    let text = concat!("<?php\n", "$foo = 42;\n", "$s = \"hello $\";\n",);

    // Cursor right after `$` — the user just typed a dollar sign to start
    // an interpolation. Variable name completion should fire.
    // Line 2: $s = "hello $";
    // $=0 s=1 ' '=2 ==3 ' '=4 "=5 h=6 e=7 l=8 l=9 o=10 ' '=11 $=12 "=13
    let result = complete_at_raw(&backend, &uri, text, 2, 13).await;
    assert!(
        result.is_some(),
        "Should allow variable completion for bare $ inside double-quoted string"
    );
    let items = result.unwrap();
    let has_foo = items.iter().any(|i| i.label == "$foo");
    assert!(
        has_foo,
        "Should suggest '$foo'. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn variable_completion_inside_heredoc_bare_dollar() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_heredoc_bare_dollar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$foo = 42;\n",
        "$s = <<<EOT\n",
        "hello $\n",
        "EOT;\n",
    );

    // Cursor right after `$` inside heredoc — line 3, col 7
    let result = complete_at_raw(&backend, &uri, text, 3, 7).await;
    assert!(
        result.is_some(),
        "Should allow variable completion for bare $ inside heredoc"
    );
    let items = result.unwrap();
    let has_foo = items.iter().any(|i| i.label == "$foo");
    assert!(
        has_foo,
        "Should suggest '$foo' inside heredoc. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn no_variable_completion_for_dollar_inside_single_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_single_bare_dollar.php").unwrap();
    let text = concat!("<?php\n", "$foo = 42;\n", "$s = 'hello $';\n",);

    // Cursor right after `$` inside a single-quoted string — no
    // interpolation in single-quoted strings.
    let result = complete_at_raw(&backend, &uri, text, 2, 13).await;
    assert!(
        result.is_none(),
        "Should suppress variable completion for $ inside single-quoted string, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_variable_completion_for_dollar_inside_nowdoc() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_nowdoc_bare_dollar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$foo = 42;\n",
        "$s = <<<'EOT'\n",
        "hello $\n",
        "EOT;\n",
    );

    // Cursor right after `$` inside nowdoc — no interpolation in nowdoc.
    let result = complete_at_raw(&backend, &uri, text, 3, 7).await;
    assert!(
        result.is_none(),
        "Should suppress variable completion for $ inside nowdoc, got: {:?}",
        result
    );
}

#[tokio::test]
async fn escaped_quote_does_not_end_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_escaped_quote.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = \"she said \\\"hello\\\" ok\";\n",
    );

    // Cursor after the escaped quotes, still inside the string (col 22 is between "ok" and closing ")
    let result = complete_at_raw(&backend, &uri, text, 5, 22).await;
    assert!(
        result.is_none(),
        "Escaped quotes should not end the string early, got: {:?}",
        result
    );
}

#[tokio::test]
async fn escaped_single_quote_does_not_end_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_escaped_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = 'she said \\'hello\\' ok';\n",
    );

    // Cursor inside the string after escaped quotes
    let result = complete_at_raw(&backend, &uri, text, 5, 22).await;
    assert!(
        result.is_none(),
        "Escaped single quotes should not end the string early, got: {:?}",
        result
    );
}

#[tokio::test]
async fn comment_syntax_inside_string_still_suppressed() {
    // Existing behavior: comment-like text inside strings should not
    // confuse the scanner. This test ensures the string check and the
    // comment check work together.
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_comment_inside.php").unwrap();
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

#[tokio::test]
async fn no_class_name_completion_inside_brace_interpolation() {
    // Even inside `{$...}` brace interpolation, class name / function
    // completions should not leak through — only member access and
    // variable name completions are useful there.
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_brace_no_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$s = \"hello {Foo}\";\n",
    );

    // Cursor at `Foo` inside braces but without `$` — not actually
    // an interpolation expression, so the brace is literal text.
    // This should be suppressed (it's string literal, not interpolation).
    let result = complete_at_raw(&backend, &uri, text, 5, 12).await;
    assert!(
        result.is_none(),
        "Should suppress class name completion inside non-interpolation braces, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_completion_for_non_interpolation_braces_in_double_quoted_string() {
    // `"{User::ADMIN_TYPE}"` is NOT interpolation because `{` is not
    // followed by `$`. PHP outputs the braces and text literally.
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_double_brace_no_dollar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    const BAR = 1;\n",
        "}\n",
        "$s = \"{Foo::}\";\n",
    );

    // Cursor after `::` inside `{Foo::}` — col 11
    // $=0 s=1 ' '=2 ==3 ' '=4 "=5 {=6 F=7 o=8 o=9 :=10 :=11 }=12 "=13
    let result = complete_at_raw(&backend, &uri, text, 4, 12).await;
    assert!(
        result.is_none(),
        "Should suppress completion inside {{ClassName::}} without $ — not interpolation, got: {:?}",
        result
    );
}

#[tokio::test]
async fn no_class_name_completion_inside_double_quoted_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_double_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$s = \"Foo\";\n",
    );

    // Cursor at 'Foo' inside the double-quoted string
    let result = complete_at_raw(&backend, &uri, text, 4, 8).await;
    assert!(
        result.is_none(),
        "Should suppress class name completion inside double-quoted string, got: {:?}",
        result
    );
}

#[tokio::test]
async fn concatenation_after_string_works() {
    // `$f = 'x' . new Foo(); $f->` — make sure the scanner correctly
    // exits the string at the closing quote.
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_concat.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$s = 'hello';\n",
        "$f = new Foo();\n",
        "$f->\n",
    );

    let result = complete_at_raw(&backend, &uri, text, 6, 4).await;
    assert!(
        result.is_some(),
        "Code after a closed string should complete normally"
    );
}

#[test]
fn unit_classify_nested_braces_in_interpolation() {
    // Verify that array access brackets inside `{$arr['key']->}` do not
    // confuse the brace-depth tracker — the cursor after `->` should
    // still be classified as BraceInterpolation.
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    let content = "<?php\n$s = \"{$arr['key']->}\";\n";
    // Line 1: $=0 s=1 ' '=2 ==3 ' '=4 "=5 {=6 $=7 a=8 r=9 r=10
    //         [=11 '=12 k=13 e=14 y=15 '=16 ]=17 -=18 >=19 }=20 "=21
    // Cursor at col 20 (right after `>`, right before `}`)
    let pos = Position {
        line: 1,
        character: 20,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::BraceInterpolation
    );
}

#[tokio::test]
async fn multiple_strings_on_same_line() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///str_multi_same_line.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$a = 'x'; $b = 'y'; $f->\n",
    );

    // Cursor after `$f->` which is after two closed strings on the same line
    let result = complete_at_raw(&backend, &uri, text, 5, 25).await;
    assert!(
        result.is_some(),
        "Should complete after multiple closed strings on the same line"
    );
    let items = result.unwrap();
    let has_bar = items.iter().any(|i| i.label.starts_with("bar"));
    assert!(
        has_bar,
        "Should suggest 'bar'. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Unit tests for classify_string_context
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unit_classify_not_in_string() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    let content = "<?php\n$x = 1;\n";
    let pos = Position {
        line: 1,
        character: 5,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::NotInString
    );
}

#[test]
fn unit_classify_single_quoted() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    let content = "<?php\n$x = 'hello';\n";
    let pos = Position {
        line: 1,
        character: 8,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::InStringLiteral
    );
}

#[test]
fn unit_classify_double_quoted_plain() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    let content = "<?php\n$x = \"hello\";\n";
    let pos = Position {
        line: 1,
        character: 8,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::InStringLiteral
    );
}

#[test]
fn unit_classify_double_quoted_brace_interpolation() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    // Cursor inside {$f->} — after the ->
    let content = "<?php\n$x = \"{$f->}\";\n";
    // {=6 $=7 f=8 -=9 >=10 }=11 "=12
    let pos = Position {
        line: 1,
        character: 11,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::BraceInterpolation
    );
}

#[test]
fn unit_classify_double_quoted_simple_interpolation() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    // Cursor right after `$f->` (no braces)
    let content = "<?php\n$x = \"hi $f->\";\n";
    // h=6 i=7 ' '=8 $=9 f=10 -=11 >=12 "=13
    let pos = Position {
        line: 1,
        character: 13,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::SimpleInterpolation
    );
}

#[test]
fn unit_classify_double_quoted_bare_dollar() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    // Cursor right after bare `$` in a double-quoted string
    let content = "<?php\n$x = \"hi $\";\n";
    // h=6 i=7 ' '=8 $=9 "=10
    let pos = Position {
        line: 1,
        character: 10,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::SimpleInterpolation
    );
}

#[test]
fn unit_classify_double_quoted_partial_variable() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    // Cursor right after `$fo` (partially typed variable name)
    let content = "<?php\n$x = \"hi $fo\";\n";
    // h=6 i=7 ' '=8 $=9 f=10 o=11 "=12
    let pos = Position {
        line: 1,
        character: 12,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::SimpleInterpolation
    );
}

#[test]
fn unit_classify_nowdoc() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    let content = "<?php\n$x = <<<'EOT'\nhello\nEOT;\n";
    let pos = Position {
        line: 2,
        character: 3,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::InStringLiteral
    );
}

#[test]
fn unit_classify_heredoc_plain() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    let content = "<?php\n$x = <<<EOT\nhello\nEOT;\n";
    let pos = Position {
        line: 2,
        character: 3,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::InStringLiteral
    );
}

#[test]
fn unit_classify_heredoc_brace_interpolation() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    let content = "<?php\n$x = <<<EOT\n{$f->}\nEOT;\n";
    // Line 2: {=0 $=1 f=2 -=3 >=4 }=5
    let pos = Position {
        line: 2,
        character: 5,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::BraceInterpolation
    );
}

#[test]
fn unit_classify_after_string() {
    use phpantom_lsp::completion::comment_position::{StringContext, classify_string_context};
    let content = "<?php\n$x = 'hello'; $y = 1;\n";
    // Position after the string, in normal code
    let pos = Position {
        line: 1,
        character: 20,
    };
    assert_eq!(
        classify_string_context(content, pos),
        StringContext::NotInString
    );
}
