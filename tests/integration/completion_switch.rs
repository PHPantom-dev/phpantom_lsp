use crate::common::{completion_response, create_test_backend, open_php};
use tower_lsp_server::gen_lsp_types::*;

/// Variable assigned inside switch cases should resolve its type
/// from all branches (union of types across cases).
#[tokio::test]
async fn test_completion_switch_basic_new_instantiation() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///switch_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "\n",
        "function test(string $type): void {\n",
        "    switch ($type) {\n",
        "        case 'dog':\n",
        "            $animal = new Dog();\n",
        "            break;\n",
        "        case 'cat':\n",
        "            $animal = new Cat();\n",
        "            break;\n",
        "    }\n",
        "    $animal->\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$animal->` on line 18
    let result = completion_response(&backend, &uri, 18, 14, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $animal-> after switch"
    );

    match result.unwrap() {
        CompletionResponse::CompletionItemList(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("bark")),
                "Should include bark from Dog, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("purr")),
                "Should include purr from Cat, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::CompletionItemList"),
    }
}

/// Variable assigned inside a switch with a default case should include
/// the default branch's type in the union.
#[tokio::test]
async fn test_completion_switch_with_default_case() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///switch_default.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Success {\n",
        "    public function getMessage(): string {}\n",
        "}\n",
        "\n",
        "class Failure {\n",
        "    public function getError(): string {}\n",
        "}\n",
        "\n",
        "function test(int $code): void {\n",
        "    switch ($code) {\n",
        "        case 200:\n",
        "            $result = new Success();\n",
        "            break;\n",
        "        default:\n",
        "            $result = new Failure();\n",
        "            break;\n",
        "    }\n",
        "    $result->\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 18, 14, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $result-> after switch with default"
    );

    match result.unwrap() {
        CompletionResponse::CompletionItemList(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getMessage")),
                "Should include getMessage from Success, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getError")),
                "Should include getError from Failure, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::CompletionItemList"),
    }
}

/// Variable assigned in a single switch case (no other cases assign it)
/// should still resolve.
#[tokio::test]
async fn test_completion_switch_single_case_assignment() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///switch_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Renderer {\n",
        "    public function render(): string {}\n",
        "    public function clear(): void {}\n",
        "}\n",
        "\n",
        "function test(string $mode): void {\n",
        "    switch ($mode) {\n",
        "        case 'html':\n",
        "            $renderer = new Renderer();\n",
        "            break;\n",
        "    }\n",
        "    $renderer->\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 12, 16, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $renderer-> after single-case switch"
    );

    match result.unwrap() {
        CompletionResponse::CompletionItemList(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("render")),
                "Should include render from Renderer, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("clear")),
                "Should include clear from Renderer, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::CompletionItemList"),
    }
}

/// Cursor inside a switch case body should resolve variables assigned
/// earlier in the same case.
#[tokio::test]
async fn test_completion_switch_cursor_inside_case() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///switch_inside.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function log(string $msg): void {}\n",
        "    public function flush(): void {}\n",
        "}\n",
        "\n",
        "function test(string $level): void {\n",
        "    switch ($level) {\n",
        "        case 'debug':\n",
        "            $logger = new Logger();\n",
        "            $logger->\n",
        "            break;\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$logger->` on line 10
    let result = completion_response(&backend, &uri, 10, 21, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $logger-> inside switch case"
    );

    match result.unwrap() {
        CompletionResponse::CompletionItemList(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("log")),
                "Should include log from Logger, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("flush")),
                "Should include flush from Logger, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::CompletionItemList"),
    }
}

/// Variable assigned inside switch inside a class method should resolve.
#[tokio::test]
async fn test_completion_switch_in_method() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///switch_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class FileReader {\n",
        "    public function read(): string {}\n",
        "}\n",
        "\n",
        "class DbReader {\n",
        "    public function query(): string {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function getReader(string $source): void {\n",
        "        switch ($source) {\n",
        "            case 'file':\n",
        "                $reader = new FileReader();\n",
        "                break;\n",
        "            case 'db':\n",
        "                $reader = new DbReader();\n",
        "                break;\n",
        "        }\n",
        "        $reader->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$reader->` on line 19
    let result = completion_response(&backend, &uri, 19, 17, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $reader-> after switch in method"
    );

    match result.unwrap() {
        CompletionResponse::CompletionItemList(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("read")),
                "Should include read from FileReader, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("query")),
                "Should include query from DbReader, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::CompletionItemList"),
    }
}

/// Variable assigned before a switch and then overridden in cases
/// should resolve to the union of the case types (overriding the
/// original assignment since each case is conditional).
#[tokio::test]
async fn test_completion_switch_overrides_earlier_assignment() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///switch_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Alpha {\n",
        "    public function alphaMethod(): void {}\n",
        "}\n",
        "\n",
        "class Beta {\n",
        "    public function betaMethod(): void {}\n",
        "}\n",
        "\n",
        "class Gamma {\n",
        "    public function gammaMethod(): void {}\n",
        "}\n",
        "\n",
        "function test(int $n): void {\n",
        "    $obj = new Alpha();\n",
        "    switch ($n) {\n",
        "        case 1:\n",
        "            $obj = new Beta();\n",
        "            break;\n",
        "        case 2:\n",
        "            $obj = new Gamma();\n",
        "            break;\n",
        "    }\n",
        "    $obj->\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$obj->` on line 23
    let result = completion_response(&backend, &uri, 23, 11, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $obj-> after switch override"
    );

    match result.unwrap() {
        CompletionResponse::CompletionItemList(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Original Alpha assignment is still reachable (switch might not match)
            assert!(
                labels.iter().any(|l| l.starts_with("alphaMethod")),
                "Should include alphaMethod from Alpha (original), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("betaMethod")),
                "Should include betaMethod from Beta, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("gammaMethod")),
                "Should include gammaMethod from Gamma, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::CompletionItemList"),
    }
}

/// Colon-delimited switch (`switch ($x): ... endswitch;`) should
/// also resolve variable types from case bodies.
#[tokio::test]
async fn test_completion_switch_colon_delimited() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///switch_colon.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pdf {\n",
        "    public function generate(): string {}\n",
        "}\n",
        "\n",
        "class Csv {\n",
        "    public function export(): string {}\n",
        "}\n",
        "\n",
        "function test(string $format): void {\n",
        "    switch ($format):\n",
        "        case 'pdf':\n",
        "            $doc = new Pdf();\n",
        "            break;\n",
        "        case 'csv':\n",
        "            $doc = new Csv();\n",
        "            break;\n",
        "    endswitch;\n",
        "    $doc->\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$doc->` on line 18
    let result = completion_response(&backend, &uri, 18, 11, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $doc-> after colon-delimited switch"
    );

    match result.unwrap() {
        CompletionResponse::CompletionItemList(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("generate")),
                "Should include generate from Pdf, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("export")),
                "Should include export from Csv, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::CompletionItemList"),
    }
}

/// Switch with three cases plus default should resolve union of all.
#[tokio::test]
async fn test_completion_switch_three_cases_plus_default() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///switch_three.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Circle {\n",
        "    public function radius(): float {}\n",
        "}\n",
        "\n",
        "class Square {\n",
        "    public function side(): float {}\n",
        "}\n",
        "\n",
        "class Triangle {\n",
        "    public function base(): float {}\n",
        "}\n",
        "\n",
        "class Unknown {\n",
        "    public function describe(): string {}\n",
        "}\n",
        "\n",
        "function test(string $shape): void {\n",
        "    switch ($shape) {\n",
        "        case 'circle':\n",
        "            $s = new Circle();\n",
        "            break;\n",
        "        case 'square':\n",
        "            $s = new Square();\n",
        "            break;\n",
        "        case 'triangle':\n",
        "            $s = new Triangle();\n",
        "            break;\n",
        "        default:\n",
        "            $s = new Unknown();\n",
        "            break;\n",
        "    }\n",
        "    $s->\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$s->` on line 32
    let result = completion_response(&backend, &uri, 32, 10, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $s-> after switch with 3 cases + default"
    );

    match result.unwrap() {
        CompletionResponse::CompletionItemList(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("radius")),
                "Should include radius from Circle, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("side")),
                "Should include side from Square, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("base")),
                "Should include base from Triangle, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("describe")),
                "Should include describe from Unknown (default), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::CompletionItemList"),
    }
}

/// Method call return type inside a switch case should resolve.
#[tokio::test]
async fn test_completion_switch_method_call_return_type() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///switch_method_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Connection {\n",
        "    public function execute(): void {}\n",
        "    public function close(): void {}\n",
        "}\n",
        "\n",
        "class Factory {\n",
        "    public function createConnection(): Connection {}\n",
        "}\n",
        "\n",
        "function test(Factory $factory, string $driver): void {\n",
        "    switch ($driver) {\n",
        "        case 'mysql':\n",
        "            $conn = $factory->createConnection();\n",
        "            break;\n",
        "    }\n",
        "    $conn->\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$conn->` on line 16
    let result = completion_response(&backend, &uri, 16, 12, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $conn-> from method call in switch"
    );

    match result.unwrap() {
        CompletionResponse::CompletionItemList(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("execute")),
                "Should include execute from Connection, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("close")),
                "Should include close from Connection, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::CompletionItemList"),
    }
}

/// Nested switch inside an if block should still resolve variable types.
#[tokio::test]
async fn test_completion_switch_nested_in_if() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///switch_nested.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Mailer {\n",
        "    public function send(): void {}\n",
        "}\n",
        "\n",
        "class Notifier {\n",
        "    public function notify(): void {}\n",
        "}\n",
        "\n",
        "function test(bool $flag, string $channel): void {\n",
        "    if ($flag) {\n",
        "        switch ($channel) {\n",
        "            case 'email':\n",
        "                $handler = new Mailer();\n",
        "                break;\n",
        "            case 'push':\n",
        "                $handler = new Notifier();\n",
        "                break;\n",
        "        }\n",
        "        $handler->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$handler->` on line 19
    let result = completion_response(&backend, &uri, 19, 18, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $handler-> after switch nested in if"
    );

    match result.unwrap() {
        CompletionResponse::CompletionItemList(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("send")),
                "Should include send from Mailer, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("notify")),
                "Should include notify from Notifier, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::CompletionItemList"),
    }
}
