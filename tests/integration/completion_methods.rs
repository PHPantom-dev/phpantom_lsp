use crate::common::{completion_response, create_test_backend, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Regression test: completion on `collect($var)->` resolves Collection members.
///
/// The `collect()` helper returns `Collection` and the cursor is right
/// after `->` — the subject is `collect($paymentOptions)` which is a
/// standalone function call whose return type is a class.
#[tokio::test]
async fn test_completion_on_function_call_arrow() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///collect_map.php").unwrap();
    let text = r#"<?php

class Collection {
    /** @return static */
    public function map(callable $callback): static {}

    /** @return static */
    public function values(): static {}

    /** @return mixed */
    public function first(): mixed {}
}

/**
 * @return Collection
 */
function collect($value = []): Collection
{
    return new Collection($value);
}

class PaymentOptionLocale {
    public bool $tokens_enabled;
}

class PaymentService {
    public function getOptions(array $paymentOptions): void {
        $formattedOptions = collect($paymentOptions)->map(function (PaymentOptionLocale $optionLocale) {
            return [
                'tokens_enabled' => $optionLocale->tokens_enabled,
            ];
        })->values();
        $formattedOptions->
    }
}
"#;

    open_php(&backend, &uri, text).await;

    // ── Test 1: completion after `$formattedOptions->` ──
    let target_line = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.trim() == "$formattedOptions->")
        .map(|(i, _)| i)
        .expect("should find $formattedOptions-> line");

    let result = completion_response(&backend, &uri, target_line as u32, 28, None).await;
    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("map")),
        "Expected 'map' in completions for Collection, got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("values")),
        "Expected 'values' in completions for Collection, got: {:?}",
        labels
    );

    // ── Test 2: completion after `collect($paymentOptions)->` ──
    // The cursor is right after `->` before `map` on the chained call line.
    let chain_line = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("collect($paymentOptions)->map("))
        .map(|(i, _)| i)
        .expect("should find collect()->map( line");

    let chain_line_text = text.lines().nth(chain_line).unwrap();
    let arrow_col = chain_line_text.find("->map(").unwrap() as u32 + 2; // after `->`

    // Must not crash and should offer Collection members
    // after `->map`
    let result2 = completion_response(&backend, &uri, chain_line as u32, arrow_col + 3, None).await;
    let items2 = match result2 {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };
    let labels2: Vec<&str> = items2.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels2.iter().any(|l| l.starts_with("map")),
        "Expected 'map' in completions on chained collect()->, got: {:?}",
        labels2
    );
}

// ─── Method Insert Text with Parameters ─────────────────────────────────────

#[tokio::test]
async fn test_completion_method_insert_text_no_params() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///insert.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    function render() {}\n",
        "    function update() {}\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    )
    .to_string();

    open_php(&backend, &uri, &text).await;

    // Cursor right after `$this->` on line 5
    let result = completion_response(&backend, &uri, 5, 15, None).await;
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .collect();
            for item in &method_items {
                let insert = item.insert_text.as_deref().unwrap();
                let filter = item.filter_text.as_deref().unwrap();
                // insert_text should be a snippet: methodName()$0 (no required params)
                let expected = format!("{}()$0", filter);
                assert_eq!(
                    insert, expected,
                    "insert_text should be a snippet with parens for '{}'",
                    filter
                );
                assert_eq!(
                    item.insert_text_format,
                    Some(InsertTextFormat::SNIPPET),
                    "insert_text_format should be Snippet"
                );
                // label should be the full signature, e.g. "render()"
                assert!(
                    item.label.starts_with(filter),
                    "Label '{}' should start with method name '{}'",
                    item.label,
                    filter
                );
            }
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_method_insert_text_with_required_params() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///params.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Editor {\n",
        "    function updateText(string $text, $frogs = false): void {}\n",
        "    function setTitle(string $title): void {}\n",
        "    function reset() {}\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    )
    .to_string();

    open_php(&backend, &uri, &text).await;

    // Cursor right after `$this->` on line 6
    let result = completion_response(&backend, &uri, 6, 15, None).await;
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .collect();
            assert_eq!(
                method_items.len(),
                4,
                "Should have 4 method completions (3 original + test)"
            );

            // Find specific methods by filter_text (method name)
            // updateText(string $text, $frogs = false) — one required param
            let update_text = method_items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("updateText"))
                .expect("Should have updateText");
            assert_eq!(
                update_text.insert_text.as_deref(),
                Some("updateText(${1:\\$text})$0"),
                "insert_text should be a snippet with required param $text"
            );
            assert_eq!(
                update_text.insert_text_format,
                Some(InsertTextFormat::SNIPPET),
            );

            // setTitle(string $title) — one required param
            let set_title = method_items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("setTitle"))
                .expect("Should have setTitle");
            assert_eq!(
                set_title.insert_text.as_deref(),
                Some("setTitle(${1:\\$title})$0"),
                "insert_text should be a snippet with required param $title"
            );
            assert_eq!(
                set_title.insert_text_format,
                Some(InsertTextFormat::SNIPPET),
            );

            // reset() — no params
            let reset = method_items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("reset"))
                .expect("Should have reset");
            assert_eq!(
                reset.insert_text.as_deref(),
                Some("reset()$0"),
                "insert_text should be a snippet with empty parens"
            );
            assert_eq!(reset.insert_text_format, Some(InsertTextFormat::SNIPPET),);
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_method_insert_text_multiple_required_params() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///multi_params.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Calculator {\n",
        "    function add(int $a, int $b): int {}\n",
        "    function addWithLabel(int $a, int $b, string $label = 'sum'): int {}\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    )
    .to_string();

    open_php(&backend, &uri, &text).await;

    // Cursor right after `$this->` on line 5
    let result = completion_response(&backend, &uri, 5, 15, None).await;
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .collect();

            // add(int $a, int $b) — two required params
            let add = method_items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("add"))
                .expect("Should have add");
            assert_eq!(
                add.insert_text.as_deref(),
                Some("add(${1:\\$a}, ${2:\\$b})$0"),
                "insert_text should be a snippet with two required params"
            );
            assert_eq!(add.insert_text_format, Some(InsertTextFormat::SNIPPET),);

            // addWithLabel(int $a, int $b, string $label = 'sum') — two required, one optional
            let add_with_label = method_items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("addWithLabel"))
                .expect("Should have addWithLabel");
            assert_eq!(
                add_with_label.insert_text.as_deref(),
                Some("addWithLabel(${1:\\$a}, ${2:\\$b})$0"),
                "insert_text should include only the two required params"
            );
            assert_eq!(
                add_with_label.insert_text_format,
                Some(InsertTextFormat::SNIPPET),
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_method_insert_text_variadic_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///variadic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    function log(string $message, ...$context): void {}\n",
        "    function logAll(...$messages): void {}\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    )
    .to_string();

    open_php(&backend, &uri, &text).await;

    // Cursor right after `$this->` on line 5
    let result = completion_response(&backend, &uri, 5, 15, None).await;
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .collect();

            // log(string $message, ...$context) — one required, one variadic
            let log = method_items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("log"))
                .expect("Should have log");
            assert_eq!(
                log.insert_text.as_deref(),
                Some("log(${1:\\$message})$0"),
                "insert_text should include the required param but not the variadic"
            );
            assert_eq!(log.insert_text_format, Some(InsertTextFormat::SNIPPET),);

            // logAll(...$messages) — only variadic, no required
            let log_all = method_items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("logAll"))
                .expect("Should have logAll");
            assert_eq!(
                log_all.insert_text.as_deref(),
                Some("logAll()$0"),
                "insert_text should be empty parens (variadic is not required)"
            );
            assert_eq!(log_all.insert_text_format, Some(InsertTextFormat::SNIPPET),);
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_method_all_optional_params() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///optional.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    function setup($debug = false, $verbose = false): void {}\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    )
    .to_string();

    open_php(&backend, &uri, &text).await;

    // Cursor right after `$this->` on line 4
    let result = completion_response(&backend, &uri, 4, 15, None).await;
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            // setup($debug = false, $verbose = false) — all optional
            let setup = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("setup"))
                .expect("Should have setup");
            assert_eq!(
                setup.insert_text.as_deref(),
                Some("setup()$0"),
                "insert_text should be empty parens (all params are optional)"
            );
            assert_eq!(setup.insert_text_format, Some(InsertTextFormat::SNIPPET),);
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_method_detail_shows_signature() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///detail.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Editor {\n",
        "    function updateText(string $text, $frogs = false): void {}\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    )
    .to_string();

    open_php(&backend, &uri, &text).await;

    // Cursor right after `$this->` on line 4
    let result = completion_response(&backend, &uri, 4, 15, None).await;
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let update = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("updateText"))
                .expect("Should have updateText");

            // Label should show the full signature
            assert_eq!(
                update.label, "updateText($text, $frogs = ...)",
                "Label should show method name and parameter names"
            );

            // Detail shows the return type.
            assert_eq!(
                update.detail.as_deref(),
                Some("void"),
                "Method detail should show the return type"
            );
            // label_details.detail is None (detail field handles the
            // return type display).
            assert_eq!(
                update
                    .label_details
                    .as_ref()
                    .and_then(|ld| ld.detail.as_deref()),
                None,
                "label_details.detail should be None"
            );
            // label_details.description shows the owning class name
            // (visible in VS Code, ignored by Zed).
            assert_eq!(
                update
                    .label_details
                    .as_ref()
                    .and_then(|ld| ld.description.as_deref()),
                Some("Editor"),
                "label_details.description should show the class name"
            );

            // insert_text should be a snippet with the required param
            assert_eq!(
                update.insert_text.as_deref(),
                Some("updateText(${1:\\$text})$0"),
                "insert_text should be a snippet with required param $text"
            );
            assert_eq!(update.insert_text_format, Some(InsertTextFormat::SNIPPET),);
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_resolve_union_member_shows_all_branches() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///union_resolve.php").unwrap();
    let text = r#"<?php
class Lamp {
    public function turnOff(): void {}
    public function dim(): void {}
}

class Faucet {
    public function turnOff(): void {}
    public function drip(): void {}
}

class Consumer {
    public function run(): void {
        if (rand(0, 1)) {
            $ambiguous = new Lamp();
        } else {
            $ambiguous = new Faucet();
        }
        $ambiguous->
    }
}
"#
    .to_string();

    open_php(&backend, &uri, &text).await;

    // Cursor right after `$ambiguous->` on line 18
    let result = completion_response(&backend, &uri, 18, 20, None).await;
    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    // Find the turnOff item (present on both branches).
    let turn_off = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("turnOff"))
        .expect("Should have turnOff in completions");

    // Resolve the item to get documentation.
    let resolved = backend.completion_resolve(turn_off.clone()).await.unwrap();
    let doc = match resolved.documentation {
        Some(Documentation::MarkupContent(mc)) => mc.value,
        other => panic!("Expected MarkupContent documentation, got: {:?}", other),
    };

    // The resolved documentation should show both classes.
    assert!(
        doc.contains("Lamp") && doc.contains("Faucet"),
        "resolved documentation should show both Lamp and Faucet, got: {}",
        doc
    );

    // The two branches should be separated by a horizontal rule.
    assert!(
        doc.contains("---"),
        "resolved documentation should use a horizontal rule separator, got: {}",
        doc
    );

    // Now resolve dim() — only on Lamp, should show a single class.
    let dim = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("dim"))
        .expect("Should have dim in completions");

    let resolved_dim = backend.completion_resolve(dim.clone()).await.unwrap();
    let dim_doc = match resolved_dim.documentation {
        Some(Documentation::MarkupContent(mc)) => mc.value,
        other => panic!("Expected MarkupContent documentation, got: {:?}", other),
    };

    assert!(
        dim_doc.contains("Lamp"),
        "dim documentation should show Lamp, got: {}",
        dim_doc
    );
    assert!(
        !dim_doc.contains("Faucet"),
        "dim documentation should NOT show Faucet, got: {}",
        dim_doc
    );
    assert!(
        !dim_doc.contains("---"),
        "single-branch member should not have separator, got: {}",
        dim_doc
    );
}

/// A member's documentation is found from its owner's FQN, so a class the
/// completing file never imports still gets one.
#[tokio::test]
async fn test_completion_resolve_member_of_class_not_imported() {
    let backend = create_test_backend();
    backend.update_ast(
        "file:///lib_widget.php",
        r#"<?php
namespace Lib;
class Widget {
    /** Spins the widget. */
    public function spin(): void {}
}
"#,
    );

    let uri = Url::parse("file:///not_imported_resolve.php").unwrap();
    let text = r#"<?php
namespace App;
class Consumer {
    public function run(\Lib\Widget $w): void {
        $w->
    }
}
"#
    .to_string();

    open_php(&backend, &uri, &text).await;

    let result = completion_response(&backend, &uri, 4, 12, None).await;
    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    let spin = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("spin"))
        .expect("Should have spin in completions");

    let resolved = backend.completion_resolve(spin.clone()).await.unwrap();
    let doc = match resolved.documentation {
        Some(Documentation::MarkupContent(mc)) => mc.value,
        other => panic!("Expected MarkupContent documentation, got: {:?}", other),
    };
    assert!(
        doc.contains("Spins the widget."),
        "resolved documentation should describe Lib\\Widget::spin, got: {}",
        doc
    );
}

#[tokio::test]
async fn test_completion_resolve_union_deduplicates_common_ancestor() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///union_dedup.php").unwrap();
    let text = r#"<?php
class BaseDevice {
    public function turnOff(): void {}
}

class Lamp extends BaseDevice {
    public function dim(): void {}
}

class Faucet extends BaseDevice {
    public function drip(): void {}
}

class Consumer {
    public function run(): void {
        if (rand(0, 1)) {
            $ambiguous = new Lamp();
        } else {
            $ambiguous = new Faucet();
        }
        $ambiguous->
    }
}
"#
    .to_string();

    open_php(&backend, &uri, &text).await;

    // Cursor right after `$ambiguous->` on line 20
    let result = completion_response(&backend, &uri, 20, 20, None).await;
    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    let turn_off = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("turnOff"))
        .expect("Should have turnOff in completions");

    let resolved = backend.completion_resolve(turn_off.clone()).await.unwrap();
    let doc = match resolved.documentation {
        Some(Documentation::MarkupContent(mc)) => mc.value,
        other => panic!("Expected MarkupContent documentation, got: {:?}", other),
    };

    // Both Lamp and Faucet inherit turnOff from BaseDevice.  The
    // resolved hover should show BaseDevice (the declaring class)
    // and should NOT be duplicated.
    assert!(
        doc.contains("BaseDevice"),
        "documentation should show declaring class BaseDevice, got: {}",
        doc
    );
    assert!(
        !doc.contains("---"),
        "should not have separator when both branches resolve to same declaring class, got: {}",
        doc
    );
}
