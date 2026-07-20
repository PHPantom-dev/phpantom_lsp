use crate::common::{
    create_psr4_workspace, create_test_backend, create_test_backend_with_function_stubs,
};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file and request inlay hints for the entire file.
async fn inlay_hints_for(backend: &phpantom_lsp::Backend, uri: &Url, text: &str) -> Vec<InlayHint> {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let line_count = text.lines().count() as u32;
    let last_line_len = text.lines().last().map(|l| l.len() as u32).unwrap_or(0);

    let range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: line_count,
            character: last_line_len,
        },
    };

    backend
        .handle_inlay_hints(uri.as_ref(), text, range)
        .unwrap_or_default()
}

/// Extract the label string from an InlayHint.
fn hint_label(hint: &InlayHint) -> String {
    match &hint.label {
        InlayHintLabel::String(s) => s.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|p| p.value.as_str()).collect(),
    }
}

/// Collect the labels from a slice of hints.
fn labels(hints: &[&InlayHint]) -> Vec<String> {
    hints.iter().map(|h| hint_label(h)).collect()
}

/// Find all hints at a specific line.
fn hints_at_line(hints: &[InlayHint], line: u32) -> Vec<&InlayHint> {
    hints.iter().filter(|h| h.position.line == line).collect()
}

// ─── Basic function call hints ──────────────────────────────────────────────

#[tokio::test]
async fn standalone_function_two_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function greet(string $name, int $age): string { return ''; }
greet('Alice', 25);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 2);

    assert_eq!(
        line_hints.len(),
        2,
        "expected 2 hints, got {:?}",
        labels(&line_hints)
    );
    assert_eq!(hint_label(line_hints[0]), "name:");
    assert_eq!(hint_label(line_hints[1]), "age:");
}

#[tokio::test]
async fn no_hints_for_zero_arg_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function doStuff(): void {}
doStuff();
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    assert!(hints.is_empty(), "expected no hints for zero-arg call");
}

#[tokio::test]
async fn hint_kind_is_parameter() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function foo(string $bar): void {}
foo('hello');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].kind, Some(InlayHintKind::PARAMETER));
}

// ─── Suppression: variable name matches parameter name ──────────────────────

#[tokio::test]
async fn suppress_when_variable_matches_param_name() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function greet(string $name): void {}
$name = 'Alice';
greet($name);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 3);
    assert!(
        line_hints.is_empty(),
        "hint should be suppressed when variable matches param name"
    );
}

#[tokio::test]
async fn no_suppress_when_variable_differs() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function greet(string $name): void {}
$foo = 'Alice';
greet($foo);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 3);
    assert_eq!(line_hints.len(), 1);
    assert_eq!(hint_label(line_hints[0]), "name:");
}

// ─── Suppression: property access trailing identifier matches ───────────────

#[tokio::test]
async fn suppress_when_property_matches_param_name() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
class Obj { public string $name = ''; }
function greet(string $name): void {}
$obj = new Obj();
greet($obj->name);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 4);
    assert!(
        line_hints.is_empty(),
        "hint should be suppressed when property matches param name"
    );
}

// ─── Suppression: named arguments ───────────────────────────────────────────

#[tokio::test]
async fn suppress_for_named_arguments() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function greet(string $name, int $age): void {}
greet(name: 'Alice', age: 25);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 2);
    assert!(
        line_hints.is_empty(),
        "hints should be suppressed for named arguments"
    );
}

#[tokio::test]
async fn mixed_named_and_positional() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function greet(string $name, int $age, string $city): void {}
greet('Alice', age: 25, 'NYC');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 2);
    // Only the positional args should get hints (first and third),
    // not the named arg (second).
    let lbls = labels(&line_hints);
    assert!(
        lbls.contains(&"name:".to_string()),
        "expected name: hint, got {:?}",
        lbls
    );
    assert!(
        !lbls.contains(&"age:".to_string()),
        "age: should be suppressed"
    );
    assert!(
        lbls.contains(&"city:".to_string()),
        "expected city: hint for third arg, got {:?}",
        lbls
    );
}

#[tokio::test]
async fn named_arg_before_positional_maps_correctly() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    // Named arg `city:` consumes param 2, so the positional arg 'Alice'
    // should map to param 0 (`name:`), not param 1 (`age:`).
    let text = r#"<?php
function greet(string $name, int $age, string $city): void {}
greet(city: 'NYC', 'Alice');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 2);
    let lbls = labels(&line_hints);
    assert!(
        lbls.contains(&"name:".to_string()),
        "expected name: hint for positional arg after named, got {:?}",
        lbls
    );
    assert!(
        !lbls.contains(&"age:".to_string()),
        "positional arg should not get age: hint, got {:?}",
        lbls
    );
    assert!(
        !lbls.contains(&"city:".to_string()),
        "city: is a named arg and should be suppressed, got {:?}",
        lbls
    );
}

#[tokio::test]
async fn multiple_named_args_with_positional_remainder() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    // Two named args consume params 0 and 2, leaving param 1 (`age:`)
    // for the single positional argument.
    let text = r#"<?php
function greet(string $name, int $age, string $city): void {}
greet(name: 'Alice', city: 'NYC', 30);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 2);
    let lbls = labels(&line_hints);
    assert!(
        lbls.contains(&"age:".to_string()),
        "expected age: hint for remaining positional arg, got {:?}",
        lbls
    );
    assert_eq!(
        lbls.len(),
        1,
        "expected exactly one hint (the positional arg), got {:?}",
        lbls
    );
}

#[tokio::test]
async fn named_arg_out_of_order_two_positional() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    // Named arg `age:` consumes param 1.  Remaining params in order
    // are 0 (`name`) and 2 (`city`).  The two positional args should
    // map to those in order.
    let text = r#"<?php
function greet(string $name, int $age, string $city): void {}
greet(age: 25, 'Alice', 'NYC');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 2);
    let lbls = labels(&line_hints);
    assert!(
        lbls.contains(&"name:".to_string()),
        "first positional should be name:, got {:?}",
        lbls
    );
    assert!(
        lbls.contains(&"city:".to_string()),
        "second positional should be city:, got {:?}",
        lbls
    );
    assert!(
        !lbls.contains(&"age:".to_string()),
        "age: is named and should be suppressed, got {:?}",
        lbls
    );
}

// ─── By-reference indicator ─────────────────────────────────────────────────

#[tokio::test]
async fn by_reference_indicator() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function modify(array &$data, string $label): void {}
$arr = [];
modify($arr, 'test');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 3);

    // $arr matches $data → suppression of name, but &indicator still shows.
    // Actually $arr does NOT match $data, so we get the full hint.
    let lbls = labels(&line_hints);
    assert!(
        lbls.iter().any(|l| l.contains('&')),
        "expected by-reference indicator, got {:?}",
        lbls
    );
}

#[tokio::test]
async fn by_reference_with_matching_name_still_shows() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function modify(array &$data): void {}
$data = [];
modify($data);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 3);

    // Even though variable name matches, the & hint should still appear.
    assert_eq!(
        line_hints.len(),
        1,
        "expected 1 hint for by-reference param"
    );
    assert!(
        hint_label(line_hints[0]).contains('&'),
        "expected & in label: {}",
        hint_label(line_hints[0])
    );
}

// ─── Method calls ───────────────────────────────────────────────────────────

#[tokio::test]
async fn instance_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
class Greeter {
    public function greet(string $name, int $age): void {}
}
class Demo {
    public function run(): void {
        $g = new Greeter();
        $g->greet('Alice', 25);
    }
}
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 7);

    let lbls = labels(&line_hints);
    assert!(
        lbls.contains(&"name:".to_string()),
        "expected name: hint, got {:?}",
        lbls
    );
    assert!(
        lbls.contains(&"age:".to_string()),
        "expected age: hint, got {:?}",
        lbls
    );
}

#[tokio::test]
async fn static_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
class Greeter {
    public static function greet(string $name, int $age): void {}
}
Greeter::greet('Alice', 25);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 4);

    let lbls = labels(&line_hints);
    assert!(lbls.contains(&"name:".to_string()), "expected name: hint");
    assert!(lbls.contains(&"age:".to_string()), "expected age: hint");
}

#[tokio::test]
async fn constructor_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
class User {
    public function __construct(string $name, int $age) {}
}
$u = new User('Alice', 25);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 4);

    let lbls = labels(&line_hints);
    assert!(lbls.contains(&"name:".to_string()), "expected name: hint");
    assert!(lbls.contains(&"age:".to_string()), "expected age: hint");
}

// ─── Variadic parameters ────────────────────────────────────────────────────

#[tokio::test]
async fn variadic_parameter_hints() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function log(string $level, string ...$messages): void {}
log('info', 'hello', 'world');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 2);

    // All three arguments should get hints.
    assert!(
        line_hints.len() >= 2,
        "expected at least 2 hints, got {}",
        line_hints.len()
    );
    assert_eq!(hint_label(line_hints[0]), "level:");
    // The variadic args map to the same param name.
    assert_eq!(hint_label(line_hints[1]), "messages:");
}

// ─── Obvious single-param suppression ───────────────────────────────────────

#[tokio::test]
async fn suppress_obvious_single_param_functions() {
    let backend = create_test_backend_with_function_stubs();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
$x = count([1, 2, 3]);
$y = strlen('hello');
$z = json_encode(['a' => 1]);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    // These well-known single-param functions should have their hints suppressed.
    let line1 = hints_at_line(&hints, 1);
    let line2 = hints_at_line(&hints, 2);
    let line3 = hints_at_line(&hints, 3);

    assert!(
        line1.is_empty(),
        "count() hint should be suppressed, got {:?}",
        labels(&line1)
    );
    assert!(
        line2.is_empty(),
        "strlen() hint should be suppressed, got {:?}",
        labels(&line2)
    );
    assert!(
        line3.is_empty(),
        "json_encode() hint should be suppressed, got {:?}",
        labels(&line3)
    );
}

// ─── $this->method() calls ──────────────────────────────────────────────────

#[tokio::test]
async fn this_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
class Calculator {
    public function add(int $a, int $b): int { return $a + $b; }
    public function demo(): void {
        $this->add(1, 2);
    }
}
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 4);

    let lbls = labels(&line_hints);
    assert!(lbls.contains(&"a:".to_string()), "expected a: hint");
    assert!(lbls.contains(&"b:".to_string()), "expected b: hint");
}

// ─── Cross-file PSR-4 ───────────────────────────────────────────────────────

#[tokio::test]
async fn cross_file_psr4_method_hints() {
    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let service_php = r#"<?php
namespace App;
class Service {
    public function process(string $input, int $retries): string {
        return $input;
    }
}
"#;

    let main_php = r#"<?php
namespace App;
class Main {
    public function run(): void {
        $svc = new Service();
        $svc->process('data', 3);
    }
}
"#;

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[("src/Service.php", service_php), ("src/Main.php", main_php)],
    );

    let main_uri = Url::parse("file:///test/Main.php").unwrap();
    let hints = inlay_hints_for(&backend, &main_uri, main_php).await;
    let line_hints = hints_at_line(&hints, 5);

    let lbls = labels(&line_hints);
    assert!(
        lbls.contains(&"input:".to_string()),
        "expected input: hint from cross-file class, got {:?}",
        lbls
    );
    assert!(
        lbls.contains(&"retries:".to_string()),
        "expected retries: hint from cross-file class, got {:?}",
        lbls
    );
}

// ─── Viewport range filtering ───────────────────────────────────────────────

#[tokio::test]
async fn only_hints_in_requested_range() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function foo(string $a): void {}
foo('line2');
foo('line3');
foo('line4');
"#;

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Request only lines 2-3 (0-based), which covers `foo('line2')` and `foo('line3')`.
    let range = Range {
        start: Position {
            line: 2,
            character: 0,
        },
        end: Position {
            line: 3,
            character: 20,
        },
    };

    let hints = backend
        .handle_inlay_hints(uri.as_ref(), text, range)
        .unwrap_or_default();

    // Should get hints for lines 2 and 3 only, not line 4.
    assert!(
        hints
            .iter()
            .all(|h| h.position.line >= 2 && h.position.line <= 3),
        "hints should be within range, got lines {:?}",
        hints.iter().map(|h| h.position.line).collect::<Vec<_>>()
    );
}

// ─── String literal matching suppression ────────────────────────────────────

#[tokio::test]
async fn suppress_string_literal_matching_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function lookup(string $key): mixed { return null; }
lookup('key');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 2);
    assert!(
        line_hints.is_empty(),
        "hint should be suppressed when string literal matches param name"
    );
}

// ─── Tooltip shows type info ────────────────────────────────────────────────

#[tokio::test]
async fn tooltip_shows_type_and_name() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function greet(string $name): void {}
greet('Alice');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    assert_eq!(hints.len(), 1);

    if let Some(InlayHintTooltip::String(tooltip)) = &hints[0].tooltip {
        assert!(
            tooltip.contains("string"),
            "tooltip should contain type info: {}",
            tooltip
        );
        assert!(
            tooltip.contains("$name"),
            "tooltip should contain param name: {}",
            tooltip
        );
    }
}

// ─── Padding ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn hint_has_right_padding() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function foo(string $bar): void {}
foo('hello');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].padding_right, Some(true));
}

// ─── Multiple calls on separate lines ───────────────────────────────────────

#[tokio::test]
async fn multiple_calls_each_get_hints() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function add(int $a, int $b): int { return $a + $b; }
$x = add(1, 2);
$y = add(3, 4);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line2 = hints_at_line(&hints, 2);
    let line3 = hints_at_line(&hints, 3);

    assert_eq!(line2.len(), 2, "expected 2 hints on line 2");
    assert_eq!(line3.len(), 2, "expected 2 hints on line 3");
}

// ─── Nested calls ───────────────────────────────────────────────────────────

#[tokio::test]
async fn nested_calls_both_get_hints() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function inner(int $x): int { return $x; }
function outer(int $y): int { return $y; }
outer(inner(42));
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 3);

    let lbls = labels(&line_hints);
    assert!(
        lbls.contains(&"y:".to_string()),
        "expected y: for outer call, got {:?}",
        lbls
    );
    assert!(
        lbls.contains(&"x:".to_string()),
        "expected x: for inner call, got {:?}",
        lbls
    );
}

// ─── Case-insensitive snake/camel suppression ───────────────────────────────

#[tokio::test]
async fn suppress_case_insensitive_match() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function process(string $userName): void {}
$user_name = 'Alice';
process($user_name);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 3);
    assert!(
        line_hints.is_empty(),
        "hint should be suppressed for snake_case matching camelCase param"
    );
}

// ─── Stub function hints ────────────────────────────────────────────────────

#[tokio::test]
async fn stub_function_multi_param_shows_hints() {
    let backend = create_test_backend_with_function_stubs();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    // Wrap the call inside a function body so that the file context
    // has a proper scope for resolution.  Top-level calls in a bare
    // <?php file sometimes lack the context needed for stub resolution.
    let text = r#"<?php
function demo(): void {
    str_contains('hello world', 'foo');
}
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 2);

    let lbls = labels(&line_hints);
    assert!(
        lbls.contains(&"haystack:".to_string()),
        "expected haystack: hint, got {:?}",
        lbls
    );
    assert!(
        lbls.contains(&"needle:".to_string()),
        "expected needle: hint, got {:?}",
        lbls
    );
}

// ─── Spread argument suppression ────────────────────────────────────────────

#[tokio::test]
async fn spread_argument_gets_no_hint() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function greet(string $name, int $age, string $city): void {}
$args = ['Alice', 25, 'NYC'];
greet(...$args);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 3);

    assert!(
        line_hints.is_empty(),
        "spread argument should not get a parameter hint, got {:?}",
        labels(&line_hints)
    );
}

#[tokio::test]
async fn spread_after_positional_suppresses_only_spread() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function greet(string $name, int $age, string $city): void {}
$rest = [25, 'NYC'];
greet('Alice', ...$rest);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 3);

    // The first positional argument should still get its hint.
    assert_eq!(
        line_hints.len(),
        1,
        "expected 1 hint for the positional arg, got {:?}",
        labels(&line_hints)
    );
    assert_eq!(hint_label(line_hints[0]), "name:");
}

#[tokio::test]
async fn positional_args_before_and_after_spread_only_spread_suppressed() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    // PHP allows positional args after a spread in some cases.
    // Even if it's unusual, we should only suppress the spread arg.
    let text = r#"<?php
function multi(string $a, int $b, string $c): void {}
$mid = [42];
multi('first', ...$mid, 'last');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let line_hints = hints_at_line(&hints, 3);

    let lbls = labels(&line_hints);
    // The spread argument (...$mid) should have no hint.
    // The positional arguments should still get hints.
    assert!(
        lbls.contains(&"a:".to_string()),
        "expected a: hint for first positional arg, got {:?}",
        lbls
    );
    assert!(
        !lbls.iter().any(|l| l == "b:"),
        "spread arg should not get b: hint, got {:?}",
        lbls
    );
}

// ─── No return type hints on named functions/methods ────────────────────────

#[tokio::test]
async fn no_return_type_hint_on_named_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
function doubled()
{
    return 42;
}
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let type_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE))
        .collect();
    assert!(
        type_hints.is_empty(),
        "named functions should not get return type hints, got {:?}",
        type_hints.iter().map(|h| hint_label(h)).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn no_return_type_hint_on_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
class Calc {
    public function answer()
    {
        return 42;
    }
}
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let type_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE))
        .collect();
    assert!(
        type_hints.is_empty(),
        "methods should not get return type hints, got {:?}",
        type_hints.iter().map(|h| hint_label(h)).collect::<Vec<_>>()
    );
}

// ─── Closure return type hints from callable context ────────────────────────

#[tokio::test]
async fn closure_return_type_hint_from_callable_signature() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
/**
 * @param callable(int): string $fn
 */
function transform(callable $fn): void {}

transform(function ($x) { return (string) $x; });
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let return_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && hint_label(h).starts_with(": "))
        .collect();
    assert!(
        !return_hints.is_empty(),
        "expected a return type hint for untyped closure, got: {:?}",
        hints.iter().map(hint_label).collect::<Vec<_>>()
    );
    assert_eq!(
        hint_label(return_hints[0]),
        ": string",
        "closure return type should come from the callable signature"
    );
}

#[tokio::test]
async fn arrow_fn_return_type_hint_from_callable_signature() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
/**
 * @param callable(int): string $fn
 */
function transform(callable $fn): void {}

transform(fn ($x) => (string) $x);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let return_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && hint_label(h).starts_with(": "))
        .collect();
    assert!(
        !return_hints.is_empty(),
        "expected a return type hint for arrow fn, got: {:?}",
        hints.iter().map(hint_label).collect::<Vec<_>>()
    );
    assert_eq!(
        hint_label(return_hints[0]),
        ": string",
        "arrow fn return type should come from the callable signature"
    );
}

#[tokio::test]
async fn no_closure_return_type_hint_when_already_typed() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
/**
 * @param callable(int): string $fn
 */
function transform(callable $fn): void {}

transform(function ($x): string { return (string) $x; });
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let return_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && hint_label(h).starts_with(": "))
        .collect();
    assert!(
        return_hints.is_empty(),
        "should not show return type hint when closure already has one, got {:?}",
        return_hints
            .iter()
            .map(|h| hint_label(h))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn closure_return_type_hint_shortened_fqn() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inlay.php").unwrap();
    let text = r#"<?php
namespace App;
class User { public string $name = ''; }
class Service {
    /**
     * @param callable(User): User $fn
     */
    public function apply(callable $fn): void {}
}

$s = new Service();
$s->apply(fn ($u) => $u);
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;
    let return_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && hint_label(h).starts_with(": "))
        .collect();
    assert!(
        !return_hints.is_empty(),
        "expected a return type hint, got: {:?}",
        hints.iter().map(hint_label).collect::<Vec<_>>()
    );
    // Should show the short name, not the FQN.
    assert_eq!(
        hint_label(return_hints[0]),
        ": User",
        "return type hint should use short name, not FQN"
    );
}

// ─── Closure parameter type hints ───────────────────────────────────────────

#[tokio::test]
async fn closure_param_type_hint_from_callable_signature() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = r#"<?php
/**
 * @param callable(int, string): void $callback
 */
function doStuff(callable $callback): void {}

doStuff(function ($a, $b) {});
"#;
    let hints = inlay_hints_for(&backend, &uri, text).await;
    // Filter to param type hints (not return type hints which start with ":").
    let param_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && !hint_label(h).starts_with(':'))
        .collect();
    assert!(
        param_hints.len() >= 2,
        "expected at least 2 closure param type hints, got {}: {:?}",
        param_hints.len(),
        param_hints
            .iter()
            .map(|h| hint_label(h))
            .collect::<Vec<_>>()
    );
    let hint_labels: Vec<String> = param_hints.iter().map(|h| hint_label(h)).collect();
    assert!(
        hint_labels.iter().any(|l| l.contains("int")),
        "expected an 'int' type hint, got: {:?}",
        hint_labels
    );
    assert!(
        hint_labels.iter().any(|l| l.contains("string")),
        "expected a 'string' type hint, got: {:?}",
        hint_labels
    );
}

#[tokio::test]
async fn arrow_fn_param_type_hint_from_callable_signature() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = r#"<?php
/**
 * @param callable(int): string $fn
 */
function transform(callable $fn): void {}

transform(fn ($x) => (string) $x);
"#;
    let hints = inlay_hints_for(&backend, &uri, text).await;
    let param_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && !hint_label(h).starts_with(':'))
        .collect();
    assert!(
        param_hints.iter().any(|h| hint_label(h).contains("int")),
        "expected an 'int' type hint for arrow fn param, got: {:?}",
        param_hints
            .iter()
            .map(|h| hint_label(h))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn no_closure_param_hint_when_already_typed() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = r#"<?php
/**
 * @param callable(int, string): void $callback
 */
function doStuff(callable $callback): void {}

doStuff(function (int $a, string $b) {});
"#;
    let hints = inlay_hints_for(&backend, &uri, text).await;
    // No param TYPE hints should appear for already-typed closure params.
    let param_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && !hint_label(h).starts_with(':'))
        .collect();
    assert!(
        param_hints.is_empty(),
        "expected no closure param type hints for fully typed params, got: {:?}",
        param_hints
            .iter()
            .map(|h| hint_label(h))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn closure_param_hint_partial_typing() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = r#"<?php
/**
 * @param callable(int, string): void $callback
 */
function doStuff(callable $callback): void {}

doStuff(function (int $a, $b) {});
"#;
    let hints = inlay_hints_for(&backend, &uri, text).await;
    // Only the untyped $b should get a param type hint.
    let param_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && !hint_label(h).starts_with(':'))
        .collect();
    assert!(
        param_hints.iter().any(|h| hint_label(h).contains("string")),
        "expected a 'string' type hint for untyped $b, got: {:?}",
        param_hints
            .iter()
            .map(|h| hint_label(h))
            .collect::<Vec<_>>()
    );
    // Should not have an 'int' param hint since $a is already typed.
    let int_hints: Vec<_> = param_hints
        .iter()
        .filter(|h| hint_label(h).contains("int"))
        .collect();
    assert!(
        int_hints.is_empty(),
        "expected no 'int' type hint for already-typed $a, got: {:?}",
        int_hints.iter().map(|h| hint_label(h)).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn closure_param_hint_kind_is_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = r#"<?php
/**
 * @param callable(int): void $fn
 */
function run(callable $fn): void {}

run(function ($x) {});
"#;
    let hints = inlay_hints_for(&backend, &uri, text).await;
    let type_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE))
        .collect();
    assert!(!type_hints.is_empty(), "expected at least one type hint");
    for hint in &type_hints {
        assert_eq!(hint.kind, Some(InlayHintKind::TYPE));
    }
}

#[tokio::test]
async fn closure_param_hint_has_trailing_space() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = r#"<?php
/**
 * @param callable(int): void $fn
 */
function run(callable $fn): void {}

run(function ($x) {});
"#;
    let hints = inlay_hints_for(&backend, &uri, text).await;
    // Filter to param TYPE hints only (not return type hints).
    let closure_param_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && !hint_label(h).starts_with(':'))
        .collect();
    assert!(
        !closure_param_hints.is_empty(),
        "expected at least one closure param type hint"
    );
    // The label should end with a space (format is "type ").
    for hint in &closure_param_hints {
        let label = hint_label(hint);
        assert!(
            label.ends_with(' '),
            "closure param type hint label should end with a space, got: {:?}",
            label
        );
    }
}

#[tokio::test]
async fn closure_param_hint_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = r#"<?php
class Processor {
    /**
     * @param callable(string): bool $filter
     */
    public function filter(callable $filter): void {}
}

$p = new Processor();
$p->filter(function ($item) { return true; });
"#;
    let hints = inlay_hints_for(&backend, &uri, text).await;
    let param_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && !hint_label(h).starts_with(':'))
        .collect();
    assert!(
        param_hints.iter().any(|h| hint_label(h).contains("string")),
        "expected a 'string' type hint for closure param in method call, got: {:?}",
        param_hints
            .iter()
            .map(|h| hint_label(h))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn closure_param_hint_shortened_fqn() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = r#"<?php
namespace App;
class User { public string $name = ''; }
class Service {
    /**
     * @param callable(User): void $fn
     */
    public function each(callable $fn): void {}
}

$s = new Service();
$s->each(fn ($u) => $u->name);
"#;
    let hints = inlay_hints_for(&backend, &uri, text).await;
    let param_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && !hint_label(h).starts_with(':'))
        .collect();
    assert!(
        !param_hints.is_empty(),
        "expected a param type hint, got: {:?}",
        hints.iter().map(hint_label).collect::<Vec<_>>()
    );
    // Should show the short name, not the FQN.
    assert_eq!(
        hint_label(param_hints[0]),
        "User ",
        "param type hint should use short name, not FQN"
    );
}

/// When a function has `@template T` with `callable(T): void`, the
/// closure parameter type hints should show the concrete type inferred
/// from sibling arguments.  T is inferred as `int` from `array<T>`
/// matched against `[1, 2, 3]`.
///
/// The closure's `$x` parameter should get an `int` type hint, not
/// `T` or `mixed`.
#[tokio::test]
async fn closure_param_hint_template_substitution_from_sibling_args() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = r#"<?php
/**
 * @template T
 * @param array<T> $items
 * @param callable(T): void $callback
 */
function each(array $items, callable $callback): void {}

each([1, 2, 3], function ($x) {});
"#;
    let hints = inlay_hints_for(&backend, &uri, text).await;
    let all: Vec<_> = hints.iter().map(|h| (hint_label(h), h.kind)).collect();

    let param_type_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && !hint_label(h).starts_with(':'))
        .collect();

    // Template substitution infers T = int from [1, 2, 3].
    assert_eq!(
        param_type_hints.len(),
        1,
        "expected exactly one param type hint for $x; all hints: {:?}",
        all
    );
    assert_eq!(
        hint_label(param_type_hints[0]),
        "int ",
        "template T should be substituted to int; all hints: {:?}",
        all
    );
    // Verify the callable resolves — we should get ": void" return hint.
    let return_type_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && hint_label(h).starts_with(':'))
        .collect();
    assert!(
        !return_type_hints.is_empty(),
        "expected at least a closure return-type hint from callable(T): void; all hints: {:?}",
        all
    );
}

/// When a function has `@template T` with `callable(T): T`, the
/// closure return type hint should show the concrete type inferred
/// from sibling arguments.
#[tokio::test]
async fn closure_return_type_hint_template_substitution() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let text = r#"<?php
/**
 * @template T
 * @param array<T> $items
 * @param callable(T): T $fn
 */
function transform(array $items, callable $fn): void {}

transform([1, 2, 3], function ($x) { return $x * 2; });
"#;
    let hints = inlay_hints_for(&backend, &uri, text).await;
    let all: Vec<_> = hints.iter().map(|h| (hint_label(h), h.kind)).collect();
    let return_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE) && hint_label(h).starts_with(':'))
        .collect();
    // Template substitution infers T = int from [1, 2, 3], so the
    // return type of callable(T): T becomes ": int".
    assert!(
        !return_hints.is_empty(),
        "expected a closure return-type hint; all hints: {:?}",
        all
    );
    assert_eq!(
        hint_label(return_hints[0]),
        ": int",
        "return type hint should be ': int' (T substituted from array elements); all: {:?}",
        all
    );
}

// ─── Call-result invocation: callable return type (#184) ────────────────────

#[tokio::test]
async fn callable_return_invocation_no_wrong_hints() {
    // When a function returns `callable` and the result is immediately
    // invoked, inlay hints must NOT show the outer function's parameter
    // names on the inner call's arguments.
    //
    // Before the fix, `makeCallable('1', '2')('test')` showed `$a:`
    // on `'test'` instead of suppressing the hint (bare `callable`
    // has no known parameter names).
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/callable_invoke.php").unwrap();
    let text = r#"<?php
function makeCallable(string $a, string $b): callable
{
    return fn (string $c) => "$a $b $c";
}

makeCallable('1', '2')('test');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;

    // Line 6: `makeCallable('1', '2')('test');`
    let line6_hints = hints_at_line(&hints, 6);
    let line6_labels: Vec<String> = line6_hints.iter().map(|h| hint_label(h)).collect();

    // The outer call `makeCallable('1', '2')` should show `$a:` and `$b:`.
    assert!(
        line6_labels.iter().any(|l| l == "a:"),
        "Should show 'a:' hint for makeCallable's first arg; got: {:?}",
        line6_labels
    );
    assert!(
        line6_labels.iter().any(|l| l == "b:"),
        "Should show 'b:' hint for makeCallable's second arg; got: {:?}",
        line6_labels
    );

    // The inner call `('test')` should NOT show `$a:` — it's invoking
    // a bare `callable`, not `makeCallable`.
    // Count how many `a:` hints there are — should be exactly 1 (from
    // the outer call), not 2.
    let a_count = line6_labels.iter().filter(|l| *l == "a:").count();
    assert_eq!(
        a_count, 1,
        "Should have exactly 1 'a:' hint (outer call only), got {}: {:?}",
        a_count, line6_labels
    );
}

#[tokio::test]
async fn callable_return_invocation_fully_qualified_closure() {
    // The fully-qualified `\Closure` return type spelling must be
    // recognised the same way as bare `Closure` and `callable`.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_invoke.php").unwrap();
    let text = r#"<?php
function makeClosure(string $a, string $b): \Closure
{
    return fn (string $c) => "$a $b $c";
}

makeClosure('1', '2')('test');
"#;

    let hints = inlay_hints_for(&backend, &uri, text).await;

    let line6_hints = hints_at_line(&hints, 6);
    let line6_labels: Vec<String> = line6_hints.iter().map(|h| hint_label(h)).collect();

    let a_count = line6_labels.iter().filter(|l| *l == "a:").count();
    assert_eq!(
        a_count, 1,
        "Should have exactly 1 'a:' hint (outer call only), got {}: {:?}",
        a_count, line6_labels
    );
}
