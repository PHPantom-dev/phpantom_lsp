use crate::common::{
    create_psr4_workspace, create_psr4_workspace_with_exception_stubs, create_test_backend,
    create_test_backend_with_exception_stubs,
};

use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Test parenthesized RHS expression in variable assignment completion
#[tokio::test]
async fn test_completion_parenthesized_rhs_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///paren_rhs.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ParenDemo {\n",
        "    public function foo(): void {}\n",
        "    public function test() {\n",
        "        $p = (new ParenDemo());\n",
        "        $p->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $p = (new ParenDemo())"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"foo"),
                "Should include 'foo' method from ParenDemo"
            );
            assert!(
                method_names.contains(&"test"),
                "Should include 'test' method from ParenDemo"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test `$var::` completion where variable holds a class-string from `Foo::class`
#[tokio::test]
async fn test_completion_class_string_variable_static_access() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///classstring.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Gadget {\n",
        "    public static function build(): void {}\n",
        "    public const VERSION = '1.0';\n",
        "    public static string $label = 'gadget';\n",
        "    public function test() {\n",
        "        $cls = Gadget::class;\n",
        "        $cls::\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $cls = Gadget::class; $cls::"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"build"),
                "Should include static method 'build', got: {:?}",
                names
            );
            assert!(
                names.contains(&"VERSION"),
                "Should include constant 'VERSION', got: {:?}",
                names
            );
            assert!(
                names.contains(&"$label"),
                "Should include static property '$label', got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test `$var::` with self::class inside a class method
#[tokio::test]
async fn test_completion_class_string_self_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///selfclass.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public static function create(): void {}\n",
        "    public const NAME = 'widget';\n",
        "    public function test() {\n",
        "        $ref = self::class;\n",
        "        $ref::\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $ref = self::class; $ref::"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"create"),
                "Should include static method 'create', got: {:?}",
                names
            );
            assert!(
                names.contains(&"NAME"),
                "Should include constant 'NAME', got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test `$var::` with ternary assignment of class-strings
#[tokio::test]
async fn test_completion_class_string_ternary() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///classstring_ternary.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Alpha {\n",
        "    public static function alphaMethod(): void {}\n",
        "}\n",
        "class Beta {\n",
        "    public static function betaMethod(): void {}\n",
        "    public function test() {\n",
        "        $cls = rand(0, 1) ? Alpha::class : Beta::class;\n",
        "        $cls::\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for ternary class-string"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"alphaMethod"),
                "Should include Alpha's static method, got: {:?}",
                names
            );
            assert!(
                names.contains(&"betaMethod"),
                "Should include Beta's static method, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test `$var::` at top level (outside any class)
#[tokio::test]
async fn test_completion_class_string_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///classstring_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Svc {\n",
        "    public static function start(): void {}\n",
        "    public const MAX = 10;\n",
        "}\n",
        "$svc = Svc::class;\n",
        "$svc::\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for top-level $svc = Svc::class; $svc::"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"start"),
                "Should include static method 'start', got: {:?}",
                names
            );
            assert!(
                names.contains(&"MAX"),
                "Should include constant 'MAX', got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_new_self_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///newself.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public function build(): void {}\n",
        "    public static function create(): self {\n",
        "        $new = new self();\n",
        "        $new->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$new->` on line 5
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $new = new self"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"build"),
                "Should include non-static 'build'"
            );
            assert!(
                !method_names.contains(&"create"),
                "Should exclude static 'create' via ->"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_new_static_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///newstatic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public function build(): void {}\n",
        "    public static function create(): static {\n",
        "        $inst = new static();\n",
        "        $inst->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $inst = new static"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"build"),
                "Should include non-static 'build'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_new_classname_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///newclass.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): void {}\n",
        "    public function test() {\n",
        "        $w = new Widget();\n",
        "        $w->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $w = new Widget"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(method_names.contains(&"render"), "Should include 'render'");
            assert!(method_names.contains(&"test"), "Should include 'test'");
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_unknown_variable_shows_fallback() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unknown.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Svc {\n",
        "    public function run(): void {}\n",
        "    public function test() {\n",
        "        $unknown->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 4,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // Unknown variable with no matching completion should return None
    assert!(result.is_none(), "Unknown variable should return None");
}

#[tokio::test]
async fn test_completion_property_chain_self_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Node {\n",
        "    public self $parent;\n",
        "    public function value(): int {}\n",
        "    public function test() {\n",
        "        $this->parent->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$this->parent->` on line 5
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 23,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve $this->parent-> via self type hint"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(method_names.contains(&"value"), "Should include 'value'");
            assert!(method_names.contains(&"test"), "Should include 'test'");
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_classname_double_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///classdcolon.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Registry {\n",
        "    public static function instance(): self {}\n",
        "    public function get(): void {}\n",
        "    const VERSION = 1;\n",
        "    function test() {\n",
        "        Registry::\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `Registry::` on line 6
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve Registry:: to Registry class"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.label.as_str())
                .collect();

            // Only static method should appear for ::
            assert!(
                method_names.contains(&"instance"),
                "Should include static 'instance'"
            );
            assert!(
                !method_names.contains(&"get"),
                "Should exclude non-static 'get'"
            );
            assert!(
                constant_names.contains(&"VERSION"),
                "Should include constant"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_param_type_hint_resolves() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///paramhint.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Processor {\n",
        "    public function run(): void {}\n",
        "    public function handle(self $other) {\n",
        "        $other->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$other->` on line 4
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 4,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve $other via parameter type hint"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(method_names.contains(&"run"), "Should include 'run'");
            assert!(method_names.contains(&"handle"), "Should include 'handle'");
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_static_double_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///staticdc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public static function create(): static {}\n",
        "    public function run(): void {}\n",
        "    const MAX = 10;\n",
        "    function test() {\n",
        "        static::\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `static::` on line 6
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(result.is_some(), "Completion should resolve static::");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.label.as_str())
                .collect();
            // Only static method for ::
            assert!(
                method_names.contains(&"create"),
                "Should include static 'create'"
            );
            assert!(
                method_names.contains(&"run"),
                "static:: should include non-static 'run'"
            );
            assert!(
                constant_names.contains(&"MAX"),
                "Should include constant 'MAX'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_parameter_uses_param_docblock_override() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///param_docblock_override.php").unwrap();
    let text = r#"<?php
class Node {}
class FuncCall extends Node {
    public function isFirstClassCallable(): bool {}
    public function getName(): string {}
}
class Handler {
    /**
     * @param FuncCall $node
     */
    public function handle(Node $node): void {
        $node->
    }
}
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

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve $node via @param override"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .filter_map(|i| i.filter_text.as_deref())
                .collect();
            assert!(
                method_names.contains(&"isFirstClassCallable"),
                "Should include 'isFirstClassCallable', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getName"),
                "Should include 'getName', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Completion: new ClassName()->  and  (new ClassName())-> ─────────────────

#[tokio::test]
async fn test_completion_new_classname_arrow() {
    let text = concat!(
        "<?php\n",
        "class SessionManager {\n",
        "    public function callCustomCreator(): void {}\n",
        "    public function boot(): void {}\n",
        "    public function run(): void {\n",
        "        new SessionManager()->\n",
        "    }\n",
        "}\n",
    );

    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 30,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for new SessionManager()->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("callCustomCreator")),
                "Should include callCustomCreator, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("boot")),
                "Should include boot, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_parenthesized_new_classname_arrow() {
    let text = concat!(
        "<?php\n",
        "class SessionManager {\n",
        "    public function callCustomCreator(): void {}\n",
        "    public function boot(): void {}\n",
        "    public function run(): void {\n",
        "        (new SessionManager())->\n",
        "    }\n",
        "}\n",
    );

    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for (new SessionManager())->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("callCustomCreator")),
                "Should include callCustomCreator, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("boot")),
                "Should include boot, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_new_classname_arrow_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/SessionManager.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "\n",
                "class SessionManager {\n",
                "    public function callCustomCreator(): void {}\n",
                "    public function boot(): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\SessionManager;\n",
        "\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        (new SessionManager())->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for (new SessionManager())-> cross-file"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("callCustomCreator")),
                "Should include callCustomCreator, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("boot")),
                "Should include boot, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Ambiguous Variable Completion Tests ────────────────────────────────────

/// When a variable is conditionally reassigned (if-block), completion should
/// offer the union of members from all candidate types.
#[tokio::test]
async fn test_completion_ambiguous_variable_if_block_union() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///ambiguous.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class SessionManager {\n",
        "    public function callCustomCreator2(): void {}\n",
        "    public function start(): void {}\n",
        "}\n",
        "\n",
        "class Manager {\n",
        "    public function doWork(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $thing = new SessionManager();\n",
        "        if ($thing->callCustomCreator2()) {\n",
        "            $thing = new Manager();\n",
        "        }\n",
        "        $thing->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$thing->` on line 16
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 16,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for ambiguous $thing->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include members from SessionManager
            assert!(
                labels.iter().any(|l| l.starts_with("callCustomCreator2")),
                "Should include callCustomCreator2 from SessionManager, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("start")),
                "Should include start from SessionManager, got: {:?}",
                labels
            );
            // Should also include members from Manager
            assert!(
                labels.iter().any(|l| l.starts_with("doWork")),
                "Should include doWork from Manager, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Unconditional reassignment: only the final type's members should appear.
#[tokio::test]
async fn test_completion_unconditional_reassignment_only_final_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unconditional.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooOnly(): void {}\n",
        "}\n",
        "\n",
        "class Bar {\n",
        "    public function barOnly(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $x = new Foo();\n",
        "        $x = new Bar();\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$x->` on line 13
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $x->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include Bar's method (the final unconditional assignment)
            assert!(
                labels.iter().any(|l| l.starts_with("barOnly")),
                "Should include barOnly from Bar, got: {:?}",
                labels
            );
            // Should NOT include Foo's method (unconditionally replaced)
            assert!(
                !labels.iter().any(|l| l.starts_with("fooOnly")),
                "Should NOT include fooOnly from Foo after unconditional reassignment, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Ambiguous variable with if/else: completion shows union of all branches
/// plus the original type.
#[tokio::test]
async fn test_completion_ambiguous_variable_if_else_union() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///ifelse.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Writer {\n",
        "    public function write(): void {}\n",
        "}\n",
        "\n",
        "class Printer {\n",
        "    public function print(): void {}\n",
        "}\n",
        "\n",
        "class Sender {\n",
        "    public function send(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $out = new Writer();\n",
        "        if (true) {\n",
        "            $out = new Printer();\n",
        "        } else {\n",
        "            $out = new Sender();\n",
        "        }\n",
        "        $out->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$out->` on line 21
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 21,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for ambiguous $out->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // The forward walker correctly excludes Writer (dead branch before
            // the if/else), so only Printer and Sender are candidates.
            assert!(
                !labels.iter().any(|l| l.starts_with("write")),
                "Should NOT include write from Writer (dead branch), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("print")),
                "Should include print from Printer, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("send")),
                "Should include send from Sender, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Union Type Completion Tests ────────────────────────────────────────────

/// When a method returns a union type (`B|C`), completion should offer
/// the union of members from all parts of the type.
#[tokio::test]
async fn test_completion_union_return_type_shows_all_members() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///union_completion.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "    public function fetch(): void {}\n",
        "}\n",
        "\n",
        "class Cat {\n",
        "    public function meow(): void {}\n",
        "    public function purr(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function getAnimal(): Dog|Cat { return new Dog(); }\n",
        "\n",
        "    public function run(): void {\n",
        "        $pet = $this->getAnimal();\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$pet->` on line 16
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 16,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for union return type Dog|Cat"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include members from Dog
            assert!(
                labels.iter().any(|l| l.starts_with("bark")),
                "Should include bark from Dog, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("fetch")),
                "Should include fetch from Dog, got: {:?}",
                labels
            );
            // Should also include members from Cat
            assert!(
                labels.iter().any(|l| l.starts_with("meow")),
                "Should include meow from Cat, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("purr")),
                "Should include purr from Cat, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Union type on a parameter: completion shows members from all parts.
#[tokio::test]
async fn test_completion_union_parameter_type_shows_all_members() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///union_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Reader {\n",
        "    public function read(): void {}\n",
        "}\n",
        "\n",
        "class Stream {\n",
        "    public function consume(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function process(Reader|Stream $input): void {\n",
        "        $input->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$input->` on line 11
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for union param type Reader|Stream"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("read")),
                "Should include read from Reader, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("consume")),
                "Should include consume from Stream, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Union Return + Conditional Reassignment ────────────────────────────────

/// When a variable is assigned from a function returning a union type (A|B)
/// and then conditionally reassigned to a new type (C), the resulting type
/// should be A|B|C — the union should grow, not be special-cased.
#[tokio::test]
async fn test_completion_union_return_plus_conditional_reassignment_grows_union() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///union_grow.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function onlyOnA(): void {}\n",
        "}\n",
        "\n",
        "class B {\n",
        "    public function onlyOnB(): void {}\n",
        "}\n",
        "\n",
        "class C {\n",
        "    public function onlyOnC(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    /** @return A|B */\n",
        "    public function makeAOrB(): A|B { return new A(); }\n",
        "\n",
        "    public function run(): void {\n",
        "        $thing = $this->makeAOrB();\n",
        "        if (true) {\n",
        "            $thing = new C();\n",
        "        }\n",
        "        $thing->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$thing->` on line 22
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 22,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $thing-> after union return + conditional reassignment"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include members from A (from makeAOrB union part)
            assert!(
                labels.iter().any(|l| l.starts_with("onlyOnA")),
                "Should include onlyOnA from A (union return), got: {:?}",
                labels
            );
            // Should include members from B (from makeAOrB union part)
            assert!(
                labels.iter().any(|l| l.starts_with("onlyOnB")),
                "Should include onlyOnB from B (union return), got: {:?}",
                labels
            );
            // Should include members from C (conditional reassignment)
            assert!(
                labels.iter().any(|l| l.starts_with("onlyOnC")),
                "Should include onlyOnC from C (conditional reassignment), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── PHPStan Conditional Return Type Tests ──────────────────────────────────

/// When a function has a PHPStan conditional return type like
/// `@return ($abstract is class-string<TClass> ? TClass : mixed)`
/// and is called with `A::class`, completion should resolve to class A.
#[tokio::test]
async fn test_completion_conditional_return_class_string() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///conditional.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function onlyOnA(): void {}\n",
        "}\n",
        "\n",
        "class B {\n",
        "    public function onlyOnB(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template TClass\n",
        " * @return ($abstract is class-string<TClass> ? TClass : ($abstract is null ? \\App : mixed))\n",
        " */\n",
        "function app($abstract = null, array $parameters = []) {}\n",
        "\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        $obj = app(A::class);\n",
        "        $obj->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$obj->` on line 18
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 18,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $obj-> after app(A::class)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include members from A (resolved via class-string<T>)
            assert!(
                labels.iter().any(|l| l.starts_with("onlyOnA")),
                "Should include onlyOnA from A (resolved via class-string conditional), got: {:?}",
                labels
            );
            // Should NOT include members from B
            assert!(
                !labels.iter().any(|l| l.starts_with("onlyOnB")),
                "Should NOT include onlyOnB from B, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a function has a PHPStan conditional return type and is called
/// without arguments, it should resolve to the null-default branch.
/// e.g. `app()` → Application
#[tokio::test]
async fn test_completion_conditional_return_null_default() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///conditional_null.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Application {\n",
        "    public function version(): string {}\n",
        "    public function boot(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @return ($abstract is class-string<TClass> ? TClass : ($abstract is null ? Application : mixed))\n",
        " */\n",
        "function app($abstract = null, array $parameters = []) {}\n",
        "\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        $a = app();\n",
        "        $a->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$a->` on line 14
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 14,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $a-> after app()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("version")),
                "Should include version from Application (null-default branch), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("boot")),
                "Should include boot from Application (null-default branch), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a function has `@return ($guard is null ? Factory : StatefulGuard)`
/// and is called with a non-null argument like `auth('web')`, completion
/// should resolve to the else branch (StatefulGuard).
#[tokio::test]
async fn test_completion_conditional_return_non_null_argument() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///conditional_auth.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public function guard(): void {}\n",
        "}\n",
        "\n",
        "class StatefulGuard {\n",
        "    public function login(): void {}\n",
        "    public function logout(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @return ($guard is null ? Factory : StatefulGuard)\n",
        " */\n",
        "function auth($guard = null) {}\n",
        "\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        $g = auth('web');\n",
        "        $g->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$g->` on line 18
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 18,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $g-> after auth('web')"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include members from StatefulGuard (non-null arg → else branch)
            assert!(
                labels.iter().any(|l| l.starts_with("login")),
                "Should include login from StatefulGuard, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("logout")),
                "Should include logout from StatefulGuard, got: {:?}",
                labels
            );
            // Should NOT include members from Factory
            assert!(
                !labels.iter().any(|l| l.starts_with("guard")),
                "Should NOT include guard from Factory, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `app(A::class)->` is used inline (without assigning to a variable),
/// completion should resolve the conditional return type using the text
/// arguments and offer members of `A`.
#[tokio::test]
async fn test_completion_inline_conditional_return_class_string() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inline_conditional.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class SessionManager {\n",
        "    public function callCustomCreator2(): void {}\n",
        "    public function driver(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template TClass\n",
        " * @return ($abstract is class-string<TClass> ? TClass : ($abstract is null ? \\App : mixed))\n",
        " */\n",
        "function app($abstract = null, array $parameters = []) {}\n",
        "\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        app(SessionManager::class)->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `app(SessionManager::class)->` on line 13
    // 8 spaces + "app(SessionManager::class)->" = 8 + 28 = 36
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 14,
                character: 36,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for app(SessionManager::class)->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include members from SessionManager
            assert!(
                labels.iter().any(|l| l.starts_with("callCustomCreator2")),
                "Should include callCustomCreator2 from SessionManager, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("driver")),
                "Should include driver from SessionManager, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `auth('web')->` is used inline (without assigning to a variable),
/// the non-null argument should resolve to the else branch of an `is null`
/// conditional return type.
#[tokio::test]
async fn test_completion_inline_conditional_return_non_null_argument() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inline_auth.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public function guard(): void {}\n",
        "}\n",
        "\n",
        "class StatefulGuard {\n",
        "    public function login(): void {}\n",
        "    public function logout(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @return ($guard is null ? Factory : StatefulGuard)\n",
        " */\n",
        "function auth($guard = null) {}\n",
        "\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        auth('web')->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `auth('web')->` on line 17
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 17,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for auth('web')->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include members from StatefulGuard (non-null arg → else branch)
            assert!(
                labels.iter().any(|l| l.starts_with("login")),
                "Should include login from StatefulGuard, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("logout")),
                "Should include logout from StatefulGuard, got: {:?}",
                labels
            );
            // Should NOT include members from Factory
            assert!(
                !labels.iter().any(|l| l.starts_with("guard")),
                "Should NOT include guard from Factory, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `app()->` is used inline with no arguments, the null-default
/// branch should be taken, just as when assigned to a variable.
#[tokio::test]
async fn test_completion_inline_conditional_return_null_default() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inline_null.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Application {\n",
        "    public function version(): string {}\n",
        "    public function boot(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @return ($abstract is class-string<TClass> ? TClass : ($abstract is null ? Application : mixed))\n",
        " */\n",
        "function app($abstract = null, array $parameters = []) {}\n",
        "\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        app()->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `app()->` on line 13
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for app()->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("version")),
                "Should include version from Application (null-default branch), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("boot")),
                "Should include boot from Application (null-default branch), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a **method** has a PHPStan conditional return type (e.g.
/// `Application::make`), chaining through it should resolve the type
/// correctly.  For example:
///
/// ```php
/// app()->make(CurrentCart::class)->save();
/// ```
///
/// `app()` returns `Application`, `make(CurrentCart::class)` should
/// resolve via the conditional `@return` to `CurrentCart`, and then
/// `->save()` (or `->` completion) should offer `CurrentCart` members.
#[tokio::test]
async fn test_completion_method_conditional_return_class_string_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_conditional.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class CurrentCart {\n",
        "    public function save(): void {}\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "\n",
        "class Application {\n",
        "    /**\n",
        "     * @template TClass of object\n",
        "     * @param  string|class-string<TClass>  $abstract\n",
        "     * @return ($abstract is class-string<TClass> ? TClass : mixed)\n",
        "     */\n",
        "    public function make($abstract, array $parameters = []) {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @return Application\n",
        " */\n",
        "function app() {}\n",
        "\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        app()->make(CurrentCart::class)->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `app()->make(CurrentCart::class)->` on line 22
    // 8 spaces + "app()->make(CurrentCart::class)->" = 8 + 33 = 41
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 22,
                character: 41,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for app()->make(CurrentCart::class)->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include members from CurrentCart (resolved via method conditional return)
            assert!(
                labels.iter().any(|l| l.starts_with("save")),
                "Should include save from CurrentCart (resolved via method class-string conditional), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getTotal")),
                "Should include getTotal from CurrentCart, got: {:?}",
                labels
            );
            // Should NOT include members from Application
            assert!(
                !labels.iter().any(|l| l.starts_with("make")),
                "Should NOT include make from Application, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ── Symfony-style conditional return with @psalm-return coexisting ───────────

/// Reproduces https://github.com/AJenbo/phpantom_lsp/issues/41
///
/// When a method has both `@phpstan-return` and `@psalm-return` conditional
/// annotations and the class-string parameter is not at index 0, the resolved
/// type should be the concrete class only (not a union with `mixed`).
///
/// ```php
/// /**
///  * @template TObject of object
///  * @template TType of string|class-string<TObject>
///  * @param mixed  $data
///  * @param TType  $type
///  * @param string $format
///  * @param array<string, mixed> $context
///  * @phpstan-return ($type is class-string<TObject> ? TObject : mixed)
///  * @psalm-return (TType is class-string<TObject> ? TObject : mixed)
///  */
/// public function deserialize(mixed $data, string $type, string $format, array $context = []): mixed;
/// ```
#[tokio::test]
async fn test_completion_conditional_return_class_string_at_param_index_1() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///symfony_deserialize.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                     // 0
        "class SubmissionProducerMessage {\n",                                         // 1
        "    public function getId(): int {}\n",                                       // 2
        "    public function getPayload(): string {}\n",                               // 3
        "}\n",                                                                         // 4
        "\n",                                                                          // 5
        "interface SerializerInterface {\n",                                           // 6
        "    /**\n",                                                                   // 7
        "     * @template TObject of object\n",                                        // 8
        "     * @template TType of string|class-string<TObject>\n",                    // 9
        "     *\n",                                                                    // 10
        "     * @param TType                $type\n",                                  // 11
        "     * @param array<string, mixed> $context\n",                               // 12
        "     *\n",                                                                    // 13
        "     * @phpstan-return ($type is class-string<TObject> ? TObject : mixed)\n", // 14
        "     * @psalm-return (TType is class-string<TObject> ? TObject : mixed)\n",   // 15
        "     */\n",                                                                   // 16
        "    public function deserialize(mixed $data, string $type, string $format, array $context = []): mixed;\n", // 17
        "}\n",                                                  // 18
        "\n",                                                   // 19
        "class Consumer {\n",                                   // 20
        "    private SerializerInterface $serializer;\n",       // 21
        "\n",                                                   // 22
        "    public function handle(string $buffer): void {\n", // 23
        "        $message = $this->serializer->deserialize($buffer, SubmissionProducerMessage::class, 'json');\n", // 24
        "        $message->\n", // 25
        "    }\n",              // 26
        "}\n",                  // 27
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$message->` on line 25
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 25,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $message-> after deserialize()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include members from SubmissionProducerMessage
            assert!(
                labels.iter().any(|l| l.starts_with("getId")),
                "Should include getId from SubmissionProducerMessage, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getPayload")),
                "Should include getPayload from SubmissionProducerMessage, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a method only has `@psalm-return` (no `@phpstan-return`), the
/// conditional should still resolve correctly.
#[tokio::test]
async fn test_completion_psalm_return_conditional_class_string() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///psalm_return.php").unwrap();
    let text = concat!(
        "<?php\n",                                                          // 0
        "class Order {\n",                                                  // 1
        "    public function getTotal(): float {}\n",                       // 2
        "}\n",                                                              // 3
        "\n",                                                               // 4
        "class Mapper {\n",                                                 // 5
        "    /**\n",                                                        // 6
        "     * @template T of object\n",                                   // 7
        "     * @param class-string<T> $class\n",                           // 8
        "     * @psalm-return ($class is class-string<T> ? T : mixed)\n",   // 9
        "     */\n",                                                        // 10
        "    public function map(string $data, string $class): mixed {}\n", // 11
        "}\n",                                                              // 12
        "\n",                                                               // 13
        "$m = new Mapper();\n",                                             // 14
        "$order = $m->map('{}', Order::class);\n",                          // 15
        "$order->\n",                                                       // 16
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 16,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $order-> after psalm-return conditional"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getTotal")),
                "Should include getTotal from Order (resolved via @psalm-return conditional), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ── Inline @var docblock override tests ─────────────────────────────────────

#[tokio::test]
async fn test_completion_inline_var_docblock_simple() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inlinevar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Session {\n",
        "    public function getId(): string {}\n",
        "    public function flash(): void {}\n",
        "}\n",
        "class Controller {\n",
        "    public function handle() {\n",
        "        /** @var Session */\n",
        "        $sess = mystery();\n",
        "        $sess->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for @var Session"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getId"),
                "Should include getId from Session, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"flash"),
                "Should include flash from Session, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_inline_var_docblock_with_variable_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inlinevar_named.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(): void {}\n",
        "    public function error(): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run() {\n",
        "        /** @var Logger $log */\n",
        "        $log = getLogger();\n",
        "        $log->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for @var Logger $log"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"info"),
                "Should include info from Logger, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"error"),
                "Should include error from Logger, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_inline_var_docblock_wrong_variable_name_ignored() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inlinevar_wrong.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run() {\n",
        "        /** @var Logger $other */\n",
        "        $log = something();\n",
        "        $log->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // The @var annotation names $other, not $log — so it should NOT apply.
    // Result may be None (no completions) or an array without Logger methods.
    if let Some(resp) = result {
        let items = match resp {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let method_names: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|i| i.filter_text.as_deref().unwrap())
            .collect();
        assert!(
            !method_names.contains(&"info"),
            "Should NOT include Logger::info when @var names a different variable, got: {:?}",
            method_names
        );
    }
}

#[tokio::test]
async fn test_completion_inline_var_docblock_override_blocked_by_scalar() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inlinevar_scalar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Session {\n",
        "    public function getId(): string {}\n",
        "}\n",
        "class App {\n",
        "    public function getName(): string {}\n",
        "    public function run() {\n",
        "        /** @var Session */\n",
        "        $s = $this->getName();\n",
        "        $s->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // getName() returns `string` — the @var Session override should be
    // blocked because string is a scalar.  Result may be None or an
    // array without Session methods.
    if let Some(resp) = result {
        let items = match resp {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let method_names: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|i| i.filter_text.as_deref().unwrap())
            .collect();
        assert!(
            !method_names.contains(&"getId"),
            "Should NOT include Session::getId when native type is scalar string, got: {:?}",
            method_names
        );
    }
}

#[tokio::test]
async fn test_completion_inline_var_docblock_override_allowed_for_object() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inlinevar_obj.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class BaseService {\n",
        "    public function base(): void {}\n",
        "}\n",
        "class Session extends BaseService {\n",
        "    public function getId(): string {}\n",
        "    public function flash(): void {}\n",
        "}\n",
        "class App {\n",
        "    public function getService(): BaseService {}\n",
        "    public function run() {\n",
        "        /** @var Session */\n",
        "        $s = $this->getService();\n",
        "        $s->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results when @var overrides a class type"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getId"),
                "Should include getId from Session (override allowed), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"flash"),
                "Should include flash from Session (override allowed), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Inline `@var` without variable name at top level resolves the type
/// for the immediately following assignment.
#[tokio::test]
async fn test_completion_inline_var_docblock_no_varname_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inlinevar_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var User */\n",
        "$red = a();\n",
        "$red->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for @var User without variable name at top level"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                names.contains(&"name"),
                "Should include 'name' from User, got: {:?}",
                names
            );
            assert!(
                names.contains(&"getEmail"),
                "Should include 'getEmail' from User, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Inline `@var` without variable name for a cross-file class at top level.
#[tokio::test]
async fn test_completion_inline_var_docblock_no_varname_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Models/User.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "class User {\n",
                "    public string $name;\n",
                "    public function getEmail(): string {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///crossfile_inlinevar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Models\\User;\n",
        "/** @var User */\n",
        "$red = a();\n",
        "$red->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 4,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for cross-file @var User without variable name"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                names.contains(&"name"),
                "Should include 'name' from User, got: {:?}",
                names
            );
            assert!(
                names.contains(&"getEmail"),
                "Should include 'getEmail' from User, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_inline_var_docblock_unconditional_reassignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inlinevar_reassign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class First {\n",
        "    public function one(): void {}\n",
        "}\n",
        "class Second {\n",
        "    public function two(): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run() {\n",
        "        $x = new First();\n",
        "        /** @var Second */\n",
        "        $x = transform();\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for reassigned @var"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"two"),
                "Should include two from Second (latest assignment), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"one"),
                "Should NOT include one from First (overwritten), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── instanceof narrowing ───────────────────────────────────────────────────

/// When the cursor is inside `if ($var instanceof Foo) { … }`, only
/// members of `Foo` should be suggested — not the full union.
#[tokio::test]
async fn test_completion_instanceof_narrows_to_single_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_basic.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class Animal {\n",                         // 1
        "    public function breathe(): void {}\n", // 2
        "}\n",                                      // 3
        "class Dog extends Animal {\n",             // 4
        "    public function bark(): void {}\n",    // 5
        "}\n",                                      // 6
        "class Cat extends Animal {\n",             // 7
        "    public function purr(): void {}\n",    // 8
        "}\n",                                      // 9
        "class Svc {\n",                            // 10
        "    public function test(): void {\n",     // 11
        "        if (rand(0,1)) {\n",               // 12
        "            $pet = new Dog();\n",          // 13
        "        } else {\n",                       // 14
        "            $pet = new Cat();\n",          // 15
        "        }\n",                              // 16
        "        if ($pet instanceof Dog) {\n",     // 17
        "            $pet->\n",                     // 18
        "        }\n",                              // 19
        "    }\n",                                  // 20
        "}\n",                                      // 21
    );

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
                position: Position {
                    line: 18,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's own method 'bark', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"breathe"),
                "Should include inherited method 'breathe', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's method 'purr', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// After the instanceof block, the full union should be restored.
#[tokio::test]
async fn test_completion_instanceof_no_narrowing_outside_block() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_outside.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "class Alpha {\n",                              // 1
        "    public function alphaMethod(): void {}\n", // 2
        "}\n",                                          // 3
        "class Beta {\n",                               // 4
        "    public function betaMethod(): void {}\n",  // 5
        "}\n",                                          // 6
        "class Svc {\n",                                // 7
        "    public function test(): void {\n",         // 8
        "        if (rand(0,1)) {\n",                   // 9
        "            $obj = new Alpha();\n",            // 10
        "        } else {\n",                           // 11
        "            $obj = new Beta();\n",             // 12
        "        }\n",                                  // 13
        "        if ($obj instanceof Alpha) {\n",       // 14
        "            // narrowed here\n",               // 15
        "        }\n",                                  // 16
        "        $obj->\n",                             // 17
        "    }\n",                                      // 18
        "}\n",                                          // 19
    );

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
                position: Position {
                    line: 17,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Outside the instanceof block, both types should be available
            assert!(
                method_names.contains(&"alphaMethod"),
                "Should still include 'alphaMethod' after instanceof block, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"betaMethod"),
                "Should still include 'betaMethod' after instanceof block, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `elseif ($var instanceof OtherClass)` should narrow to OtherClass.
#[tokio::test]
async fn test_completion_instanceof_elseif_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_elseif.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // 0
        "class Foo {\n",                              // 1
        "    public function fooMethod(): void {}\n", // 2
        "}\n",                                        // 3
        "class Bar {\n",                              // 4
        "    public function barMethod(): void {}\n", // 5
        "}\n",                                        // 6
        "class Baz {\n",                              // 7
        "    public function bazMethod(): void {}\n", // 8
        "}\n",                                        // 9
        "class Svc {\n",                              // 10
        "    public function test(): void {\n",       // 11
        "        if (rand(0,1)) {\n",                 // 12
        "            $x = new Foo();\n",              // 13
        "        } elseif (rand(0,1)) {\n",           // 14
        "            $x = new Bar();\n",              // 15
        "        } else {\n",                         // 16
        "            $x = new Baz();\n",              // 17
        "        }\n",                                // 18
        "        if ($x instanceof Foo) {\n",         // 19
        "            // cursor NOT here\n",           // 20
        "        } elseif ($x instanceof Bar) {\n",   // 21
        "            $x->\n",                         // 22
        "        }\n",                                // 23
        "    }\n",                                    // 24
        "}\n",                                        // 25
    );

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
                position: Position {
                    line: 22,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"barMethod"),
                "elseif instanceof Bar should narrow to Bar, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"fooMethod"),
                "Should NOT include Foo methods inside Bar elseif, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"bazMethod"),
                "Should NOT include Baz methods inside Bar elseif, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// instanceof with a different variable name should NOT narrow our variable.
#[tokio::test]
async fn test_completion_instanceof_different_variable_no_effect() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_other_var.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class TypeA {\n",                          // 1
        "    public function aMethod(): void {}\n", // 2
        "}\n",                                      // 3
        "class TypeB {\n",                          // 4
        "    public function bMethod(): void {}\n", // 5
        "}\n",                                      // 6
        "class Svc {\n",                            // 7
        "    public function test(): void {\n",     // 8
        "        if (rand(0,1)) {\n",               // 9
        "            $obj = new TypeA();\n",        // 10
        "        } else {\n",                       // 11
        "            $obj = new TypeB();\n",        // 12
        "        }\n",                              // 13
        "        $other = new TypeA();\n",          // 14
        "        if ($other instanceof TypeA) {\n", // 15
        "            $obj->\n",                     // 16
        "        }\n",                              // 17
        "    }\n",                                  // 18
        "}\n",                                      // 19
    );

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
                position: Position {
                    line: 16,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // $obj should NOT be narrowed because the instanceof checks $other
            assert!(
                method_names.contains(&"aMethod"),
                "Should include aMethod (union not narrowed), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"bMethod"),
                "Should include bMethod (union not narrowed), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// instanceof narrowing in top-level code (outside any class).
#[tokio::test]
async fn test_completion_instanceof_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Container {\n",                              // 1
        "    public function bind(): void {}\n",            // 2
        "}\n",                                              // 3
        "class AdminUser {\n",                              // 4
        "    public function grantPermission(): void {}\n", // 5
        "}\n",                                              // 6
        "\n",                                               // 7
        "if (rand(0, 1)) {\n",                              // 8
        "    $ambiguous = new Container();\n",              // 9
        "} else {\n",                                       // 10
        "    $ambiguous = new AdminUser();\n",              // 11
        "}\n",                                              // 12
        "if ($ambiguous instanceof AdminUser) {\n",         // 13
        "    $ambiguous->\n",                               // 14
        "}\n",                                              // 15
    );

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
                position: Position {
                    line: 14,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"grantPermission"),
                "Should include AdminUser's 'grantPermission', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"bind"),
                "Should NOT include Container's 'bind', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// instanceof narrowing should resolve the class via cross-file PSR-4.
#[tokio::test]
async fn test_completion_instanceof_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Vehicle.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Vehicle {\n",
                    "    public function drive(): void {}\n",
                    "}\n",
                ),
            ),
            (
                "src/Truck.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Truck extends Vehicle {\n",
                    "    public function haul(): void {}\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///main.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Vehicle;\n",
        "use App\\Truck;\n",
        "class Dispatch {\n",
        "    public function run(Vehicle $v): void {\n",
        "        if ($v instanceof Truck) {\n",
        "            $v->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 6,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"haul"),
                "Should include Truck's own method 'haul', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"drive"),
                "Should include inherited method 'drive', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Parenthesised instanceof condition should also narrow.
#[tokio::test]
async fn test_completion_instanceof_parenthesised_condition() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_parens.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // 0
        "class One {\n",                              // 1
        "    public function oneMethod(): void {}\n", // 2
        "}\n",                                        // 3
        "class Two {\n",                              // 4
        "    public function twoMethod(): void {}\n", // 5
        "}\n",                                        // 6
        "class Svc {\n",                              // 7
        "    public function test(): void {\n",       // 8
        "        if (rand(0,1)) {\n",                 // 9
        "            $val = new One();\n",            // 10
        "        } else {\n",                         // 11
        "            $val = new Two();\n",            // 12
        "        }\n",                                // 13
        "        if (($val instanceof Two)) {\n",     // 14
        "            $val->\n",                       // 15
        "        }\n",                                // 16
        "    }\n",                                    // 17
        "}\n",                                        // 18
    );

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
                position: Position {
                    line: 15,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"twoMethod"),
                "Parenthesised instanceof should narrow to Two, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"oneMethod"),
                "Should NOT include One's methods, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_top_level_chained_method_on_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///toplevel_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "    public function getProfile(): UserProfile {\n",
        "        return new UserProfile();\n",
        "    }\n",
        "}\n",
        "\n",
        "class UserProfile {\n",
        "    public string $bio = '';\n",
        "    public function getUser(): User {\n",
        "        return new User();\n",
        "    }\n",
        "    public function getDisplayName(): string {\n",
        "        return '';\n",
        "    }\n",
        "}\n",
        "\n",
        "$profile = new UserProfile();\n",
        "$profile->getUser()->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 20,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $profile->getUser()->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getName"),
                "Should include 'getName' from User via chain, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should include 'getEmail' from User via chain, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getProfile"),
                "Should include 'getProfile' from User via chain, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_chained_method_on_variable_inside_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_in_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "\n",
        "class UserProfile {\n",
        "    public function getUser(): User {\n",
        "        return new User();\n",
        "    }\n",
        "    public function test(): void {\n",
        "        $p = new UserProfile();\n",
        "        $p->getUser()->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 23,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $p->getUser()-> inside a method"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getName"),
                "Should include 'getName' from User via chain, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should include 'getEmail' from User via chain, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_top_level_variable_new_classname() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $email;\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "\n",
        "$user = new User();\n",
        "$user->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 7,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for top-level $user = new User()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getName"),
                "Should include 'getName', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should include 'getEmail', got: {:?}",
                method_names
            );

            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                prop_names.contains(&"email"),
                "Should include property 'email', got: {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_top_level_variable_from_function_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///toplevel_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "    public function getStatus(): string { return ''; }\n",
        "}\n",
        "\n",
        "function createOrder(): Order {\n",
        "    return new Order();\n",
        "}\n",
        "\n",
        "$order = createOrder();\n",
        "$order->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for top-level $order = createOrder()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getTotal"),
                "Should include 'getTotal', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getStatus"),
                "Should include 'getStatus', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── instanceof narrowing to interface / abstract class ─────────────────────

/// When narrowing via `instanceof` to an interface that is NOT one of the
/// existing union candidates (e.g. `mixed $x`), the interface's own
/// declared members should be offered.
#[tokio::test]
async fn test_completion_instanceof_narrows_to_interface_from_mixed() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_iface.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "interface Renderable {\n",                         // 1
        "    public function render(): string;\n",          // 2
        "    public function format(string $f): string;\n", // 3
        "}\n",                                              // 4
        "function handle(mixed $x): void {\n",              // 5
        "    if ($x instanceof Renderable) {\n",            // 6
        "        $x->\n",                                   // 7
        "    }\n",                                          // 8
        "}\n",                                              // 9
    );

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
                position: Position {
                    line: 7,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for interface members after instanceof narrowing"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"render"),
                "Should include Renderable::render(), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"format"),
                "Should include Renderable::format(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When narrowing via `instanceof` to an abstract class, members of the
/// abstract class should be offered even when the variable had no prior type.
#[tokio::test]
async fn test_completion_instanceof_narrows_to_abstract_class_from_mixed() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_abstract.php").unwrap();
    let text = concat!(
        "<?php\n",                                                 // 0
        "abstract class Shape {\n",                                // 1
        "    abstract public function area(): float;\n",           // 2
        "    public function describe(): string { return ''; }\n", // 3
        "}\n",                                                     // 4
        "function process(mixed $item): void {\n",                 // 5
        "    if ($item instanceof Shape) {\n",                     // 6
        "        $item->\n",                                       // 7
        "    }\n",                                                 // 8
        "}\n",                                                     // 9
    );

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
                position: Position {
                    line: 7,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for abstract class members after instanceof narrowing"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"area"),
                "Should include Shape::area(), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"describe"),
                "Should include Shape::describe(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// instanceof narrowing to an interface with no prior type — the variable
/// is completely untyped (no parameter hint at all).
#[tokio::test]
async fn test_completion_instanceof_narrows_untyped_variable_to_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_untyped.php").unwrap();
    let text = concat!(
        "<?php\n",                                                       // 0
        "interface Loggable {\n",                                        // 1
        "    public function log(string $msg): void;\n",                 // 2
        "}\n",                                                           // 3
        "class Svc {\n",                                                 // 4
        "    public function run(): void {\n",                           // 5
        "        $thing = $this->getSomething();\n",                     // 6
        "        if ($thing instanceof Loggable) {\n",                   // 7
        "            $thing->\n",                                        // 8
        "        }\n",                                                   // 9
        "    }\n",                                                       // 10
        "    private function getSomething(): mixed { return null; }\n", // 11
        "}\n",                                                           // 12
    );

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
                position: Position {
                    line: 8,
                    character: 20,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for Loggable members after instanceof narrowing on untyped var"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"log"),
                "Should include Loggable::log(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// instanceof narrowing to a cross-file interface via PSR-4.
#[tokio::test]
async fn test_completion_instanceof_narrows_to_cross_file_interface() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Contracts/Renderable.php",
            concat!(
                "<?php\n",
                "namespace App\\Contracts;\n",
                "interface Renderable {\n",
                "    public function render(): string;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///handler.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Contracts\\Renderable;\n",
        "function handle(mixed $x): void {\n",
        "    if ($x instanceof Renderable) {\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 4,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for cross-file interface after instanceof narrowing"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"render"),
                "Should include Renderable::render(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// instanceof narrowing to an interface that has @method magic members.
#[tokio::test]
async fn test_completion_instanceof_narrows_to_interface_with_magic_methods() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_magic.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "/**\n",                                   // 1
        " * @method string getName()\n",           // 2
        " * @method void setName(string $name)\n", // 3
        " */\n",                                   // 4
        "interface HasName {\n",                   // 5
        "}\n",                                     // 6
        "function greet(mixed $x): void {\n",      // 7
        "    if ($x instanceof HasName) {\n",      // 8
        "        $x->\n",                          // 9
        "    }\n",                                 // 10
        "}\n",                                     // 11
    );

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
                position: Position {
                    line: 9,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for interface @method members after instanceof"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should include @method getName(), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"setName"),
                "Should include @method setName(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When the variable has a union type (A|B) and instanceof narrows to an
/// interface C that is NOT in the union, the result should be C's members only.
#[tokio::test]
async fn test_completion_instanceof_narrows_disjoint_union_to_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_disjoint.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // 0
        "class Alpha {\n",                               // 1
        "    public function alphaMethod(): void {}\n",  // 2
        "}\n",                                           // 3
        "class Beta {\n",                                // 4
        "    public function betaMethod(): void {}\n",   // 5
        "}\n",                                           // 6
        "interface Serializable {\n",                    // 7
        "    public function serialize(): string;\n",    // 8
        "}\n",                                           // 9
        "class Svc {\n",                                 // 10
        "    public function run(): void {\n",           // 11
        "        if (rand(0,1)) {\n",                    // 12
        "            $obj = new Alpha();\n",             // 13
        "        } else {\n",                            // 14
        "            $obj = new Beta();\n",              // 15
        "        }\n",                                   // 16
        "        if ($obj instanceof Serializable) {\n", // 17
        "            $obj->\n",                          // 18
        "        }\n",                                   // 19
        "    }\n",                                       // 20
        "}\n",                                           // 21
    );

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
                position: Position {
                    line: 18,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for Serializable after instanceof on disjoint union"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"serialize"),
                "Should include Serializable::serialize(), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"alphaMethod"),
                "Should NOT include Alpha::alphaMethod(), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"betaMethod"),
                "Should NOT include Beta::betaMethod(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Guard clause with instanceof to an interface:
/// `if (!$x instanceof Renderable) { return; }` should narrow $x after the block.
#[tokio::test]
async fn test_completion_guard_clause_narrows_to_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_iface.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "interface Cacheable {\n",                      // 1
        "    public function getCacheKey(): string;\n", // 2
        "}\n",                                          // 3
        "function store(mixed $item): void {\n",        // 4
        "    if (!$item instanceof Cacheable) {\n",     // 5
        "        return;\n",                            // 6
        "    }\n",                                      // 7
        "    $item->\n",                                // 8
        "}\n",                                          // 9
    );

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
                position: Position {
                    line: 8,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for Cacheable after guard clause narrowing"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getCacheKey"),
                "Should include Cacheable::getCacheKey(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── assert($var instanceof …) narrowing ────────────────────────────────────

/// When `assert($var instanceof Foo)` appears before the cursor,
/// only members of `Foo` should be suggested.
#[tokio::test]
async fn test_completion_assert_instanceof_narrows_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_instanceof_basic.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class Animal {\n",                         // 1
        "    public function breathe(): void {}\n", // 2
        "}\n",                                      // 3
        "class Dog extends Animal {\n",             // 4
        "    public function bark(): void {}\n",    // 5
        "}\n",                                      // 6
        "class Cat extends Animal {\n",             // 7
        "    public function purr(): void {}\n",    // 8
        "}\n",                                      // 9
        "class Svc {\n",                            // 10
        "    public function test(): void {\n",     // 11
        "        if (rand(0,1)) {\n",               // 12
        "            $pet = new Dog();\n",          // 13
        "        } else {\n",                       // 14
        "            $pet = new Cat();\n",          // 15
        "        }\n",                              // 16
        "        assert($pet instanceof Dog);\n",   // 17
        "        $pet->\n",                         // 18
        "    }\n",                                  // 19
        "}\n",                                      // 20
    );

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
                position: Position {
                    line: 18,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's own method 'bark', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"breathe"),
                "Should include inherited method 'breathe', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's method 'purr', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `assert()` narrowing should work in top-level code (outside a class).
#[tokio::test]
async fn test_completion_assert_instanceof_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_instanceof_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "class Alpha {\n",                              // 1
        "    public function alphaMethod(): void {}\n", // 2
        "}\n",                                          // 3
        "class Beta {\n",                               // 4
        "    public function betaMethod(): void {}\n",  // 5
        "}\n",                                          // 6
        "if (rand(0,1)) {\n",                           // 7
        "    $obj = new Alpha();\n",                    // 8
        "} else {\n",                                   // 9
        "    $obj = new Beta();\n",                     // 10
        "}\n",                                          // 11
        "assert($obj instanceof Alpha);\n",             // 12
        "$obj->\n",                                     // 13
    );

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
                position: Position {
                    line: 13,
                    character: 6,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"alphaMethod"),
                "Should include 'alphaMethod', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"betaMethod"),
                "Should NOT include 'betaMethod' after assert, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `assert()` narrowing should work on parameters with type hints.
#[tokio::test]
async fn test_completion_assert_instanceof_narrows_parameter() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_instanceof_param.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Base {\n",                                   // 1
        "    public function baseMethod(): void {}\n",      // 2
        "}\n",                                              // 3
        "class Child extends Base {\n",                     // 4
        "    public function childMethod(): void {}\n",     // 5
        "}\n",                                              // 6
        "class Handler {\n",                                // 7
        "    public function handle(Base $item): void {\n", // 8
        "        assert($item instanceof Child);\n",        // 9
        "        $item->\n",                                // 10
        "    }\n",                                          // 11
        "}\n",                                              // 12
    );

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
                position: Position {
                    line: 10,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"childMethod"),
                "Should include Child's 'childMethod', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"baseMethod"),
                "Should include inherited 'baseMethod', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `assert()` with a different variable should not affect the target variable.
#[tokio::test]
async fn test_completion_assert_instanceof_different_variable_no_effect() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_instanceof_diffvar.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // 0
        "class Foo {\n",                              // 1
        "    public function fooMethod(): void {}\n", // 2
        "}\n",                                        // 3
        "class Bar {\n",                              // 4
        "    public function barMethod(): void {}\n", // 5
        "}\n",                                        // 6
        "class Svc {\n",                              // 7
        "    public function run(): void {\n",        // 8
        "        $a = new Foo();\n",                  // 9
        "        $b = new Bar();\n",                  // 10
        "        assert($b instanceof Foo);\n",       // 11
        "        $a->\n",                             // 12
        "    }\n",                                    // 13
        "}\n",                                        // 14
    );

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
                position: Position {
                    line: 12,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // $a is still Foo — the assert on $b should not affect $a
            assert!(
                method_names.contains(&"fooMethod"),
                "Should include 'fooMethod' for $a, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"barMethod"),
                "Should NOT include 'barMethod' for $a, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `assert()` narrowing should work with cross-file PSR-4 class resolution.
#[tokio::test]
async fn test_completion_assert_instanceof_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Models/User.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "class User {\n",
                "    public function addRoles(): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///assert_instanceof_cross.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Models\\User;\n",
        "class Base {\n",
        "    public function baseMethod(): void {}\n",
        "}\n",
        "function getUnknownValue(int $i): Base { return new Base(); }\n",
        "$asserted = getUnknownValue(1);\n",
        "assert($asserted instanceof User);\n",
        "$asserted->\n",
    );

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
                position: Position {
                    line: 8,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"addRoles"),
                "Should include User's 'addRoles' via cross-file resolution, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"baseMethod"),
                "Should NOT include Base's 'baseMethod' after assert narrowing, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `assert()` with parenthesised inner expression should also narrow.
#[tokio::test]
async fn test_completion_assert_instanceof_parenthesised() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_instanceof_parens.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class X {\n",                              // 1
        "    public function xMethod(): void {}\n", // 2
        "}\n",                                      // 3
        "class Y {\n",                              // 4
        "    public function yMethod(): void {}\n", // 5
        "}\n",                                      // 6
        "class Svc {\n",                            // 7
        "    public function run(): void {\n",      // 8
        "        if (rand(0,1)) {\n",               // 9
        "            $v = new X();\n",              // 10
        "        } else {\n",                       // 11
        "            $v = new Y();\n",              // 12
        "        }\n",                              // 13
        "        assert(($v instanceof X));\n",     // 14
        "        $v->\n",                           // 15
        "    }\n",                                  // 16
        "}\n",                                      // 17
    );

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
                position: Position {
                    line: 15,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"xMethod"),
                "Should include X's 'xMethod', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"yMethod"),
                "Should NOT include Y's 'yMethod' after assert narrowing, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── instanceof self/static/parent narrowing ────────────────────────────────

/// `assert($param instanceof self)` inside a class method should narrow
/// the variable to the current class. The Mago AST produces
/// `Expression::Self_` (not `Expression::Identifier`) for the `self` keyword.
#[tokio::test]
async fn test_completion_assert_instanceof_self_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_instanceof_self.php").unwrap();
    let text = concat!(
        "<?php\n",                                                   // 0
        "class BaseCatalogFeature {\n",                              // 1
        "    public function baseMethod(): void {}\n",               // 2
        "}\n",                                                       // 3
        "class SpecificFeature extends BaseCatalogFeature {\n",      // 4
        "    public function specificMethod(): void {}\n",           // 5
        "    public function test(BaseCatalogFeature $f): void {\n", // 6
        "        assert($f instanceof self);\n",                     // 7
        "        $f->\n",                                            // 8
        "    }\n",                                                   // 9
        "}\n",                                                       // 10
    );

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
                position: Position {
                    line: 8,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"specificMethod"),
                "Should include SpecificFeature's own 'specificMethod' after instanceof self, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"baseMethod"),
                "Should include inherited 'baseMethod' after instanceof self, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if ($param instanceof static)` should narrow to the current class.
/// The Mago AST produces `Expression::Static` for the `static` keyword.
#[tokio::test]
async fn test_completion_if_instanceof_static_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_static.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class BaseModel {\n",                               // 1
        "    public function baseMethod(): void {}\n",       // 2
        "}\n",                                               // 3
        "class UserModel extends BaseModel {\n",             // 4
        "    public function userMethod(): void {}\n",       // 5
        "    public function check(BaseModel $m): void {\n", // 6
        "        if ($m instanceof static) {\n",             // 7
        "            $m->\n",                                // 8
        "        }\n",                                       // 9
        "    }\n",                                           // 10
        "}\n",                                               // 11
    );

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
                position: Position {
                    line: 8,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"userMethod"),
                "Should include UserModel's 'userMethod' after instanceof static, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"baseMethod"),
                "Should include inherited 'baseMethod' after instanceof static, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if ($param instanceof parent)` should narrow to the parent class.
/// The Mago AST produces `Expression::Parent` for the `parent` keyword.
#[tokio::test]
async fn test_completion_if_instanceof_parent_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///instanceof_parent.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // 0
        "class Vehicle {\n",                               // 1
        "    public function drive(): void {}\n",          // 2
        "}\n",                                             // 3
        "class Car extends Vehicle {\n",                   // 4
        "    public function honk(): void {}\n",           // 5
        "}\n",                                             // 6
        "class SportsCar extends Car {\n",                 // 7
        "    public function turbo(): void {}\n",          // 8
        "    public function check(Vehicle $v): void {\n", // 9
        "        if ($v instanceof parent) {\n",           // 10
        "            $v->\n",                              // 11
        "        }\n",                                     // 12
        "    }\n",                                         // 13
        "}\n",                                             // 14
    );

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
                position: Position {
                    line: 11,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"honk"),
                "Should include Car's 'honk' after instanceof parent, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"drive"),
                "Should include Vehicle's 'drive' after instanceof parent, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Guard clause with `instanceof self` and early return should apply
/// negative narrowing for subsequent code.
#[tokio::test]
async fn test_guard_clause_instanceof_self_excludes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_instanceof_self.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class Animal {\n",                         // 1
        "    public function breathe(): void {}\n", // 2
        "}\n",                                      // 3
        "class Dog extends Animal {\n",             // 4
        "    public function bark(): void {}\n",    // 5
        "}\n",                                      // 6
        "class Cat extends Animal {\n",             // 7
        "    public function purr(): void {}\n",    // 8
        "    public function handle(): void {\n",   // 9
        "        if (rand(0,1)) {\n",               // 10
        "            $a = new Dog();\n",            // 11
        "        } else {\n",                       // 12
        "            $a = new Cat();\n",            // 13
        "        }\n",                              // 14
        "        if ($a instanceof self) {\n",      // 15
        "            return;\n",                    // 16
        "        }\n",                              // 17
        "        $a->\n",                           // 18
        "    }\n",                                  // 19
        "}\n",                                      // 20
    );

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
                position: Position {
                    line: 18,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's 'bark' after guard excludes Cat (self), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after guard clause with instanceof self, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── negated instanceof narrowing ───────────────────────────────────────────

/// `assert(!$var instanceof ClassName)` should *exclude* ClassName from
/// the candidate set (PHP precedence: `instanceof` binds tighter than `!`,
/// so `!$x instanceof Foo` ≡ `!($x instanceof Foo)`).
#[tokio::test]
async fn test_completion_assert_negated_instanceof_excludes_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_neg_instanceof.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "class Alpha {\n",                              // 1
        "    public function alphaMethod(): void {}\n", // 2
        "}\n",                                          // 3
        "class Beta {\n",                               // 4
        "    public function betaMethod(): void {}\n",  // 5
        "}\n",                                          // 6
        "class Svc {\n",                                // 7
        "    public function run(): void {\n",          // 8
        "        if (rand(0,1)) {\n",                   // 9
        "            $obj = new Alpha();\n",            // 10
        "        } else {\n",                           // 11
        "            $obj = new Beta();\n",             // 12
        "        }\n",                                  // 13
        "        assert(!$obj instanceof Alpha);\n",    // 14
        "        $obj->\n",                             // 15
        "    }\n",                                      // 16
        "}\n",                                          // 17
    );

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
                position: Position {
                    line: 15,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"betaMethod"),
                "Should include Beta's 'betaMethod' after excluding Alpha, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"alphaMethod"),
                "Should NOT include Alpha's 'alphaMethod' after negated assert, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `assert(!($var instanceof ClassName))` with explicit parens should
/// also exclude ClassName.
#[tokio::test]
async fn test_completion_assert_negated_instanceof_parenthesised_excludes_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_neg_instanceof_parens.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // 0
        "class Foo {\n",                              // 1
        "    public function fooMethod(): void {}\n", // 2
        "}\n",                                        // 3
        "class Bar {\n",                              // 4
        "    public function barMethod(): void {}\n", // 5
        "}\n",                                        // 6
        "class Svc {\n",                              // 7
        "    public function run(): void {\n",        // 8
        "        if (rand(0,1)) {\n",                 // 9
        "            $x = new Foo();\n",              // 10
        "        } else {\n",                         // 11
        "            $x = new Bar();\n",              // 12
        "        }\n",                                // 13
        "        assert(!($x instanceof Foo));\n",    // 14
        "        $x->\n",                             // 15
        "    }\n",                                    // 16
        "}\n",                                        // 17
    );

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
                position: Position {
                    line: 15,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"barMethod"),
                "Should include Bar's 'barMethod', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"fooMethod"),
                "Should NOT include Foo's 'fooMethod' after negated assert, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if (!($var instanceof ClassName)) { … }` should exclude ClassName
/// inside the if body.
#[tokio::test]
async fn test_completion_if_negated_instanceof_excludes_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///if_neg_instanceof.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "class Cat {\n",                           // 1
        "    public function purr(): void {}\n",   // 2
        "}\n",                                     // 3
        "class Dog {\n",                           // 4
        "    public function bark(): void {}\n",   // 5
        "}\n",                                     // 6
        "class Svc {\n",                           // 7
        "    public function run(): void {\n",     // 8
        "        if (rand(0,1)) {\n",              // 9
        "            $pet = new Cat();\n",         // 10
        "        } else {\n",                      // 11
        "            $pet = new Dog();\n",         // 12
        "        }\n",                             // 13
        "        if (!($pet instanceof Cat)) {\n", // 14
        "            $pet->\n",                    // 15
        "        }\n",                             // 16
        "    }\n",                                 // 17
        "}\n",                                     // 18
    );

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
                position: Position {
                    line: 15,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's 'bark' inside negated instanceof block, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' inside negated instanceof block, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if (!$var instanceof ClassName)` without explicit inner parens should
/// also exclude (PHP precedence: `instanceof` > `!`).
#[tokio::test]
async fn test_completion_if_negated_instanceof_no_inner_parens() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///if_neg_instanceof_no_parens.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // 0
        "class A1 {\n",                              // 1
        "    public function a1Method(): void {}\n", // 2
        "}\n",                                       // 3
        "class B1 {\n",                              // 4
        "    public function b1Method(): void {}\n", // 5
        "}\n",                                       // 6
        "class Svc {\n",                             // 7
        "    public function run(): void {\n",       // 8
        "        if (rand(0,1)) {\n",                // 9
        "            $v = new A1();\n",              // 10
        "        } else {\n",                        // 11
        "            $v = new B1();\n",              // 12
        "        }\n",                               // 13
        "        if (!$v instanceof A1) {\n",        // 14
        "            $v->\n",                        // 15
        "        }\n",                               // 16
        "    }\n",                                   // 17
        "}\n",                                       // 18
    );

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
                position: Position {
                    line: 15,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"b1Method"),
                "Should include B1's 'b1Method', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"a1Method"),
                "Should NOT include A1's 'a1Method' after negated instanceof, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if ($var instanceof ClassName) { … } else { … }` — inside the else
/// branch the variable is NOT ClassName, so exclude it.
#[tokio::test]
async fn test_completion_if_instanceof_else_excludes_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///if_instanceof_else.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "class Red {\n",                               // 1
        "    public function redMethod(): void {}\n",  // 2
        "}\n",                                         // 3
        "class Blue {\n",                              // 4
        "    public function blueMethod(): void {}\n", // 5
        "}\n",                                         // 6
        "class Svc {\n",                               // 7
        "    public function run(): void {\n",         // 8
        "        if (rand(0,1)) {\n",                  // 9
        "            $c = new Red();\n",               // 10
        "        } else {\n",                          // 11
        "            $c = new Blue();\n",              // 12
        "        }\n",                                 // 13
        "        if ($c instanceof Red) {\n",          // 14
        "            // narrowed to Red here\n",       // 15
        "        } else {\n",                          // 16
        "            $c->\n",                          // 17
        "        }\n",                                 // 18
        "    }\n",                                     // 19
        "}\n",                                         // 20
    );

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
                position: Position {
                    line: 17,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"blueMethod"),
                "Should include Blue's 'blueMethod' in else branch, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"redMethod"),
                "Should NOT include Red's 'redMethod' in else branch, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if ($var instanceof ClassName) { … } else { … }` — the then branch
/// should still narrow positively (regression check).
#[tokio::test]
async fn test_completion_if_instanceof_then_still_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///if_instanceof_then_check.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "class Red {\n",                               // 1
        "    public function redMethod(): void {}\n",  // 2
        "}\n",                                         // 3
        "class Blue {\n",                              // 4
        "    public function blueMethod(): void {}\n", // 5
        "}\n",                                         // 6
        "class Svc {\n",                               // 7
        "    public function run(): void {\n",         // 8
        "        if (rand(0,1)) {\n",                  // 9
        "            $c = new Red();\n",               // 10
        "        } else {\n",                          // 11
        "            $c = new Blue();\n",              // 12
        "        }\n",                                 // 13
        "        if ($c instanceof Red) {\n",          // 14
        "            $c->\n",                          // 15
        "        } else {\n",                          // 16
        "            // excluded here\n",              // 17
        "        }\n",                                 // 18
        "    }\n",                                     // 19
        "}\n",                                         // 20
    );

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
                position: Position {
                    line: 15,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"redMethod"),
                "Should include Red's 'redMethod' in then branch, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"blueMethod"),
                "Should NOT include Blue's 'blueMethod' in then branch, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if ($param instanceof ClassName) { … } else { … }` — the else branch
/// should exclude ClassName when the variable comes from a parameter.
#[tokio::test]
async fn test_completion_if_instanceof_else_excludes_with_parameter() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///if_instanceof_else_param.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "class Sun {\n",                                  // 1
        "    public function shine(): void {}\n",         // 2
        "}\n",                                            // 3
        "class Moon {\n",                                 // 4
        "    public function glow(): void {}\n",          // 5
        "}\n",                                            // 6
        "class Svc {\n",                                  // 7
        "    public function run(Sun|Moon $s): void {\n", // 8
        "        if ($s instanceof Sun) {\n",             // 9
        "            // narrowed to Sun\n",               // 10
        "        } else {\n",                             // 11
        "            $s->\n",                             // 12
        "        }\n",                                    // 13
        "    }\n",                                        // 14
        "}\n",                                            // 15
    );

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
                position: Position {
                    line: 12,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"glow"),
                "Should include Moon's 'glow' in else branch, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"shine"),
                "Should NOT include Sun's 'shine' in else branch, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Top-level `if ($var instanceof …) {} else {}` should also narrow in
/// the else branch.
#[tokio::test]
async fn test_completion_if_instanceof_else_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///if_instanceof_else_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "class Up {\n",                          // 1
        "    public function rise(): void {}\n", // 2
        "}\n",                                   // 3
        "class Down {\n",                        // 4
        "    public function fall(): void {}\n", // 5
        "}\n",                                   // 6
        "if (rand(0,1)) {\n",                    // 7
        "    $dir = new Up();\n",                // 8
        "} else {\n",                            // 9
        "    $dir = new Down();\n",              // 10
        "}\n",                                   // 11
        "if ($dir instanceof Up) {\n",           // 12
        "    // narrowed to Up\n",               // 13
        "} else {\n",                            // 14
        "    $dir->\n",                          // 15
        "}\n",                                   // 16
    );

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
                position: Position {
                    line: 15,
                    character: 10,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"fall"),
                "Should include Down's 'fall' in else branch, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"rise"),
                "Should NOT include Up's 'rise' in else branch, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── while-loop instanceof narrowing ────────────────────────────────────────

/// When the cursor is inside `while ($var instanceof Foo) { … }`, only
/// members of `Foo` should be suggested.
#[tokio::test]
async fn test_completion_while_instanceof_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///while_instanceof.php").unwrap();
    let text = concat!(
        "<?php\n",                                             // 0
        "class Node {\n",                                      // 1
        "    public function next(): ?Node {}\n",              // 2
        "    public function getValue(): string {}\n",         // 3
        "}\n",                                                 // 4
        "class Leaf {\n",                                      // 5
        "    public function leafOnly(): void {}\n",           // 6
        "}\n",                                                 // 7
        "class Svc {\n",                                       // 8
        "    public function walk(Node|Leaf $item): void {\n", // 9
        "        while ($item instanceof Node) {\n",           // 10
        "            $item->\n",                               // 11
        "        }\n",                                         // 12
        "    }\n",                                             // 13
        "}\n",                                                 // 14
    );

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
                position: Position {
                    line: 11,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"next"),
                "Should include Node's 'next' inside while body, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getValue"),
                "Should include Node's 'getValue' inside while body, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"leafOnly"),
                "Should NOT include Leaf's 'leafOnly' inside while body, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Negated instanceof in a while condition should exclude the class.
#[tokio::test]
async fn test_completion_while_negated_instanceof_excludes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///while_neg_instanceof.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class Alpha {\n",                                   // 1
        "    public function alphaMethod(): void {}\n",      // 2
        "}\n",                                               // 3
        "class Beta {\n",                                    // 4
        "    public function betaMethod(): void {}\n",       // 5
        "}\n",                                               // 6
        "class Svc {\n",                                     // 7
        "    public function test(Alpha|Beta $x): void {\n", // 8
        "        while (!($x instanceof Alpha)) {\n",        // 9
        "            $x->\n",                                // 10
        "        }\n",                                       // 11
        "    }\n",                                           // 12
        "}\n",                                               // 13
    );

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
                position: Position {
                    line: 10,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"betaMethod"),
                "Should include Beta's 'betaMethod' when Alpha is excluded, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"alphaMethod"),
                "Should NOT include Alpha's 'alphaMethod' when Alpha is excluded, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── is_a() narrowing ──────────────────────────────────────────────────────

/// `if (is_a($var, Foo::class))` should narrow like instanceof.
#[tokio::test]
async fn test_completion_is_a_narrows_like_instanceof() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///is_a_narrow.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Dog {\n",                                    // 1
        "    public function bark(): void {}\n",            // 2
        "}\n",                                              // 3
        "class Cat {\n",                                    // 4
        "    public function purr(): void {}\n",            // 5
        "}\n",                                              // 6
        "class Svc {\n",                                    // 7
        "    public function test(Dog|Cat $pet): void {\n", // 8
        "        if (is_a($pet, Dog::class)) {\n",          // 9
        "            $pet->\n",                             // 10
        "        }\n",                                      // 11
        "    }\n",                                          // 12
        "}\n",                                              // 13
    );

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
                position: Position {
                    line: 10,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's 'bark' inside is_a block, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' inside is_a block, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if (!is_a($var, Foo::class))` should exclude Foo (negated is_a).
#[tokio::test]
async fn test_completion_negated_is_a_excludes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///is_a_neg.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Dog {\n",                                    // 1
        "    public function bark(): void {}\n",            // 2
        "}\n",                                              // 3
        "class Cat {\n",                                    // 4
        "    public function purr(): void {}\n",            // 5
        "}\n",                                              // 6
        "class Svc {\n",                                    // 7
        "    public function test(Dog|Cat $pet): void {\n", // 8
        "        if (!is_a($pet, Dog::class)) {\n",         // 9
        "            $pet->\n",                             // 10
        "        }\n",                                      // 11
        "    }\n",                                          // 12
        "}\n",                                              // 13
    );

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
                position: Position {
                    line: 10,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"purr"),
                "Should include Cat's 'purr' when Dog is excluded, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"bark"),
                "Should NOT include Dog's 'bark' when Dog is excluded, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `is_a()` else branch should invert narrowing (exclude the matched class).
#[tokio::test]
async fn test_completion_is_a_else_branch_excludes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///is_a_else.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Dog {\n",                                    // 1
        "    public function bark(): void {}\n",            // 2
        "}\n",                                              // 3
        "class Cat {\n",                                    // 4
        "    public function purr(): void {}\n",            // 5
        "}\n",                                              // 6
        "class Svc {\n",                                    // 7
        "    public function test(Dog|Cat $pet): void {\n", // 8
        "        if (is_a($pet, Dog::class)) {\n",          // 9
        "            // dog branch\n",                      // 10
        "        } else {\n",                               // 11
        "            $pet->\n",                             // 12
        "        }\n",                                      // 13
        "    }\n",                                          // 14
        "}\n",                                              // 15
    );

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
                position: Position {
                    line: 12,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"purr"),
                "Should include Cat's 'purr' in else branch (Dog excluded), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"bark"),
                "Should NOT include Dog's 'bark' in else branch, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── get_class() / $var::class narrowing ────────────────────────────────────

/// `if (get_class($var) === Foo::class)` should narrow to exactly Foo.
#[tokio::test]
async fn test_completion_get_class_identical_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///get_class_narrow.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser extends User {\n",                      // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "class Svc {\n",                                         // 7
        "    public function test(User|AdminUser $u): void {\n", // 8
        "        if (get_class($u) === User::class) {\n",        // 9
        "            $u->\n",                                    // 10
        "        }\n",                                           // 11
        "    }\n",                                               // 12
        "}\n",                                                   // 13
    );

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
                position: Position {
                    line: 10,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should include User's 'getName' when get_class matches User, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"addRoles"),
                "Should NOT include AdminUser's 'addRoles' when get_class matches User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if ($var::class === Foo::class)` should narrow to exactly Foo.
#[tokio::test]
async fn test_completion_var_class_constant_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_class_narrow.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser extends User {\n",                      // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "class Svc {\n",                                         // 7
        "    public function test(User|AdminUser $u): void {\n", // 8
        "        if ($u::class === User::class) {\n",            // 9
        "            $u->\n",                                    // 10
        "        }\n",                                           // 11
        "    }\n",                                               // 12
        "}\n",                                                   // 13
    );

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
                position: Position {
                    line: 10,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should include User's 'getName' when $u::class === User::class, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"addRoles"),
                "Should NOT include AdminUser's 'addRoles' when $u::class === User::class, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if (get_class($var) !== Foo::class)` should exclude Foo (negated identity).
#[tokio::test]
async fn test_completion_get_class_not_identical_excludes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///get_class_neg.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "class Svc {\n",                                         // 7
        "    public function test(User|AdminUser $u): void {\n", // 8
        "        if (get_class($u) !== User::class) {\n",        // 9
        "            $u->\n",                                    // 10
        "        }\n",                                           // 11
        "    }\n",                                               // 12
        "}\n",                                                   // 13
    );

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
                position: Position {
                    line: 10,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"addRoles"),
                "Should include AdminUser's 'addRoles' when User is excluded, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"getName"),
                "Should NOT include User's 'getName' when User is excluded, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `get_class($var) === Foo::class` else branch should exclude Foo.
#[tokio::test]
async fn test_completion_get_class_else_branch_excludes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///get_class_else.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "class Svc {\n",                                         // 7
        "    public function test(User|AdminUser $u): void {\n", // 8
        "        if (get_class($u) === User::class) {\n",        // 9
        "            // user branch\n",                          // 10
        "        } else {\n",                                    // 11
        "            $u->\n",                                    // 12
        "        }\n",                                           // 13
        "    }\n",                                               // 14
        "}\n",                                                   // 15
    );

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
                position: Position {
                    line: 12,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"addRoles"),
                "Should include AdminUser's 'addRoles' in else branch, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"getName"),
                "Should NOT include User's 'getName' in else branch (get_class matched), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Reversed order: `Foo::class === get_class($var)` should also narrow.
#[tokio::test]
async fn test_completion_get_class_reversed_order_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///get_class_reversed.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "class Svc {\n",                                         // 7
        "    public function test(User|AdminUser $u): void {\n", // 8
        "        if (User::class === get_class($u)) {\n",        // 9
        "            $u->\n",                                    // 10
        "        }\n",                                           // 11
        "    }\n",                                               // 12
        "}\n",                                                   // 13
    );

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
                position: Position {
                    line: 10,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should include User's 'getName' with reversed order, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"addRoles"),
                "Should NOT include AdminUser's 'addRoles' with reversed order, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── match(true) instanceof narrowing ───────────────────────────────────────

/// Inside a `match (true) { $var instanceof Foo => … }` arm body,
/// the variable should be narrowed to Foo.
#[tokio::test]
async fn test_completion_match_true_instanceof_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_true_narrow.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "class Svc {\n",                                         // 7
        "    public function test(User|AdminUser $v): void {\n", // 8
        "        $result = match (true) {\n",                    // 9
        "            $v instanceof AdminUser => $v->\n",         // 10
        "            default => null,\n",                        // 11
        "        };\n",                                          // 12
        "    }\n",                                               // 13
        "}\n",                                                   // 14
    );

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
                position: Position {
                    line: 10,
                    character: 49,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"addRoles"),
                "Should include AdminUser's 'addRoles' in match arm, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"getName"),
                "Should NOT include User's 'getName' in match arm, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `match (true)` with is_a() in arm condition should also narrow.
#[tokio::test]
async fn test_completion_match_true_is_a_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_true_is_a.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Dog {\n",                                    // 1
        "    public function bark(): void {}\n",            // 2
        "}\n",                                              // 3
        "class Cat {\n",                                    // 4
        "    public function purr(): void {}\n",            // 5
        "}\n",                                              // 6
        "class Svc {\n",                                    // 7
        "    public function test(Dog|Cat $pet): void {\n", // 8
        "        $result = match (true) {\n",               // 9
        "            is_a($pet, Cat::class) => $pet->\n",   // 10
        "            default => null,\n",                   // 11
        "        };\n",                                     // 12
        "    }\n",                                          // 13
        "}\n",                                              // 14
    );

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
                position: Position {
                    line: 10,
                    character: 50,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"purr"),
                "Should include Cat's 'purr' in match arm with is_a, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"bark"),
                "Should NOT include Dog's 'bark' in match arm with is_a, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `assert(is_a($var, Foo::class))` should narrow unconditionally.
#[tokio::test]
async fn test_completion_assert_is_a_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_is_a.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Dog {\n",                                    // 1
        "    public function bark(): void {}\n",            // 2
        "}\n",                                              // 3
        "class Cat {\n",                                    // 4
        "    public function purr(): void {}\n",            // 5
        "}\n",                                              // 6
        "class Svc {\n",                                    // 7
        "    public function test(Dog|Cat $pet): void {\n", // 8
        "        assert(is_a($pet, Dog::class));\n",        // 9
        "        $pet->\n",                                 // 10
        "    }\n",                                          // 11
        "}\n",                                              // 12
    );

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
                position: Position {
                    line: 10,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's 'bark' after assert(is_a()), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after assert(is_a()), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `while` narrowing with `is_a()` should also work.
#[tokio::test]
async fn test_completion_while_is_a_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///while_is_a.php").unwrap();
    let text = concat!(
        "<?php\n",                                             // 0
        "class Node {\n",                                      // 1
        "    public function next(): ?Node {}\n",              // 2
        "    public function getValue(): string {}\n",         // 3
        "}\n",                                                 // 4
        "class Leaf {\n",                                      // 5
        "    public function leafOnly(): void {}\n",           // 6
        "}\n",                                                 // 7
        "class Svc {\n",                                       // 8
        "    public function walk(Node|Leaf $item): void {\n", // 9
        "        while (is_a($item, Node::class)) {\n",        // 10
        "            $item->\n",                               // 11
        "        }\n",                                         // 12
        "    }\n",                                             // 13
        "}\n",                                                 // 14
    );

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
                position: Position {
                    line: 11,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"next"),
                "Should include Node's 'next' inside while body with is_a, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"leafOnly"),
                "Should NOT include Leaf's 'leafOnly' inside while body with is_a, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$var::class === Foo::class` else branch should exclude Foo.
#[tokio::test]
async fn test_completion_var_class_constant_else_branch_excludes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_class_else.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "class Svc {\n",                                         // 7
        "    public function test(User|AdminUser $u): void {\n", // 8
        "        if ($u::class === User::class) {\n",            // 9
        "            // user branch\n",                          // 10
        "        } else {\n",                                    // 11
        "            $u->\n",                                    // 12
        "        }\n",                                           // 13
        "    }\n",                                               // 14
        "}\n",                                                   // 15
    );

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
                position: Position {
                    line: 12,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"addRoles"),
                "Should include AdminUser's 'addRoles' in else branch, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"getName"),
                "Should NOT include User's 'getName' in else branch ($u::class matched), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `get_class()` narrowing with `==` (loose equality) should also work.
#[tokio::test]
async fn test_completion_get_class_loose_equality_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///get_class_eq.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "class Svc {\n",                                         // 7
        "    public function test(User|AdminUser $u): void {\n", // 8
        "        if (get_class($u) == User::class) {\n",         // 9
        "            $u->\n",                                    // 10
        "        }\n",                                           // 11
        "    }\n",                                               // 12
        "}\n",                                                   // 13
    );

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
                position: Position {
                    line: 10,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should include User's 'getName' with loose == comparison, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"addRoles"),
                "Should NOT include AdminUser's 'addRoles' with loose == comparison, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── @phpstan-assert / @psalm-assert narrowing ──────────────────────────────

/// `@phpstan-assert User $value` on a standalone function call should narrow
/// the variable unconditionally after the call.
#[tokio::test]
async fn test_completion_phpstan_assert_narrows_unconditionally() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///phpstan_assert_basic.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "/**\n",                                                 // 7
        " * @phpstan-assert User $value\n",                      // 8
        " */\n",                                                 // 9
        "function assertUser($value): void {}\n",                // 10
        "class Svc {\n",                                         // 11
        "    public function test(User|AdminUser $v): void {\n", // 12
        "        assertUser($v);\n",                             // 13
        "        $v->\n",                                        // 14
        "    }\n",                                               // 15
        "}\n",                                                   // 16
    );

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
                position: Position {
                    line: 14,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should include User's 'getName' after assertUser(), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"addRoles"),
                "Should NOT include AdminUser's 'addRoles' after assertUser(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Negated `@phpstan-assert !User $value` should exclude User.
#[tokio::test]
async fn test_completion_phpstan_assert_negated_excludes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///phpstan_assert_neg.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "/**\n",                                                 // 7
        " * @phpstan-assert !User $value\n",                     // 8
        " */\n",                                                 // 9
        "function assertNotUser($value): void {}\n",             // 10
        "class Svc {\n",                                         // 11
        "    public function test(User|AdminUser $v): void {\n", // 12
        "        assertNotUser($v);\n",                          // 13
        "        $v->\n",                                        // 14
        "    }\n",                                               // 15
        "}\n",                                                   // 16
    );

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
                position: Position {
                    line: 14,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"addRoles"),
                "Should include AdminUser's 'addRoles' after assertNotUser(), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"getName"),
                "Should NOT include User's 'getName' after assertNotUser(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `@psalm-assert` should work identically to `@phpstan-assert`.
#[tokio::test]
async fn test_completion_psalm_assert_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///psalm_assert.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "class Dog {\n",                                  // 1
        "    public function bark(): void {}\n",          // 2
        "}\n",                                            // 3
        "class Cat {\n",                                  // 4
        "    public function purr(): void {}\n",          // 5
        "}\n",                                            // 6
        "/**\n",                                          // 7
        " * @psalm-assert Dog $animal\n",                 // 8
        " */\n",                                          // 9
        "function assertDog($animal): void {}\n",         // 10
        "class Svc {\n",                                  // 11
        "    public function test(Dog|Cat $a): void {\n", // 12
        "        assertDog($a);\n",                       // 13
        "        $a->\n",                                 // 14
        "    }\n",                                        // 15
        "}\n",                                            // 16
    );

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
                position: Position {
                    line: 14,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's 'bark' after assertDog() with @psalm-assert, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after assertDog() with @psalm-assert, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── @phpstan-assert-if-true narrowing ──────────────────────────────────────

/// `@phpstan-assert-if-true User $value` should narrow inside the if-body
/// when the function is used as a condition.
#[tokio::test]
async fn test_completion_phpstan_assert_if_true_narrows_in_then() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_if_true_then.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "/**\n",                                                 // 7
        " * @phpstan-assert-if-true User $value\n",              // 8
        " */\n",                                                 // 9
        "function isUser($value): bool {}\n",                    // 10
        "class Svc {\n",                                         // 11
        "    public function test(User|AdminUser $v): void {\n", // 12
        "        if (isUser($v)) {\n",                           // 13
        "            $v->\n",                                    // 14
        "        }\n",                                           // 15
        "    }\n",                                               // 16
        "}\n",                                                   // 17
    );

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
                position: Position {
                    line: 14,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should include User's 'getName' in if-body with assert-if-true, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"addRoles"),
                "Should NOT include AdminUser's 'addRoles' in if-body with assert-if-true, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `@phpstan-assert-if-true User $value` in the else-body means the function
/// returned false, so $value is NOT User → exclude User from the union.
#[tokio::test]
async fn test_completion_phpstan_assert_if_true_excludes_in_else() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_if_true_else.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "/**\n",                                                 // 7
        " * @phpstan-assert-if-true User $value\n",              // 8
        " */\n",                                                 // 9
        "function isUser($value): bool {}\n",                    // 10
        "class Svc {\n",                                         // 11
        "    public function test(User|AdminUser $v): void {\n", // 12
        "        if (isUser($v)) {\n",                           // 13
        "            // then branch\n",                          // 14
        "        } else {\n",                                    // 15
        "            $v->\n",                                    // 16
        "        }\n",                                           // 17
        "    }\n",                                               // 18
        "}\n",                                                   // 19
    );

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
                position: Position {
                    line: 16,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // In the else branch, function returned false → User is excluded
            // from User|AdminUser, leaving only AdminUser.
            assert!(
                method_names.contains(&"addRoles"),
                "Should include AdminUser's 'addRoles' in else branch (User excluded), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"getName"),
                "Should NOT include User's 'getName' in else branch (User excluded), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Negated condition `if (!isUser($v))` with `@phpstan-assert-if-true`
/// should NOT narrow in the then-body (function returned false).
/// But the else-body should narrow (function returned true).
#[tokio::test]
async fn test_completion_phpstan_assert_if_true_negated_narrows_in_else() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_if_true_neg.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "class User {\n",                                         // 1
        "    public function getName(): string {}\n",             // 2
        "}\n",                                                    // 3
        "class AdminUser {\n",                                    // 4
        "    public function addRoles(string $r): void {}\n",     // 5
        "}\n",                                                    // 6
        "/**\n",                                                  // 7
        " * @phpstan-assert-if-true User $value\n",               // 8
        " */\n",                                                  // 9
        "function isUser($value): bool {}\n",                     // 10
        "class Svc {\n",                                          // 11
        "    public function test(User|AdminUser $v): void {\n",  // 12
        "        if (!isUser($v)) {\n",                           // 13
        "            // negated then: function returned false\n", // 14
        "        } else {\n",                                     // 15
        "            $v->\n",                                     // 16
        "        }\n",                                            // 17
        "    }\n",                                                // 18
        "}\n",                                                    // 19
    );

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
                position: Position {
                    line: 16,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // `!isUser($v)` in then means func returned false.
            // In else, func returned true → IfTrue applies → narrow to User.
            assert!(
                method_names.contains(&"getName"),
                "Should include User's 'getName' in else of negated assert-if-true, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"addRoles"),
                "Should NOT include AdminUser's 'addRoles' in else of negated assert-if-true, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── @phpstan-assert-if-false narrowing ─────────────────────────────────────

/// `@phpstan-assert-if-false User $value` should narrow in the else-body
/// (function returned false → assertion holds).
#[tokio::test]
async fn test_completion_phpstan_assert_if_false_narrows_in_else() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_if_false_else.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "/**\n",                                                 // 7
        " * @phpstan-assert-if-false User $value\n",             // 8
        " */\n",                                                 // 9
        "function isNotUser($value): bool {}\n",                 // 10
        "class Svc {\n",                                         // 11
        "    public function test(User|AdminUser $v): void {\n", // 12
        "        if (isNotUser($v)) {\n",                        // 13
        "            // then branch: function returned true\n",  // 14
        "        } else {\n",                                    // 15
        "            $v->\n",                                    // 16
        "        }\n",                                           // 17
        "    }\n",                                               // 18
        "}\n",                                                   // 19
    );

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
                position: Position {
                    line: 16,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Else body: function returned false → IfFalse assertion applies
            assert!(
                method_names.contains(&"getName"),
                "Should include User's 'getName' in else with assert-if-false, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"addRoles"),
                "Should NOT include AdminUser's 'addRoles' in else with assert-if-false, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `@phpstan-assert-if-false User $value` in the then-body means the function
/// returned true, so $value is NOT User → exclude User from the union.
#[tokio::test]
async fn test_completion_phpstan_assert_if_false_excludes_in_then() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_if_false_then.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "/**\n",                                                 // 7
        " * @phpstan-assert-if-false User $value\n",             // 8
        " */\n",                                                 // 9
        "function isNotUser($value): bool {}\n",                 // 10
        "class Svc {\n",                                         // 11
        "    public function test(User|AdminUser $v): void {\n", // 12
        "        if (isNotUser($v)) {\n",                        // 13
        "            $v->\n",                                    // 14
        "        }\n",                                           // 15
        "    }\n",                                               // 16
        "}\n",                                                   // 17
    );

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
                position: Position {
                    line: 14,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Then body: function returned true → User is excluded
            // from User|AdminUser, leaving only AdminUser.
            assert!(
                method_names.contains(&"addRoles"),
                "Should include AdminUser's 'addRoles' in then branch (User excluded), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"getName"),
                "Should NOT include User's 'getName' in then branch (User excluded), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `@phpstan-assert-if-true` in a while-loop condition should narrow
/// inside the loop body.
#[tokio::test]
async fn test_completion_phpstan_assert_if_true_in_while() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_if_true_while.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Node {\n",                                   // 1
        "    public function next(): ?Node {}\n",           // 2
        "    public function getValue(): string {}\n",      // 3
        "}\n",                                              // 4
        "class Leaf {\n",                                   // 5
        "    public function leafOnly(): void {}\n",        // 6
        "}\n",                                              // 7
        "/**\n",                                            // 8
        " * @phpstan-assert-if-true Node $item\n",          // 9
        " */\n",                                            // 10
        "function isNode($item): bool {}\n",                // 11
        "class Svc {\n",                                    // 12
        "    public function walk(Node|Leaf $n): void {\n", // 13
        "        while (isNode($n)) {\n",                   // 14
        "            $n->\n",                               // 15
        "        }\n",                                      // 16
        "    }\n",                                          // 17
        "}\n",                                              // 18
    );

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
                position: Position {
                    line: 15,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getValue"),
                "Should include Node's 'getValue' in while body with assert-if-true, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"leafOnly"),
                "Should NOT include Leaf's 'leafOnly' in while body with assert-if-true, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `@phpstan-assert` on the second parameter should narrow the correct variable.
#[tokio::test]
async fn test_completion_phpstan_assert_second_parameter() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///phpstan_assert_second.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "/**\n",                                                 // 7
        " * @phpstan-assert User $obj\n",                        // 8
        " */\n",                                                 // 9
        "function assertType(string $class, $obj): void {}\n",   // 10
        "class Svc {\n",                                         // 11
        "    public function test(User|AdminUser $v): void {\n", // 12
        "        assertType(User::class, $v);\n",                // 13
        "        $v->\n",                                        // 14
        "    }\n",                                               // 15
        "}\n",                                                   // 16
    );

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
                position: Position {
                    line: 14,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should include User's 'getName' (assert on second param), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"addRoles"),
                "Should NOT include AdminUser's 'addRoles' (assert on second param), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `@phpstan-assert-if-true $this` on an instance method narrows the
/// receiver variable in the then-body.
///
/// Pattern (common in Drupal and other frameworks):
/// ```php
/// if ($app->isTestApp()) {
///     $app->testAppMethod(); // $app is TestApplication here
/// }
/// ```
#[tokio::test]
async fn test_completion_phpstan_assert_if_true_this_narrows_receiver() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_if_true_this.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                              // 0
        "class TestApplication extends Application {\n",                                        // 1
        "    public function testAppMethod(): void {}\n",                                       // 2
        "}\n",                                                                                  // 3
        "class Application {\n",                                                                // 4
        "    /**\n",                                                                            // 5
        "     * @phpstan-assert-if-true \\TestApplication $this\n",                             // 6
        "     */\n",                                                                            // 7
        "    public function isTestApp(): bool { return $this instanceof TestApplication; }\n", // 8
        "}\n",                                                                                  // 9
        "function test(Application $app): void {\n", // 10
        "    if ($app->isTestApp()) {\n",            // 11
        "        $app->\n",                          // 12
        "    }\n",                                   // 13
        "}\n",                                       // 14
    );

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
                position: Position {
                    line: 12,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"testAppMethod"),
                "Should include TestApplication's 'testAppMethod' after @phpstan-assert-if-true $this narrowing, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `@phpstan-assert-if-true` + `@phpstan-assert-if-false` on the same
/// function: each applies in the correct branch.
#[tokio::test]
async fn test_completion_phpstan_assert_if_true_and_false_combined() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///assert_combined.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class User {\n",                                        // 1
        "    public function getName(): string {}\n",            // 2
        "}\n",                                                   // 3
        "class AdminUser {\n",                                   // 4
        "    public function addRoles(string $r): void {}\n",    // 5
        "}\n",                                                   // 6
        "/**\n",                                                 // 7
        " * @phpstan-assert-if-true User $value\n",              // 8
        " * @phpstan-assert-if-false AdminUser $value\n",        // 9
        " */\n",                                                 // 10
        "function isUser($value): bool {}\n",                    // 11
        "class Svc {\n",                                         // 12
        "    public function test(User|AdminUser $v): void {\n", // 13
        "        if (isUser($v)) {\n",                           // 14
        "            $v->\n",                                    // 15
        "        } else {\n",                                    // 16
        "            $v->\n",                                    // 17
        "        }\n",                                           // 18
        "    }\n",                                               // 19
        "}\n",                                                   // 20
    );

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

    // Test then-body: IfTrue → User
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 15,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions for then-body");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Then-body should include User's 'getName' (assert-if-true), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }

    // Test else-body: IfFalse → AdminUser
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 17,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions for else-body");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"addRoles"),
                "Else-body should include AdminUser's 'addRoles' (assert-if-false), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Top-level code (outside class) should also support `@phpstan-assert`.
#[tokio::test]
async fn test_completion_phpstan_assert_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///phpstan_assert_top.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Dog {\n",                                         // 1
        "    public function bark(): void {}\n",                 // 2
        "}\n",                                                   // 3
        "class Cat {\n",                                         // 4
        "    public function purr(): void {}\n",                 // 5
        "}\n",                                                   // 6
        "/**\n",                                                 // 7
        " * @phpstan-assert Dog $val\n",                         // 8
        " */\n",                                                 // 9
        "function assertDog($val): void {}\n",                   // 10
        "\n",                                                    // 11
        "function getAnimal(): Dog|Cat { return new Dog(); }\n", // 12
        "$animal = getAnimal();\n",                              // 13
        "assertDog($animal);\n",                                 // 14
        "$animal->\n",                                           // 15
    );

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
                position: Position {
                    line: 15,
                    character: 10,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's 'bark' at top level after assertDog(), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' at top level after assertDog(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_intersection_parameter_type_shows_all_members() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///intersection_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Loggable {\n",
        "    public function log(): void;\n",
        "}\n",
        "\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function handle(User&Loggable $user): void {\n",
        "        $user->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$user->` on line 11
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for intersection param type User&Loggable"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getName")),
                "Should include getName from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("log")),
                "Should include log from Loggable, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_dnf_type_shows_all_members() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///dnf_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Serializable {\n",
        "    public function serialize(): string;\n",
        "}\n",
        "\n",
        "interface Loggable {\n",
        "    public function log(): void;\n",
        "}\n",
        "\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function handle((Serializable&Loggable)|User $input): void {\n",
        "        $input->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$input->` on line 15
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 15,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for DNF param type (Serializable&Loggable)|User"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("serialize")),
                "Should include serialize from Serializable, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("log")),
                "Should include log from Loggable, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getName")),
                "Should include getName from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_closure_param_type_hint_in_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///closure_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Carrier {\n",
        "    public string $name;\n",
        "    public function ship(): void {}\n",
        "}\n",
        "class Service {\n",
        "    public function process(): void {\n",
        "        $fn = function (Carrier $carrier) {\n",
        "            $carrier->\n",
        "        };\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$carrier->` on line 8
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve $carrier via closure parameter type hint"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("ship")),
                "Should include ship method, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_closure_param_type_hint_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///closure_top.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "    public function cancel(): void {}\n",
        "}\n",
        "$cb = function (Order $order) {\n",
        "    $order->\n",
        "};\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$order->` on line 6
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve $order via top-level closure parameter type hint"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("id")),
                "Should include id property, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("cancel")),
                "Should include cancel method, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_closure_param_as_function_argument() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///closure_arg.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public float $price;\n",
        "    public function discount(): void {}\n",
        "}\n",
        "array_map(function (Item $item) {\n",
        "    $item->\n",
        "}, $items);\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$item->` on line 6
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve $item via closure parameter passed as argument"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("price")),
                "Should include price property, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("discount")),
                "Should include discount method, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_arrow_function_param_type_hint() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///arrow_fn.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $sku;\n",
        "    public function label(): string { return ''; }\n",
        "}\n",
        "$items = array_map(fn(Product $p) => $p->sku, []);\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$p->` on line 5 (inside the arrow function body)
    // `$items = array_map(fn(Product $p) => $p->`
    //  0         1         2         3
    //  0123456789012345678901234567890123456789012
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 42,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve $p via arrow function parameter type hint"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("sku")),
                "Should include sku property, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("label")),
                "Should include label method, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_nested_closure_resolves_inner_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nested_closure.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Outer {\n",
        "    public string $outerProp;\n",
        "}\n",
        "class Inner {\n",
        "    public string $innerProp;\n",
        "    public function doStuff(): void {}\n",
        "}\n",
        "$a = function (Outer $o) {\n",
        "    $b = function (Inner $i) {\n",
        "        $i->\n",
        "    };\n",
        "};\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$i->` on line 10
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve $i via inner closure parameter type hint"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("innerProp")),
                "Should include innerProp from Inner, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("doStuff")),
                "Should include doStuff from Inner, got: {:?}",
                labels
            );
            assert!(
                !labels.iter().any(|l| l.starts_with("outerProp")),
                "Should NOT include outerProp from Outer, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_closure_param_in_method_argument() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///closure_method_arg.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Task {\n",
        "    public string $title;\n",
        "    public function run(): void {}\n",
        "}\n",
        "class Runner {\n",
        "    public function execute(): void {\n",
        "        $this->each(function (Task $task) {\n",
        "            $task->\n",
        "        });\n",
        "    }\n",
        "    public function each(callable $fn): void {}\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$task->` on line 8
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve $task via closure parameter in method argument"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("title")),
                "Should include title property, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("run")),
                "Should include run method, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Foreach generic iterable type resolution ───────────────────────────────

#[tokio::test]
async fn test_completion_foreach_list_generic_var_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_list.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Controller {\n",
        "    public function index() {\n",
        "        /** @var list<User> $users */\n",
        "        $users = getUsers();\n",
        "        foreach ($users as $user) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$user->` on line 10
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $user from list<User>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_foreach_array_bracket_shorthand() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_bracket.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $title;\n",
        "    public function getPrice(): float {}\n",
        "}\n",
        "class Shop {\n",
        "    public function list() {\n",
        "        /** @var Product[] $products */\n",
        "        $products = fetchProducts();\n",
        "        foreach ($products as $product) {\n",
        "            $product->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $product from Product[]"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("title")),
                "Should include title property from Product, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getPrice")),
                "Should include getPrice method from Product, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_foreach_array_generic_two_params() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_array_kv.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "class Service {\n",
        "    public function process() {\n",
        "        /** @var array<int, Order> $orders */\n",
        "        $orders = loadOrders();\n",
        "        foreach ($orders as $key => $order) {\n",
        "            $order->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $order from array<int, Order>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("id")),
                "Should include id property from Order, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getTotal")),
                "Should include getTotal method from Order, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_foreach_param_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Task {\n",
        "    public string $title;\n",
        "    public function run(): void {}\n",
        "}\n",
        "class Runner {\n",
        "    /**\n",
        "     * @param list<Task> $tasks\n",
        "     */\n",
        "    public function execute(array $tasks) {\n",
        "        foreach ($tasks as $task) {\n",
        "            $task->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$task->` on line 11
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $task from @param list<Task>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("title")),
                "Should include title property from Task, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("run")),
                "Should include run method from Task, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_foreach_top_level_list_generic() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public string $label;\n",
        "    public function getValue(): int {}\n",
        "}\n",
        "/** @var list<Item> $items */\n",
        "$items = getItems();\n",
        "foreach ($items as $item) {\n",
        "    $item->\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$item->` on line 8
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $item from top-level list<Item>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("label")),
                "Should include label property from Item, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getValue")),
                "Should include getValue method from Item, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_foreach_cross_file_list_generic() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Models/Customer.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "class Customer {\n",
                "    public string $name;\n",
                "    public function getAddress(): string {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///foreach_cross.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Models\\Customer;\n",
        "/** @var list<Customer> $customers */\n",
        "$customers = loadCustomers();\n",
        "foreach ($customers as $customer) {\n",
        "    $customer->\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$customer->` on line 5
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $customer from cross-file list<Customer>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from Customer, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getAddress")),
                "Should include getAddress method from Customer, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_foreach_array_generic_single_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_array_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Invoice {\n",
        "    public int $number;\n",
        "    public function send(): void {}\n",
        "}\n",
        "class Billing {\n",
        "    public function process() {\n",
        "        /** @var array<Invoice> $invoices */\n",
        "        $invoices = getInvoices();\n",
        "        foreach ($invoices as $invoice) {\n",
        "            $invoice->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $invoice from array<Invoice>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("number")),
                "Should include number property from Invoice, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("send")),
                "Should include send method from Invoice, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_foreach_scalar_list_no_completion() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_scalar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    public function process() {\n",
        "        /** @var list<int> $ids */\n",
        "        $ids = getIds();\n",
        "        foreach ($ids as $id) {\n",
        "            $id->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // list<int> → int is a scalar, so no class completion should be offered.
    // The result might be None or an empty array.
    if let Some(CompletionResponse::Array(items)) = result {
        let method_items: Vec<_> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .collect();
        assert!(
            method_items.is_empty(),
            "Should not offer method completions for scalar element type (int), got: {:?}",
            method_items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }
}

// ─── Array access element type resolution ───────────────────────────────────

#[tokio::test]
async fn test_completion_array_access_list_generic() {
    // `$users[0]->` should suggest User members when $users is `list<User>`
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_access_list.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "function getUnknownValue(): mixed { return null; }\n",
        "/** @var list<User> $users */\n",
        "$users = getUnknownValue();\n",
        "$users[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$users[0]->` on line 8
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $users[0]-> from list<User>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_array_access_array_generic() {
    // `$admins[0]->` should suggest AdminUser members when $admins is `array<int, AdminUser>`
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_access_generic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class AdminUser extends User {\n",
        "    public function grantPermission(string $p): void {}\n",
        "}\n",
        "function getUnknownValue(): mixed { return null; }\n",
        "/** @var array<int, AdminUser> $admins */\n",
        "$admins = getUnknownValue();\n",
        "$admins[0]->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $admins[0]-> from array<int, AdminUser>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("grantPermission")),
                "Should include grantPermission from AdminUser, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail from inherited User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_array_access_bracket_shorthand() {
    // `$members[0]->` should suggest User members when $members is `User[]`
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_access_shorthand.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "function getUnknownValue(): mixed { return null; }\n",
        "/** @var User[] $members */\n",
        "$members = getUnknownValue();\n",
        "$members[0]->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $members[0]-> from User[]"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_array_access_variable_key() {
    // `$admins[$key]->` should work the same as `$admins[0]->`
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_access_varkey.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AdminUser {\n",
        "    public function grantPermission(string $p): void {}\n",
        "}\n",
        "function getUnknownValue(): mixed { return null; }\n",
        "/** @var array<int, AdminUser> $admins */\n",
        "$admins = getUnknownValue();\n",
        "$key = 0;\n",
        "$admins[$key]->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $admins[$key]-> from array<int, AdminUser>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("grantPermission")),
                "Should include grantPermission from AdminUser, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_variable_assigned_from_array_access() {
    // `$admin = $admins[0]; $admin->` should suggest AdminUser members
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_from_array_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class AdminUser extends User {\n",
        "    public function grantPermission(string $p): void {}\n",
        "}\n",
        "function getUnknownValue(): mixed { return null; }\n",
        "/** @var array<int, AdminUser> $admins */\n",
        "$admins = getUnknownValue();\n",
        "$admin = $admins[0];\n",
        "$admin->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $admin assigned from $admins[0]"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("grantPermission")),
                "Should include grantPermission from AdminUser, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail from inherited User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_variable_assigned_from_list_array_access() {
    // `$user = $users[0]; $user->` with `list<User>` annotation
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_from_list_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "function getUnknownValue(): mixed { return null; }\n",
        "/** @var list<User> $users */\n",
        "$users = getUnknownValue();\n",
        "$user = $users[0];\n",
        "$user->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 7,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $user assigned from $users[0]"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_array_access_inside_method() {
    // Array access inside a class method body
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_access_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Controller {\n",
        "    public function index() {\n",
        "        /** @var list<User> $users */\n",
        "        $users = [];\n",
        "        $users[0]->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $users[0]-> inside method"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_variable_assigned_from_array_access_inside_method() {
    // `$admin = $admins[0]; $admin->` inside a method
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_array_access_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AdminUser {\n",
        "    public function grantPermission(string $p): void {}\n",
        "}\n",
        "class Controller {\n",
        "    public function index() {\n",
        "        /** @var list<AdminUser> $admins */\n",
        "        $admins = [];\n",
        "        $admin = $admins[0];\n",
        "        $admin->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $admin from $admins[0] inside method"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("grantPermission")),
                "Should include grantPermission from AdminUser, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_array_access_scalar_no_completion() {
    // `$items[0]->` should NOT suggest anything when $items is `int[]`
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_access_scalar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/** @var int[] $items */\n",
        "$items = [1, 2, 3];\n",
        "$items[0]->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 3,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // Should not get class-member completions for a scalar element type
    if let Some(response) = result {
        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let method_items: Vec<_> = items
            .iter()
            .filter(|i| {
                i.kind == Some(CompletionItemKind::METHOD)
                    || i.kind == Some(CompletionItemKind::PROPERTY)
            })
            .collect();
        assert!(
            method_items.is_empty(),
            "Should not offer method/property completions for scalar element type (int), got: {:?}",
            method_items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }
}

// ─── Foreach key type resolution ────────────────────────────────────────────

/// When iterating over a two-parameter generic with a class key type,
/// the foreach key variable should resolve to that class and offer
/// completions.
///
/// Example: `SplObjectStorage<Request, Response>` → `$key` is `Request`.
#[tokio::test]
async fn test_completion_foreach_key_object_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_key_object.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Request {\n",
        "    public string $method;\n",
        "    public function getUri(): string {}\n",
        "}\n",
        "class Response {\n",
        "    public int $status;\n",
        "}\n",
        "class Handler {\n",
        "    public function process() {\n",
        "        /** @var SplObjectStorage<Request, Response> $storage */\n",
        "        $storage = getStorage();\n",
        "        foreach ($storage as $req => $res) {\n",
        "            $req->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $req from SplObjectStorage<Request, Response>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("method")),
                "Should include method property from Request, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getUri")),
                "Should include getUri method from Request, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When iterating over `array<int, Order>`, the key type is `int` (scalar).
/// No class-member completions should be offered for `$key->`.
#[tokio::test]
async fn test_completion_foreach_key_scalar_no_completions() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_key_scalar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order { public int $id; }\n",
        "/** @var array<int, Order> $orders */\n",
        "$orders = [];\n",
        "foreach ($orders as $key => $order) {\n",
        "    $key->\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 10,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // Should not get class-member completions for a scalar key type (int)
    if let Some(response) = result {
        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let method_items: Vec<_> = items
            .iter()
            .filter(|i| {
                i.kind == Some(CompletionItemKind::METHOD)
                    || i.kind == Some(CompletionItemKind::PROPERTY)
            })
            .collect();
        assert!(
            method_items.is_empty(),
            "Should not offer method/property completions for scalar key type (int), got: {:?}",
            method_items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }
}

/// When iterating over a custom generic collection with class key types,
/// the foreach key variable should resolve correctly.
///
/// Example: `WeakMap<User, Session>` → `$key` is `User`.
#[tokio::test]
async fn test_completion_foreach_key_custom_generic_collection() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_key_custom.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Session {\n",
        "    public string $token;\n",
        "}\n",
        "class Manager {\n",
        "    public function check() {\n",
        "        /** @var WeakMap<User, Session> $sessions */\n",
        "        $sessions = new WeakMap();\n",
        "        foreach ($sessions as $user => $session) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $user from WeakMap<User, Session>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Foreach key type should work with @param annotations on function
/// parameters, not just @var.
#[tokio::test]
async fn test_completion_foreach_key_from_param_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_key_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $sku;\n",
        "    public function getPrice(): float {}\n",
        "}\n",
        "class Category {\n",
        "    public string $label;\n",
        "    public function getSlug(): string {}\n",
        "}\n",
        "class Catalog {\n",
        "    /**\n",
        "     * @param array<Category, Product> $grouped\n",
        "     */\n",
        "    public function display(array $grouped) {\n",
        "        foreach ($grouped as $cat => $product) {\n",
        "            $cat->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 15,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $cat from array<Category, Product>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("label")),
                "Should include label property from Category, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getSlug")),
                "Should include getSlug method from Category, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When iterating with no key variable (`foreach ($items as $val)`),
/// the value type should still resolve correctly — regression guard.
#[tokio::test]
async fn test_completion_foreach_value_still_works_without_key() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_no_key.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item { public string $name; }\n",
        "/** @var list<Item> $items */\n",
        "$items = [];\n",
        "foreach ($items as $item) {\n",
        "    $item->\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $item from list<Item>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from Item, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Array destructuring type resolution ────────────────────────────────────

/// `[$a, $b] = getUsers()` where `getUsers` returns `list<User>` should
/// resolve `$a` to `User`.
#[tokio::test]
async fn test_completion_destructuring_short_syntax_function_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_short_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/**\n",
        " * @return list<User>\n",
        " */\n",
        "function getUsers(): array { return []; }\n",
        "[$first, $second] = getUsers();\n",
        "$first->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `$first->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $first from list<User>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `list($a, $b) = getUsers()` where `getUsers` returns `array<int, User>`
/// should resolve `$a` to `User`.
#[tokio::test]
async fn test_completion_destructuring_list_syntax_function_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_list_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "/**\n",
        " * @return array<int, Order>\n",
        " */\n",
        "function loadOrders(): array { return []; }\n",
        "list($first, $second) = loadOrders();\n",
        "$first->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `$first->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $first from array<int, Order>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("id")),
                "Should include id property from Order, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getTotal")),
                "Should include getTotal method from Order, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `[$a, $b] = $users` where `$users` is annotated as `list<User>` should
/// resolve `$a` to `User`.
#[tokio::test]
async fn test_completion_destructuring_variable_rhs() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Task {\n",
        "    public string $title;\n",
        "    public function run(): void {}\n",
        "}\n",
        "/** @var list<Task> $tasks */\n",
        "$tasks = [];\n",
        "[$first, $second] = $tasks;\n",
        "$first->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 8: `$first->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $first from list<Task>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("title")),
                "Should include title property from Task, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("run")),
                "Should include run method from Task, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `[$a, $b] = $this->getItems()` where `getItems` returns `User[]`
/// should resolve `$a` to `User`.
#[tokio::test]
async fn test_completion_destructuring_method_call_rhs() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $sku;\n",
        "    public function getPrice(): float {}\n",
        "}\n",
        "class Warehouse {\n",
        "    /** @return Product[] */\n",
        "    public function getProducts(): array { return []; }\n",
        "    public function demo() {\n",
        "        [$a, $b] = $this->getProducts();\n",
        "        $a->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `        $a->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $a from Product[]"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("sku")),
                "Should include sku property from Product, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getPrice")),
                "Should include getPrice method from Product, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `[$a, $b] = MyClass::getItems()` where `getItems` returns `list<User>`
/// should resolve `$a` to `User`.
#[tokio::test]
async fn test_completion_destructuring_static_method_call_rhs() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public string $species;\n",
        "    public function speak(): string {}\n",
        "}\n",
        "class Zoo {\n",
        "    /** @return list<Animal> */\n",
        "    public static function getAnimals(): array { return []; }\n",
        "}\n",
        "[$first, $second] = Zoo::getAnimals();\n",
        "$first->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `$first->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $first from list<Animal>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("species")),
                "Should include species property from Animal, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("speak")),
                "Should include speak method from Animal, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Inline `/** @var list<User> */` annotation before a destructuring
/// assignment should resolve the element type.
#[tokio::test]
async fn test_completion_destructuring_inline_var_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_inline_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $email;\n",
        "    public function getBilling(): string {}\n",
        "}\n",
        "/** @var list<Customer> */\n",
        "[$a, $b] = unknownFunction();\n",
        "$a->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 7: `$a->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 4,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $a from inline @var list<Customer>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("email")),
                "Should include email property from Customer, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getBilling")),
                "Should include getBilling method from Customer, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Destructuring with `$this->property` on the RHS should resolve
/// the property's generic type annotation.
#[tokio::test]
async fn test_completion_destructuring_property_rhs() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public string $label;\n",
        "    public function render(): string {}\n",
        "}\n",
        "class Dashboard {\n",
        "    /** @var list<Widget> */\n",
        "    public array $widgets;\n",
        "    public function demo() {\n",
        "        [$first, $second] = $this->widgets;\n",
        "        $first->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `        $first->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $first from list<Widget>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("label")),
                "Should include label property from Widget, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("render")),
                "Should include render method from Widget, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// A variable NOT in the destructured list should not be resolved by
/// the destructuring logic — regression guard.
#[tokio::test]
async fn test_completion_destructuring_unrelated_variable_not_affected() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_unrelated.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Gadget { public string $code; }\n",
        "/**\n",
        " * @return list<Gadget>\n",
        " */\n",
        "function loadGadgets(): array { return []; }\n",
        "[$a, $b] = loadGadgets();\n",
        "$unrelated->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 7: `$unrelated->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // $unrelated is not in the destructured list, so it should NOT
    // get Gadget completions.
    if let Some(response) = result {
        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let gadget_items: Vec<_> = items
            .iter()
            .filter(|i| i.label.starts_with("code"))
            .collect();
        assert!(
            gadget_items.is_empty(),
            "$unrelated should NOT get Gadget completions, got: {:?}",
            gadget_items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }
}

/// The second variable in a destructuring should also resolve.
/// `[$first, $second] = getUsers()` → `$second` is also `User`.
#[tokio::test]
async fn test_completion_destructuring_second_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_second.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Fruit {\n",
        "    public string $color;\n",
        "    public function peel(): void {}\n",
        "}\n",
        "/**\n",
        " * @return list<Fruit>\n",
        " */\n",
        "function getFruits(): array { return []; }\n",
        "[$first, $second] = getFruits();\n",
        "$second->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `$second->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $second from list<Fruit>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("color")),
                "Should include color property from Fruit, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("peel")),
                "Should include peel method from Fruit, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Destructuring inside a class method should work with `@param` annotation.
#[tokio::test]
async fn test_completion_destructuring_from_param_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Ticket {\n",
        "    public string $seat;\n",
        "    public function validate(): bool {}\n",
        "}\n",
        "class Booking {\n",
        "    /**\n",
        "     * @param list<Ticket> $tickets\n",
        "     */\n",
        "    public function process(array $tickets) {\n",
        "        [$first, $second] = $tickets;\n",
        "        $first->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 11: `        $first->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $first from list<Ticket>"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("seat")),
                "Should include seat property from Ticket, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("validate")),
                "Should include validate method from Ticket, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_catch_variable_single_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///catch_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ValidationException {\n",
        "    public function getErrors(): array { return []; }\n",
        "    public function getField(): string { return ''; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        try {\n",
        "            // something\n",
        "        } catch (ValidationException $e) {\n",
        "            $e->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for catch variable"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getErrors")),
                "Should include getErrors from ValidationException, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getField")),
                "Should include getField from ValidationException, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_catch_variable_multi_catch_union() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///catch_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class HttpException {\n",
        "    public function getStatusCode(): int { return 500; }\n",
        "}\n",
        "class TimeoutException {\n",
        "    public function getTimeout(): int { return 30; }\n",
        "}\n",
        "class Handler {\n",
        "    public function handle(): void {\n",
        "        try {\n",
        "            // something\n",
        "        } catch (HttpException | TimeoutException $e) {\n",
        "            $e->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for multi-catch variable"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getStatusCode")),
                "Should include getStatusCode from HttpException, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getTimeout")),
                "Should include getTimeout from TimeoutException, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_catch_variable_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/DatabaseException.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "class DatabaseException {\n",
                "    public function getQuery(): string { return ''; }\n",
                "    public function getErrorCode(): int { return 0; }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///catch_cross.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Handler {\n",
        "    public function run(): void {\n",
        "        try {\n",
        "            // something\n",
        "        } catch (DatabaseException $e) {\n",
        "            $e->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for cross-file catch variable"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getQuery")),
                "Should include getQuery from DatabaseException, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getErrorCode")),
                "Should include getErrorCode from DatabaseException, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file catch variable where the exception class lives in a different
/// namespace, extends the global `Exception` via a `use Exception;` import,
/// and has NO own methods.  Inherited methods like `getMessage()` must still
/// resolve through the parent chain even though the `class_loader` closure
/// carries the *consumer* file's use_map (which does not import `Exception`).
#[tokio::test]
async fn test_completion_catch_variable_cross_file_global_parent_via_use() {
    let (backend, _dir) = create_psr4_workspace_with_exception_stubs(
        r#"{ "autoload": { "psr-4": { "Vendor\\Exceptions\\": "src/Exceptions/", "App\\Console\\": "src/Console/" } } }"#,
        &[(
            "src/Exceptions/AppException.php",
            concat!(
                "<?php\n",
                "namespace Vendor\\Exceptions;\n",
                "\n",
                "use Exception;\n",
                "\n",
                "abstract class AppException extends Exception\n",
                "{\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///catch_cross_global.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Console;\n",
        "\n",
        "use Vendor\\Exceptions\\AppException;\n",
        "\n",
        "class SyncCommand\n",
        "{\n",
        "    public function handle(): void\n",
        "    {\n",
        "        try {\n",
        "        } catch (AppException $e) {\n",
        "            $e->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for cross-file catch variable with global parent"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getMessage")),
                "Should include getMessage inherited from Exception, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getCode")),
                "Should include getCode inherited from Exception, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_catch_variable_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///catch_top.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class RuntimeError {\n",
        "    public function getMessage(): string { return ''; }\n",
        "}\n",
        "try {\n",
        "    // something\n",
        "} catch (RuntimeError $err) {\n",
        "    $err->\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 10,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for top-level catch variable"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getMessage")),
                "Should include getMessage from RuntimeError, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_catch_variable_no_own_methods_no_namespace() {
    // Same as the namespace test but WITHOUT a namespace — verifies
    // whether the bug is namespace-specific or inheritance-specific.
    let backend = create_test_backend_with_exception_stubs();

    let uri = Url::parse("file:///catch_bare_no_ns.php").unwrap();
    let text = concat!(
        "<?php\n",
        "\n",
        "class ValidationException extends \\RuntimeException {}\n",
        "\n",
        "class CatchVariableDemo\n",
        "{\n",
        "    public function singleCatch(): void\n",
        "    {\n",
        "        try {\n",
        "            $this->riskyOperation();\n",
        "        } catch (ValidationException $e) {\n",
        "            $e->\n",
        "        }\n",
        "    }\n",
        "    private function riskyOperation(): void {}\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$e->` on line 11
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for catch variable with no own methods (no namespace)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

            assert!(
                labels.iter().any(|l| l.starts_with("getMessage")),
                "Should include getMessage from RuntimeException (no namespace), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_catch_variable_namespace_no_own_methods() {
    // Reproduces: namespace + exception class with NO own methods (only
    // inherits from \RuntimeException) → catch variable should still
    // resolve via inheritance.
    let backend = create_test_backend_with_exception_stubs();

    let uri = Url::parse("file:///catch_ns_bare.php").unwrap();
    let text = concat!(
        "<?php\n",
        "\n",
        "namespace Demo;\n",
        "\n",
        "class ValidationException extends \\RuntimeException {}\n",
        "\n",
        "class CatchVariableDemo\n",
        "{\n",
        "    public function singleCatch(): void\n",
        "    {\n",
        "        try {\n",
        "            $this->riskyOperation();\n",
        "        } catch (ValidationException $e) {\n",
        "            $e->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$e->` on line 13
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for catch variable in namespace with no own methods"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            // ValidationException extends \RuntimeException which has
            // getMessage(), getCode(), etc. — these should appear via
            // inheritance even though the class has no own members.
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

            assert!(
                labels.iter().any(|l| l.starts_with("getMessage")),
                "Should include getMessage from RuntimeException via inheritance, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getCode")),
                "Should include getCode from RuntimeException via inheritance, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_catch_variable_with_namespace() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///catch_ns.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "class ValidationException extends \\RuntimeException {\n",
        "    public function getErrors(): array { return []; }\n",
        "}\n",
        "class CatchVariableDemo {\n",
        "    public function singleCatch(): void {\n",
        "        try {\n",
        "            $this->riskyOperation();\n",
        "        } catch (ValidationException $e) {\n",
        "            $e->\n",
        "        }\n",
        "    }\n",
        "    private function riskyOperation(): void {}\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for catch variable in namespaced file"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getErrors")),
                "Should include getErrors from ValidationException in namespaced file, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Clone expression type preservation ─────────────────────────────────────

#[tokio::test]
async fn test_completion_clone_new_instance() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///clone_new.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): string { return ''; }\n",
        "    public function resize(int $w): void {}\n",
        "}\n",
        "class CloneNewDemo {\n",
        "    public function demo(): void {\n",
        "        $original = new Widget();\n",
        "        $copy = clone $original;\n",
        "        $copy->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for clone of new instance"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"render"),
                "Should include 'render' from Widget via clone, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"resize"),
                "Should include 'resize' from Widget via clone, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_clone_parameter() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///clone_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Invoice {\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "    public function addItem(string $name): void {}\n",
        "}\n",
        "class CloneParamDemo {\n",
        "    public function duplicate(Invoice $invoice): void {\n",
        "        $copy = clone $invoice;\n",
        "        $copy->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for clone of parameter"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getTotal"),
                "Should include 'getTotal' from Invoice via clone of param, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"addItem"),
                "Should include 'addItem' from Invoice via clone of param, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_clone_this() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///clone_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Immutable {\n",
        "    public function withValue(int $v): self {\n",
        "        $clone = clone $this;\n",
        "        $clone->\n",
        "    }\n",
        "    public function getValue(): int { return 0; }\n",
        "    public function toArray(): array { return []; }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 4,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for clone $this"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getValue"),
                "Should include 'getValue' from Immutable via clone $this, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"toArray"),
                "Should include 'toArray' from Immutable via clone $this, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_clone_method_return() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///clone_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    public function get(string $key): string { return ''; }\n",
        "    public function set(string $key, string $val): void {}\n",
        "}\n",
        "class App {\n",
        "    public function getConfig(): Config { return new Config(); }\n",
        "    public function demo(): void {\n",
        "        $cfg = clone $this->getConfig();\n",
        "        $cfg->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for clone of method return"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"get"),
                "Should include 'get' from Config via clone of method return, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"set"),
                "Should include 'set' from Config via clone of method return, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_clone_in_ternary() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///clone_ternary.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Settings {\n",
        "    public function save(): void {}\n",
        "    public function reset(): void {}\n",
        "}\n",
        "class TernaryCloneDemo {\n",
        "    public function demo(Settings $s, bool $flag): void {\n",
        "        $result = $flag ? clone $s : new Settings();\n",
        "        $result->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for clone in ternary"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"save"),
                "Should include 'save' from Settings via clone in ternary, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"reset"),
                "Should include 'reset' from Settings via clone in ternary, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_clone_cross_file() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let entity_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Entity {\n",
        "    public function getId(): int { return 0; }\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
    );

    let service_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Entity;\n",
        "class Service {\n",
        "    public function snapshot(Entity $entity): void {\n",
        "        $snapshot = clone $entity;\n",
        "        $snapshot->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("src/Entity.php", entity_php),
            ("src/Service.php", service_php),
        ],
    );

    let uri = Url::parse("file:///src/Service.php").unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: service_php.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for clone in cross-file scenario"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getId"),
                "Should include 'getId' from Entity via clone cross-file, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getName"),
                "Should include 'getName' from Entity via clone cross-file, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `(clone $date)->` should resolve to the same type as `$date` without
/// needing an intermediate variable assignment.
#[tokio::test]
async fn test_completion_clone_inline_parenthesized() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///clone_inline.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class DateRange {\n",
        "    public function endOfMonth(): self { return $this; }\n",
        "    public function format(string $f): string { return ''; }\n",
        "}\n",
        "class InlineCloneDemo {\n",
        "    public function demo(DateRange $date): void {\n",
        "        $lastDay = (clone $date)->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 37,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for inline (clone $date)->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"endOfMonth"),
                "Should include 'endOfMonth' from DateRange via inline clone, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"format"),
                "Should include 'format' from DateRange via inline clone, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Foreach over method calls / property access / static calls ─────────────

/// `foreach ($this->getUsers() as $user)` should resolve `$user` to `User`.
#[tokio::test]
async fn test_completion_foreach_method_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class UserService {\n",
        "    /** @return list<User> */\n",
        "    public function getUsers(): array { return []; }\n",
        "    public function process() {\n",
        "        foreach ($this->getUsers() as $user) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach over $this->getUsers()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include 'name' property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include 'getEmail' method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `foreach ($this->users as $user)` should resolve `$user` via property type.
#[tokio::test]
async fn test_completion_foreach_property_access() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class UserService {\n",
        "    /** @var list<User> */\n",
        "    public array $users;\n",
        "    public function process() {\n",
        "        foreach ($this->users as $user) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach over $this->users"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include 'name' property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include 'getEmail' method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `foreach (UserService::getAll() as $user)` should resolve `$user`.
#[tokio::test]
async fn test_completion_foreach_static_method_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class UserService {\n",
        "    /** @return list<User> */\n",
        "    public static function getAll(): array { return []; }\n",
        "}\n",
        "class Controller {\n",
        "    public function index() {\n",
        "        foreach (UserService::getAll() as $user) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach over UserService::getAll()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include 'name' property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include 'getEmail' method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `foreach (self::getAll() as $user)` should resolve via self.
#[tokio::test]
async fn test_completion_foreach_self_static_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_self.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n",
        "class UserRepo {\n",
        "    /** @return list<User> */\n",
        "    public static function getAll(): array { return []; }\n",
        "    public function process() {\n",
        "        foreach (self::getAll() as $user) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach over self::getAll()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include 'name' property from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `foreach ($this->getUsers() as $key => $user)` — key type should also work.
#[tokio::test]
async fn test_completion_foreach_method_call_key_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_method_key.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Request {\n",
        "    public string $url;\n",
        "}\n",
        "class Response {\n",
        "    public int $status;\n",
        "}\n",
        "class Service {\n",
        "    /** @return array<Request, Response> */\n",
        "    public function getMapping(): array { return []; }\n",
        "    public function process() {\n",
        "        foreach ($this->getMapping() as $req => $res) {\n",
        "            $req->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach key from method call"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("url")),
                "Should include 'url' property from Request (key type), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `foreach ($var->getItems() as $item)` — method call on non-$this variable.
#[tokio::test]
async fn test_completion_foreach_method_call_on_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_var_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public string $label;\n",
        "}\n",
        "class ItemRepo {\n",
        "    /** @return list<Item> */\n",
        "    public function getItems(): array { return []; }\n",
        "}\n",
        "class Controller {\n",
        "    public function index() {\n",
        "        $repo = new ItemRepo();\n",
        "        foreach ($repo->getItems() as $item) {\n",
        "            $item->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach over $repo->getItems()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("label")),
                "Should include 'label' property from Item, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file: `foreach ($this->getUsers() as $user)` where User is in another file.
#[tokio::test]
async fn test_completion_foreach_method_call_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Models/User.php",
                "<?php\nnamespace App\\Models;\nclass User {\n    public string $name;\n    public function getEmail(): string {}\n}\n",
            ),
            (
                "src/Services/UserService.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Services;\n",
                    "use App\\Models\\User;\n",
                    "class UserService {\n",
                    "    /** @return list<User> */\n",
                    "    public function getUsers(): array { return []; }\n",
                    "    public function process() {\n",
                    "        foreach ($this->getUsers() as $user) {\n",
                    "            $user->\n",
                    "        }\n",
                    "    }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let service_path = _dir.path().join("src/Services/UserService.php");
    let uri = Url::from_file_path(&service_path).unwrap();
    let text = std::fs::read_to_string(&service_path).unwrap();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach over method call (cross-file)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include 'name' from User via cross-file foreach method call, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include 'getEmail' from User via cross-file foreach method call, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Foreach over property access on non-$this variable: `foreach ($repo->items as $item)`.
#[tokio::test]
async fn test_completion_foreach_property_access_on_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_var_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public string $label;\n",
        "}\n",
        "class ItemRepo {\n",
        "    /** @var list<Item> */\n",
        "    public array $items;\n",
        "}\n",
        "class Controller {\n",
        "    public function index() {\n",
        "        $repo = new ItemRepo();\n",
        "        foreach ($repo->items as $item) {\n",
        "            $item->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach over $repo->items"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("label")),
                "Should include 'label' property from Item, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Foreach with array type shorthand in method return: `@return User[]`.
#[tokio::test]
async fn test_completion_foreach_method_call_array_shorthand() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_shorthand.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n",
        "class UserService {\n",
        "    /** @return User[] */\n",
        "    public function getUsers(): array { return []; }\n",
        "    public function process() {\n",
        "        foreach ($this->getUsers() as $user) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach over method returning User[]"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include 'name' property from User via User[] return type, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Chained method calls in variable assignment RHS ────────────────────────

#[tokio::test]
async fn test_chained_method_call_this_method_chain() {
    // $this->getRepository()->find(1) — method chain on $this
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Repository {\n",
        "    public function find(int $id): User { return new User(); }\n",
        "}\n",
        "\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function getRepository(): Repository { return new Repository(); }\n",
        "\n",
        "    public function doStuff(): void {\n",
        "        $user = $this->getRepository()->find(1);\n",
        "        $user->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$user->` on line 15
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 15,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for chained $this->method()->method() in assignment RHS"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getName")),
                "Should include getName from User via chained call, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail from User via chained call, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_chained_method_call_function_then_method() {
    // getFactory()->createWidget() — standalone function returning a class,
    // then a method call on the result.
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): string { return ''; }\n",
        "    public function getSize(): int { return 0; }\n",
        "}\n",
        "\n",
        "class Factory {\n",
        "    public function createWidget(): Widget { return new Widget(); }\n",
        "}\n",
        "\n",
        "/** @return Factory */\n",
        "function getFactory(): Factory { return new Factory(); }\n",
        "\n",
        "$widget = getFactory()->createWidget();\n",
        "$widget->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$widget->` on line 14
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 14,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for function()->method() chain in assignment RHS"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("render")),
                "Should include render from Widget via function()->method() chain, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_chained_method_call_variable_method_chain() {
    // $repo->findOne()->toModel() — variable (not $this) resolved via
    // prior assignment, then chained methods.
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Model {\n",
        "    public function save(): bool { return true; }\n",
        "    public function toArray(): array { return []; }\n",
        "}\n",
        "\n",
        "class QueryResult {\n",
        "    public function toModel(): Model { return new Model(); }\n",
        "}\n",
        "\n",
        "class Repo {\n",
        "    public function findOne(): QueryResult { return new QueryResult(); }\n",
        "}\n",
        "\n",
        "class Controller {\n",
        "    public function action(): void {\n",
        "        $repo = new Repo();\n",
        "        $model = $repo->findOne()->toModel();\n",
        "        $model->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$model->` on line 18
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 18,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for $var->method()->method() chain in assignment RHS"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("save")),
                "Should include save from Model via $var->method()->method() chain, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("toArray")),
                "Should include toArray from Model via $var->method()->method() chain, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_chained_method_call_static_then_method() {
    // Factory::create()->process() — static method chain
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Processor {\n",
        "    public function getOutput(): string { return ''; }\n",
        "}\n",
        "\n",
        "class Builder {\n",
        "    public function process(): Processor { return new Processor(); }\n",
        "}\n",
        "\n",
        "class Factory {\n",
        "    public static function create(): Builder { return new Builder(); }\n",
        "}\n",
        "\n",
        "$result = Factory::create()->process();\n",
        "$result->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$result->` on line 14
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 14,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for Static::method()->method() chain in assignment RHS"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getOutput")),
                "Should include getOutput from Processor via static chain, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_chained_method_call_triple_chain() {
    // $this->a()->b()->c() — three-deep chain
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_triple.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function b(): B { return new B(); }\n",
        "}\n",
        "class B {\n",
        "    public function c(): C { return new C(); }\n",
        "}\n",
        "class C {\n",
        "    public function doSomething(): void {}\n",
        "    public function getValue(): int { return 0; }\n",
        "}\n",
        "\n",
        "class Entry {\n",
        "    public function a(): A { return new A(); }\n",
        "    public function run(): void {\n",
        "        $val = $this->a()->b()->c();\n",
        "        $val->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$val->` on line 16
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 16,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for triple-deep method chain in assignment RHS"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("doSomething")),
                "Should include doSomething from C via triple chain, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getValue")),
                "Should include getValue from C via triple chain, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_chained_method_call_new_then_method() {
    // (new Builder())->build() — parenthesized new then method chain
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_new.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function ship(): void {}\n",
        "    public function getPrice(): float { return 0.0; }\n",
        "}\n",
        "\n",
        "class Builder {\n",
        "    public function build(): Product { return new Product(); }\n",
        "}\n",
        "\n",
        "$result = (new Builder())->build();\n",
        "$result->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$result->` on line 11
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for (new Class())->method() chain in assignment RHS"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("ship")),
                "Should include ship from Product via (new Builder())->build(), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getPrice")),
                "Should include getPrice from Product via (new Builder())->build(), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_chained_method_call_new_then_method_inside_class() {
    // (new Builder())->build() inside a class method
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_new_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function ship(): void {}\n",
        "    public function getPrice(): float { return 0.0; }\n",
        "}\n",
        "\n",
        "class Builder {\n",
        "    public function build(): Product { return new Product(); }\n",
        "}\n",
        "\n",
        "class Store {\n",
        "    public function run(): void {\n",
        "        $result = (new Builder())->build();\n",
        "        $result->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$result->` on line 13
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for (new Class())->method() inside class method"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("ship")),
                "Should include ship from Product via (new Builder())->build() in class, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_chained_method_call_new_no_parens_then_method() {
    // new Builder()->build() — PHP 8.x style without outer parens
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_new_no_parens.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function ship(): void {}\n",
        "}\n",
        "\n",
        "class Builder {\n",
        "    public function build(): Product { return new Product(); }\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $result = new Builder()->build();\n",
        "        $result->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Note: `new Foo()->bar()` is parsed by PHP 8.4+ as `new (Foo()->bar())`.
    // In PHP < 8.4, it's `(new Foo())->bar()`.
    // The mago parser may interpret this differently; this test verifies
    // we handle whichever AST we get.
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // This may or may not resolve depending on how the parser interprets it.
    // We mainly want to ensure no panic / crash.
    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        // If it resolves, it should show Product members
        if !labels.is_empty() {
            assert!(
                labels.iter().any(|l| l.starts_with("ship")),
                "If resolved, should include ship from Product, got: {:?}",
                labels
            );
        }
    }
}

#[tokio::test]
async fn test_chained_method_call_extract_raw_type_chain() {
    // Verifies that extract_raw_type_from_assignment_text handles chained
    // calls when the result is used with array access:
    // $items = $this->getRepo()->findAll();  // returns array<int, User>
    // $items[0]->
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_array_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "\n",
        "class Repo {\n",
        "    /** @return User[] */\n",
        "    public function findAll(): array { return []; }\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function getRepo(): Repo { return new Repo(); }\n",
        "\n",
        "    public function run(): void {\n",
        "        $items = $this->getRepo()->findAll();\n",
        "        $items[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$items[0]->` on line 15
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 15,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for array element after chained call assignment"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getName")),
                "Should include getName from User via $this->getRepo()->findAll()[0]->, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Array element access from assignment (no docblock on variable) ─────────

/// When `$attributes` is assigned from a multi-line `(new Foo())->method()`
/// chain, `$attributes[0]->` should still resolve the element type.
/// The text path must trim whitespace from the LHS when splitting at `->`.
#[tokio::test]
async fn test_array_access_multiline_new_expression_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///multiline_array_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class UseFactory {\n",
        "    public function newInstance(): self { return new self(); }\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "\n",
        "class MyReflectionClass {\n",
        "    /** @return UseFactory[] */\n",
        "    public function getAttributes(): array { return []; }\n",
        "}\n",
        "\n",
        "class Handler {\n",
        "    public function run(): void {\n",
        "        $attributes = (new MyReflectionClass())\n",
        "            ->getAttributes();\n",
        "\n",
        "        $attributes[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$attributes[0]->` on line 16
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 16,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for array element from multi-line new expression chain"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("newInstance")),
                "Should include newInstance from UseFactory via multi-line chain, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getName")),
                "Should include getName from UseFactory via multi-line chain, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `$this->repo->findAll()` returns `Product[]`, indexing into
/// the result with `$items[0]->` should resolve the element type.
/// This exercises the text path through `resolve_raw_type_from_call_chain`
/// with a property-based LHS (`$this->repo`).
#[tokio::test]
async fn test_array_access_property_method_call_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///prop_array_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function getTitle(): string { return ''; }\n",
        "    public function getPrice(): float { return 0.0; }\n",
        "}\n",
        "\n",
        "class ProductRepo {\n",
        "    /** @return Product[] */\n",
        "    public function findAll(): array { return []; }\n",
        "}\n",
        "\n",
        "class ProductService {\n",
        "    public ProductRepo $repo;\n",
        "\n",
        "    public function doStuff(): void {\n",
        "        $items = $this->repo->findAll();\n",
        "        $items[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$items[0]->` on line 16
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 16,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for $items[0]-> after $this->repo->findAll()"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getTitle")),
                "Should include getTitle from Product, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getPrice")),
                "Should include getPrice from Product, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// AST path: when `$first = $items[0]` and `$items` was assigned from a
/// method returning `User[]`, `$first->` should resolve to User.
/// This exercises the `resolve_rhs_array_access` fallback.
#[tokio::test]
async fn test_array_access_intermediate_assignment_ast_path() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///ast_array_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function getId(): int { return 0; }\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "}\n",
        "\n",
        "class OrderService {\n",
        "    /** @return Order[] */\n",
        "    public function getOrders(): array { return []; }\n",
        "\n",
        "    public function process(): void {\n",
        "        $orders = $this->getOrders();\n",
        "        $first = $orders[0];\n",
        "        $first->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$first->` on line 13
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for $first after $first = $orders[0]"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getId")),
                "Should include getId from Order via intermediate $first = $orders[0], got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getTotal")),
                "Should include getTotal from Order via intermediate $first = $orders[0], got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// AST path with generic syntax: `@return array<int, User>` should also
/// work through the intermediate assignment fallback.
#[tokio::test]
async fn test_array_access_intermediate_assignment_generic_syntax() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///ast_array_access_generic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Task {\n",
        "    public function run(): void {}\n",
        "    public function getStatus(): string { return ''; }\n",
        "}\n",
        "\n",
        "class TaskQueue {\n",
        "    /** @return array<int, Task> */\n",
        "    public function pending(): array { return []; }\n",
        "\n",
        "    public function processNext(): void {\n",
        "        $tasks = $this->pending();\n",
        "        $task = $tasks[0];\n",
        "        $task->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$task->` on line 13
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for $task after $task = $tasks[0] with generic return type"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("run")),
                "Should include run from Task via array<int, Task>[0], got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getStatus")),
                "Should include getStatus from Task via array<int, Task>[0], got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Named key destructuring from array shapes ─────────────────────────────

/// `['user' => $person] = $data` where `$data` is `array{user: User, active: bool}`
/// should resolve `$person` to `User`.
#[tokio::test]
async fn test_completion_destructuring_named_key_from_array_shape_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_shape_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var array{user: User, active: bool} $data */\n",
        "$data = loadRecord();\n",
        "['user' => $person, 'active' => $flag] = $data;\n",
        "$person->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 8: `$person->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $person from array shape"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Named key destructuring from function return type annotated as array shape.
/// `['order' => $ord] = getRecord()` where `getRecord` returns `array{order: Order, total: float}`.
#[tokio::test]
async fn test_completion_destructuring_named_key_from_function_return_shape() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_shape_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "/**\n",
        " * @return array{order: Order, total: float}\n",
        " */\n",
        "function getRecord(): array { return []; }\n",
        "['order' => $ord] = getRecord();\n",
        "$ord->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `$ord->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $ord from array shape return type"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("id")),
                "Should include id property from Order, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getTotal")),
                "Should include getTotal method from Order, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Positional destructuring from array shape with numeric keys.
/// `[$first, $second] = $data` where `$data` is `array{User, Address}`
/// should resolve `$first` to `User` and `$second` to `Address`.
#[tokio::test]
async fn test_completion_destructuring_positional_from_array_shape() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_shape_pos.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n",
        "class Address {\n",
        "    public string $city;\n",
        "}\n",
        "/** @var array{User, Address} $data */\n",
        "$data = getStuff();\n",
        "[$first, $second] = $data;\n",
        "$second->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `$second->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $second from positional array shape"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("city")),
                "Should include city property from Address (2nd positional entry), got: {:?}",
                labels
            );
            // Should NOT include User members since $second is the 2nd element.
            assert!(
                !labels.iter().any(|l| l.starts_with("name")),
                "Should NOT include name from User (1st element) on $second, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Explicit numeric key destructuring: `[0 => $a, 1 => $b] = $data`
/// where `$data` is `array{0: User, 1: Address}`.
#[tokio::test]
async fn test_completion_destructuring_numeric_key_from_array_shape() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_shape_numkey.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $sku;\n",
        "}\n",
        "class Category {\n",
        "    public string $label;\n",
        "}\n",
        "/** @var array{0: Product, 1: Category} $pair */\n",
        "$pair = getPair();\n",
        "[0 => $item, 1 => $cat] = $pair;\n",
        "$item->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `$item->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 7,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $item from numeric key array shape"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("sku")),
                "Should include sku property from Product, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Named key destructuring inside a class method, with the RHS coming
/// from a method call returning an array shape.
#[tokio::test]
async fn test_completion_destructuring_named_key_method_return_shape() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_shape_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $email;\n",
        "    public function getBilling(): string {}\n",
        "}\n",
        "class Service {\n",
        "    /**\n",
        "     * @return array{customer: Customer, total: float}\n",
        "     */\n",
        "    public function getInvoice(): array { return []; }\n",
        "    public function process(): void {\n",
        "        ['customer' => $cust] = $this->getInvoice();\n",
        "        $cust->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 12: `$cust->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $cust from method returning array shape"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("email")),
                "Should include email property from Customer, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getBilling")),
                "Should include getBilling method from Customer, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Inline `/** @var array{name: string, user: User} */` annotation before
/// a named destructuring assignment should resolve the keyed entry type.
#[tokio::test]
async fn test_completion_destructuring_named_key_inline_var_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_shape_inline.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public string $label;\n",
        "    public function render(): string {}\n",
        "}\n",
        "/** @var array{widget: Widget, count: int} */\n",
        "['widget' => $w] = getData();\n",
        "$w->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 7: `$w->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 4,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $w from inline var array shape"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("label")),
                "Should include label property from Widget, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("render")),
                "Should include render method from Widget, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Double-quoted key destructuring: `["user" => $u] = $data` should work
/// the same as single-quoted keys.
#[tokio::test]
async fn test_completion_destructuring_double_quoted_key_from_array_shape() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_shape_dblquote.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public string $species;\n",
        "}\n",
        "/** @var array{animal: Animal, count: int} $zoo */\n",
        "$zoo = getZoo();\n",
        "[\"animal\" => $a] = $zoo;\n",
        "$a->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 7: `$a->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 4,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $a from double-quoted key destructuring"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("species")),
                "Should include species property from Animal, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Generic list destructuring should still work when array shape lookup
/// does not match — regression guard.
#[tokio::test]
async fn test_completion_destructuring_generic_list_still_works() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_generic_guard.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Gadget {\n",
        "    public string $code;\n",
        "}\n",
        "/**\n",
        " * @return list<Gadget>\n",
        " */\n",
        "function loadGadgets(): array { return []; }\n",
        "[$g1, $g2] = loadGadgets();\n",
        "$g1->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 9: `$g1->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 5,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should still return results for generic list destructuring"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("code")),
                "Should include code property from Gadget, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `list()` syntax with named keys: `list('user' => $u) = $data`.
#[tokio::test]
async fn test_completion_destructuring_list_syntax_named_key_shape() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_shape_list.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Ticket {\n",
        "    public string $seat;\n",
        "    public function validate(): bool {}\n",
        "}\n",
        "/** @var array{ticket: Ticket, price: float} $booking */\n",
        "$booking = getBooking();\n",
        "list('ticket' => $t) = $booking;\n",
        "$t->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 8: `$t->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 4,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $t from list() named key destructuring"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("seat")),
                "Should include seat property from Ticket, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("validate")),
                "Should include validate method from Ticket, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Array function type preservation ───────────────────────────────────────

/// `$active = array_filter($users, ...)` where `$users` is `list<User>`
/// should preserve the element type so that `$active[0]->` resolves to User.
#[tokio::test]
async fn test_completion_array_filter_preserves_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_filter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "$active = array_filter($users, fn(User $u) => true);\n",
        "$active[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 8: `$active[0]->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for array_filter result element"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$vals = array_values($users)` where `$users` is `list<User>`
/// should preserve the element type.
#[tokio::test]
async fn test_completion_array_values_preserves_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_values.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $sku;\n",
        "    public function getPrice(): float {}\n",
        "}\n",
        "/** @var array<int, Product> $products */\n",
        "$products = [];\n",
        "$vals = array_values($products);\n",
        "$vals[0]->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 10,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for array_values result element"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("sku")),
                "Should include sku property from Product, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getPrice")),
                "Should include getPrice method from Product, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$unique = array_unique($items)` preserves element type.
#[tokio::test]
async fn test_completion_array_unique_preserves_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_unique.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Tag {\n",
        "    public string $label;\n",
        "}\n",
        "/** @var list<Tag> $tags */\n",
        "$tags = [];\n",
        "$unique = array_unique($tags);\n",
        "$unique[0]->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for array_unique result element"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("label")),
                "Should include label property from Tag, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$reversed = array_reverse($items)` preserves element type.
#[tokio::test]
async fn test_completion_array_reverse_preserves_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_reverse.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Step {\n",
        "    public int $order;\n",
        "}\n",
        "/** @var list<Step> $steps */\n",
        "$steps = [];\n",
        "$reversed = array_reverse($steps);\n",
        "$reversed[0]->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for array_reverse result element"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("order")),
                "Should include order property from Step, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$item = array_pop($users)` where `$users` is `list<User>` should
/// resolve `$item` to `User`.
#[tokio::test]
async fn test_completion_array_pop_extracts_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_pop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "$item = array_pop($users);\n",
        "$item->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 8: `$item->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 7,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for array_pop element"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$item = array_shift($items)` extracts the element type.
#[tokio::test]
async fn test_completion_array_shift_extracts_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_shift.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "/** @var list<Order> $orders */\n",
        "$orders = [];\n",
        "$first = array_shift($orders);\n",
        "$first->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for array_shift element"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("id")),
                "Should include id property from Order, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getTotal")),
                "Should include getTotal method from Order, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$cur = current($items)` extracts the element type.
#[tokio::test]
async fn test_completion_current_extracts_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///current.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public string $label;\n",
        "}\n",
        "/** @var list<Widget> $widgets */\n",
        "$widgets = [];\n",
        "$cur = current($widgets);\n",
        "$cur->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for current() element"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("label")),
                "Should include label property from Widget, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `foreach (array_filter($users, ...) as $u)` should resolve `$u` to User.
#[tokio::test]
async fn test_completion_foreach_over_array_filter() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_filter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "foreach (array_filter($users, fn(User $u) => true) as $active) {\n",
        "    $active->\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 8: `    $active->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach over array_filter"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$names = array_map(fn(User $u): string => $u->name, $users)`
/// — when the callback has a return type hint that is a class, resolve it.
/// Here the return type is `string` (scalar) so we fall back to the input
/// array's element type.
#[tokio::test]
async fn test_completion_array_map_fallback_to_input_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_map_fallback.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "$mapped = array_map(fn($u) => $u, $users);\n",
        "$mapped[0]->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for array_map fallback element"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Destructuring from array_values should preserve element type:
/// `[$a] = array_values($users)` → `$a` resolves to User.
#[tokio::test]
async fn test_completion_destructuring_from_array_values() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///destruct_array_values.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $email;\n",
        "    public function getBilling(): string {}\n",
        "}\n",
        "/** @var list<Customer> $customers */\n",
        "$customers = [];\n",
        "[$first] = array_values($customers);\n",
        "$first->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 8: `$first->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for destructuring from array_values"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("email")),
                "Should include email property from Customer, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getBilling")),
                "Should include getBilling method from Customer, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$item = end($users)` where `$users` is `User[]` shorthand syntax.
#[tokio::test]
async fn test_completion_end_extracts_element_type_bracket_shorthand() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///end_bracket.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Task {\n",
        "    public string $title;\n",
        "    public function run(): void {}\n",
        "}\n",
        "/** @var Task[] $tasks */\n",
        "$tasks = [];\n",
        "$last = end($tasks);\n",
        "$last->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 7,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for end() with bracket shorthand"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("title")),
                "Should include title property from Task, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("run")),
                "Should include run method from Task, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$item = reset($items)` extracts element type.
#[tokio::test]
async fn test_completion_reset_extracts_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///reset.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Gadget {\n",
        "    public string $code;\n",
        "}\n",
        "/** @var list<Gadget> $gadgets */\n",
        "$gadgets = [];\n",
        "$g = reset($gadgets);\n",
        "$g->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 4,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for reset() element"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("code")),
                "Should include code property from Gadget, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Inside a class method, array functions with `@param` annotations
/// should also work.
#[tokio::test]
async fn test_completion_array_filter_from_param_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_func_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Service {\n",
        "    /**\n",
        "     * @param list<User> $users\n",
        "     */\n",
        "    public function process(array $users): void {\n",
        "        $active = array_filter($users, fn(User $u) => true);\n",
        "        $active[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 11: `        $active[0]->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for array_filter inside method"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$sliced = array_slice($users, 0, 5)` preserves element type.
#[tokio::test]
async fn test_completion_array_slice_preserves_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_slice.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public string $desc;\n",
        "}\n",
        "/** @var list<Item> $items */\n",
        "$items = [];\n",
        "$sliced = array_slice($items, 0, 5);\n",
        "$sliced[0]->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for array_slice result element"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("desc")),
                "Should include desc property from Item, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `array_filter($this->users, ...)` inside a class method should preserve
/// element type when the property has a `@var list<User>` annotation.
#[tokio::test]
async fn test_completion_array_filter_this_property_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///af_this_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Demo {\n",
        "    /** @var list<User> */\n",
        "    public array $users;\n",
        "    public function test(): void {\n",
        "        $active = array_filter($this->users, fn(User $u) => true);\n",
        "        $active[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `        $active[0]->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for array_filter with $this->prop arg"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `array_values($this->users)` inside a class method should preserve
/// element type via the property's `@var` annotation.
#[tokio::test]
async fn test_completion_array_values_this_property_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///av_this_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Demo {\n",
        "    /** @var list<User> */\n",
        "    public array $users;\n",
        "    public function test(): void {\n",
        "        $vals = array_values($this->users);\n",
        "        $vals[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `        $vals[0]->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for array_values with $this->prop arg"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `current($this->users)` inside a class method should extract the
/// element type from the property's `@var list<User>` annotation.
#[tokio::test]
async fn test_completion_current_this_property_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///cur_this_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Demo {\n",
        "    /** @var list<User> */\n",
        "    public array $users;\n",
        "    public function test(): void {\n",
        "        $cur = current($this->users);\n",
        "        $cur->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `        $cur->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for current() with $this->prop arg"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$users = $this->getUsers(); $last = array_pop($users);` should
/// resolve `$last` to the element type by chasing the variable assignment.
#[tokio::test]
async fn test_completion_array_pop_method_assigned_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///pop_method_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Demo {\n",
        "    /** @return list<User> */\n",
        "    public function getUsers(): array { return []; }\n",
        "    public function test(): void {\n",
        "        $users = $this->getUsers();\n",
        "        $last = array_pop($users);\n",
        "        $last->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 11: `        $last->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for array_pop with method-assigned variable"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$users = $this->getUsers(); $first = array_shift($users);` should
/// resolve `$first` to the element type by chasing the variable assignment.
#[tokio::test]
async fn test_completion_array_shift_method_assigned_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///shift_method_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Demo {\n",
        "    /** @return list<User> */\n",
        "    public function getUsers(): array { return []; }\n",
        "    public function test(): void {\n",
        "        $users = $this->getUsers();\n",
        "        $first = array_shift($users);\n",
        "        $first->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 11: `        $first->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for array_shift with method-assigned variable"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `array_map(fn($u) => $u, $this->users)` inside a class method should
/// fall back to the input element type from the property annotation.
#[tokio::test]
async fn test_completion_array_map_this_property_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///map_this_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Demo {\n",
        "    /** @var list<User> */\n",
        "    public array $users;\n",
        "    public function test(): void {\n",
        "        $mapped = array_map(fn($u) => $u, $this->users);\n",
        "        $mapped[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `        $mapped[0]->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for array_map with $this->prop arg"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `foreach (array_filter($this->users, ...) as $u)` inside a class method
/// should preserve the element type.
#[tokio::test]
async fn test_completion_foreach_array_filter_this_property_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_filter_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Demo {\n",
        "    /** @var list<User> */\n",
        "    public array $users;\n",
        "    public function test(): void {\n",
        "        foreach (array_filter($this->users, fn(User $u) => true) as $u) {\n",
        "            $u->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `            $u->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for foreach over array_filter with $this->prop"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from User, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getEmail")),
                "Should include getEmail method from User, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that `$var->propName` resolves via the text-based path when `$var`
/// is assigned from `new ClassName()`.
#[tokio::test]
async fn test_completion_variable_property_access_text_path() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_prop_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Address {\n",
        "    public string $city;\n",
        "    public function getZip(): string { return ''; }\n",
        "}\n",
        "class Person {\n",
        "    public Address $address;\n",
        "    public function test() {\n",
        "        $user = new Person();\n",
        "        $addr = $user->address;\n",
        "        $addr->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for $addr = $user->address; $addr->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("city")),
                "Should include 'city' property from Address, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getZip")),
                "Should include 'getZip' method from Address, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test `$var->propName` where `$var` is resolved from `$this->prop`.
#[tokio::test]
async fn test_completion_variable_property_access_from_this() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_prop_from_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Engine {\n",
        "    public int $horsepower;\n",
        "    public function start(): void {}\n",
        "}\n",
        "class Car {\n",
        "    public Engine $engine;\n",
        "    public function test() {\n",
        "        $e = $this->engine;\n",
        "        $hp = $e->horsepower;\n",
        "        // This tests that $e->horsepower resolves Engine first\n",
        "        $e->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for $e = $this->engine; $e->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("horsepower")),
                "Should include 'horsepower' property from Engine, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("start")),
                "Should include 'start' method from Engine, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test `$var->propName` resolves through an intermediate variable
/// assignment chain (two hops).
#[tokio::test]
async fn test_completion_variable_property_access_chained_assignments() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_prop_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Wheel {\n",
        "    public string $brand;\n",
        "    public function rotate(): void {}\n",
        "}\n",
        "class Axle {\n",
        "    public Wheel $frontLeft;\n",
        "}\n",
        "class Chassis {\n",
        "    public Axle $axle;\n",
        "    public function test() {\n",
        "        $a = $this->axle;\n",
        "        $w = $a->frontLeft;\n",
        "        $w->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for chained $a->frontLeft; $w->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("brand")),
                "Should include 'brand' property from Wheel, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("rotate")),
                "Should include 'rotate' method from Wheel, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test `$var->propName` cross-file: the variable and property types
/// come from a different file loaded via PSR-4.
#[tokio::test]
async fn test_completion_variable_property_access_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Models/Address.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Models;\n",
                    "class Address {\n",
                    "    public string $street;\n",
                    "    public function format(): string { return ''; }\n",
                    "}\n",
                ),
            ),
            (
                "src/Models/Customer.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Models;\n",
                    "class Customer {\n",
                    "    public Address $address;\n",
                    "    public function test() {\n",
                    "        $c = new Customer();\n",
                    "        $a = $c->address;\n",
                    "        $a->\n",
                    "    }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let cust_path = _dir.path().join("src/Models/Customer.php");
    let uri = Url::from_file_path(&cust_path).unwrap();
    let text = std::fs::read_to_string(&cust_path).unwrap();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for $a = $c->address; $a-> (cross-file)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("street")),
                "Should include 'street' property from Address, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("format")),
                "Should include 'format' method from Address, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test top-level (outside class) `$var->propName` resolution.
#[tokio::test]
async fn test_completion_variable_property_access_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_prop_top_level.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    public string $dsn;\n",
        "    public function validate(): bool { return true; }\n",
        "}\n",
        "class AppContext {\n",
        "    public Config $config;\n",
        "}\n",
        "$app = new AppContext();\n",
        "$cfg = $app->config;\n",
        "$cfg->\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for top-level $cfg = $app->config; $cfg->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("dsn")),
                "Should include 'dsn' property from Config, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("validate")),
                "Should include 'validate' method from Config, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that parameter resolution picks the correct method when multiple
/// methods share the same parameter name but with different types.
/// Regression: previously the resolver would return the parameter type
/// from the first method encountered (by source order), even when the
/// cursor was inside a later method.
#[tokio::test]
async fn test_completion_param_same_name_different_methods() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///param_same_name.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface IShoppingCart {}\n",
        "/**\n",
        " * @property int $customer_id\n",
        " */\n",
        "class ShoppingCart {\n",
        "    public int $customer_id;\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "}\n",
        "/**\n",
        " * @property int $id\n",
        " */\n",
        "class Customer {\n",
        "    public int $id;\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class CurrentCart {\n",
        "    private function handleGsmp(IShoppingCart $cart): void {}\n",
        "    private function updateCartData(ShoppingCart $cart, Customer $customer): void {\n",
        "        $cart->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 19,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for $cart-> in updateCartData (ShoppingCart, not IShoppingCart)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("customer_id")),
                "Should include 'customer_id' from ShoppingCart, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getTotal")),
                "Should include 'getTotal' from ShoppingCart, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same as above but verifying the second parameter ($customer) also
/// resolves correctly when a prior method has a different $customer.
#[tokio::test]
async fn test_completion_param_same_name_second_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///param_same_name2.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface ICustomer {}\n",
        "class Customer {\n",
        "    public int $id;\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Service {\n",
        "    private function other(ICustomer $customer): void {}\n",
        "    private function process(Customer $customer): void {\n",
        "        $customer->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for $customer-> in process (Customer, not ICustomer)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("id")),
                "Should include 'id' from Customer, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getName")),
                "Should include 'getName' from Customer, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Verify the fix also works when the matching method comes first in
/// source order (the cursor's method is declared before the other).
#[tokio::test]
async fn test_completion_param_same_name_cursor_in_first_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///param_first_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Alpha {\n",
        "    public string $name;\n",
        "    public function greet(): string { return ''; }\n",
        "}\n",
        "class Beta {\n",
        "    public int $code;\n",
        "}\n",
        "class Handler {\n",
        "    public function first(Alpha $item): void {\n",
        "        $item->\n",
        "    }\n",
        "    public function second(Beta $item): void {}\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for $item-> in first method (Alpha)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include 'name' from Alpha, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("greet")),
                "Should include 'greet' from Alpha, got: {:?}",
                labels
            );
            assert!(
                !labels.iter().any(|l| l.starts_with("code")),
                "Should NOT include 'code' from Beta, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that union completion sorts intersection members (shared by all
/// types) above branch-only members (present on a subset of types).
#[tokio::test]
async fn test_completion_union_sort_intersection_above_branch_only() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///union_sort.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "    public function eat(): void {}\n",
        "    public function sleep(): void {}\n",
        "}\n",
        "class Cat {\n",
        "    public function meow(): void {}\n",
        "    public function eat(): void {}\n",
        "    public function sleep(): void {}\n",
        "}\n",
        "class Zoo {\n",
        "    public function getAnimal(): Dog|Cat { return new Dog(); }\n",
        "    public function run(): void {\n",
        "        $pet = $this->getAnimal();\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 15,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return union completion results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            // Intersection members: eat, sleep (on both Dog and Cat)
            let eat = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("eat"))
                .unwrap();
            let sleep = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("sleep"))
                .unwrap();
            // Branch-only members: bark (Dog only), meow (Cat only)
            let bark = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("bark"))
                .unwrap();
            let meow = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("meow"))
                .unwrap();

            // Intersection sort_text should start with "0_"
            assert!(
                eat.sort_text.as_deref().unwrap().starts_with("0_"),
                "Intersection member 'eat' should have sort prefix '0_', got: {:?}",
                eat.sort_text
            );
            assert!(
                sleep.sort_text.as_deref().unwrap().starts_with("0_"),
                "Intersection member 'sleep' should have sort prefix '0_', got: {:?}",
                sleep.sort_text
            );

            // Branch-only sort_text should start with "1_"
            assert!(
                bark.sort_text.as_deref().unwrap().starts_with("1_"),
                "Branch-only member 'bark' should have sort prefix '1_', got: {:?}",
                bark.sort_text
            );
            assert!(
                meow.sort_text.as_deref().unwrap().starts_with("1_"),
                "Branch-only member 'meow' should have sort prefix '1_', got: {:?}",
                meow.sort_text
            );

            // All intersection members should sort before all branch-only
            let eat_sort = eat.sort_text.as_deref().unwrap();
            let sleep_sort = sleep.sort_text.as_deref().unwrap();
            let bark_sort = bark.sort_text.as_deref().unwrap();
            let meow_sort = meow.sort_text.as_deref().unwrap();
            assert!(
                eat_sort < bark_sort && eat_sort < meow_sort,
                "Intersection 'eat' ({}) should sort before branch-only 'bark' ({}) and 'meow' ({})",
                eat_sort,
                bark_sort,
                meow_sort
            );
            assert!(
                sleep_sort < bark_sort && sleep_sort < meow_sort,
                "Intersection 'sleep' ({}) should sort before branch-only 'bark' ({}) and 'meow' ({})",
                sleep_sort,
                bark_sort,
                meow_sort
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that branch-only members in a union get `label_details.description`
/// showing the originating class, while intersection members do not.
#[tokio::test]
async fn test_completion_union_branch_only_has_label_details() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///union_label_details.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Truck {\n",
        "    public function haul(): void {}\n",
        "    public function drive(): void {}\n",
        "}\n",
        "class Sedan {\n",
        "    public function park(): void {}\n",
        "    public function drive(): void {}\n",
        "}\n",
        "class Garage {\n",
        "    public function getVehicle(): Truck|Sedan { return new Truck(); }\n",
        "    public function demo(): void {\n",
        "        $v = $this->getVehicle();\n",
        "        $v->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return union completion results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            // Intersection member: drive (on both)
            let drive = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("drive"))
                .unwrap();
            // Branch-only members
            let haul = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("haul"))
                .unwrap();
            let park = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("park"))
                .unwrap();

            // Intersection members should have all class names pipe-separated
            let drive_desc = drive
                .label_details
                .as_ref()
                .and_then(|ld| ld.description.as_deref());
            assert_eq!(
                drive_desc,
                Some("Truck|Sedan"),
                "Intersection member 'drive' should show 'Truck|Sedan' in label_details, got: {:?}",
                drive.label_details
            );

            // Branch-only members SHOULD have label_details with class name
            let haul_desc = haul
                .label_details
                .as_ref()
                .and_then(|ld| ld.description.as_deref());
            assert_eq!(
                haul_desc,
                Some("Truck"),
                "Branch-only 'haul' should show 'Truck' in label_details, got: {:?}",
                haul.label_details
            );

            let park_desc = park
                .label_details
                .as_ref()
                .and_then(|ld| ld.description.as_deref());
            assert_eq!(
                park_desc,
                Some("Sedan"),
                "Branch-only 'park' should show 'Sedan' in label_details, got: {:?}",
                park.label_details
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When only a single class is resolved (no union), sort_text should
/// remain as-is (no "0_"/"1_" prefixing) and no label_details are added.
#[tokio::test]
async fn test_completion_single_class_no_union_sort_adjustment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///no_union.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): void {}\n",
        "    public function update(): void {}\n",
        "}\n",
        "class App {\n",
        "    public function test(): void {\n",
        "        $w = new Widget();\n",
        "        $w->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return completion results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let render = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("render"))
                .unwrap();
            let update = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("update"))
                .unwrap();

            // sort_text should NOT have "0_" or "1_" prefix
            assert!(
                !render.sort_text.as_deref().unwrap().starts_with("0_"),
                "Single-class completion should not use union sort prefix, got: {:?}",
                render.sort_text
            );
            assert!(
                !update.sort_text.as_deref().unwrap().starts_with("1_"),
                "Single-class completion should not use union sort prefix, got: {:?}",
                update.sort_text
            );

            // Single-class completions should have the class name in label_details description.
            let render_desc = render
                .label_details
                .as_ref()
                .and_then(|ld| ld.description.as_deref());
            assert_eq!(
                render_desc,
                Some("Widget"),
                "Single-class completion should have 'Widget' in label_details description, got: {:?}",
                render.label_details
            );
            let update_desc = update
                .label_details
                .as_ref()
                .and_then(|ld| ld.description.as_deref());
            assert_eq!(
                update_desc,
                Some("Widget"),
                "Single-class completion should have 'Widget' in label_details description, got: {:?}",
                update.label_details
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test union sorting with three types: members shared by all three sort
/// first, members shared by two sort after, and members unique to one
/// sort last. All branch-only items share the same "1_" prefix tier.
#[tokio::test]
async fn test_completion_union_three_types_sort() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///union_three.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function shared(): void {}\n",
        "    public function onlyA(): void {}\n",
        "}\n",
        "class B {\n",
        "    public function shared(): void {}\n",
        "    public function onlyB(): void {}\n",
        "}\n",
        "class C {\n",
        "    public function shared(): void {}\n",
        "    public function onlyC(): void {}\n",
        "}\n",
        "class Demo {\n",
        "    /** @return A|B|C */\n",
        "    public function get() {}\n",
        "    public function test(): void {\n",
        "        $x = $this->get();\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 18,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return union completion results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let shared = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("shared"))
                .unwrap();
            let only_a = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("onlyA"))
                .unwrap();
            let only_b = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("onlyB"))
                .unwrap();
            let only_c = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("onlyC"))
                .unwrap();

            // shared is on all 3 — intersection
            assert!(
                shared.sort_text.as_deref().unwrap().starts_with("0_"),
                "'shared' (on all 3) should be intersection, got: {:?}",
                shared.sort_text
            );

            // onlyA, onlyB, onlyC are branch-only
            assert!(
                only_a.sort_text.as_deref().unwrap().starts_with("1_"),
                "'onlyA' should be branch-only, got: {:?}",
                only_a.sort_text
            );
            assert!(
                only_b.sort_text.as_deref().unwrap().starts_with("1_"),
                "'onlyB' should be branch-only, got: {:?}",
                only_b.sort_text
            );
            assert!(
                only_c.sort_text.as_deref().unwrap().starts_with("1_"),
                "'onlyC' should be branch-only, got: {:?}",
                only_c.sort_text
            );

            // Intersection sorts before branch-only
            assert!(
                shared.sort_text.as_deref().unwrap() < only_a.sort_text.as_deref().unwrap(),
                "'shared' should sort before 'onlyA'"
            );

            // Intersection members should have all class names pipe-separated
            let shared_desc = shared
                .label_details
                .as_ref()
                .and_then(|ld| ld.description.as_deref());
            assert_eq!(
                shared_desc,
                Some("A|B|C"),
                "'shared' label_details should show 'A|B|C', got: {:?}",
                shared.label_details
            );

            // label_details on branch-only members
            assert!(
                only_a
                    .label_details
                    .as_ref()
                    .and_then(|ld| ld.description.as_deref())
                    == Some("A"),
                "'onlyA' label_details should be 'A', got: {:?}",
                only_a.label_details
            );
            assert!(
                only_b
                    .label_details
                    .as_ref()
                    .and_then(|ld| ld.description.as_deref())
                    == Some("B"),
                "'onlyB' label_details should be 'B', got: {:?}",
                only_b.label_details
            );
            assert!(
                only_c
                    .label_details
                    .as_ref()
                    .and_then(|ld| ld.description.as_deref())
                    == Some("C"),
                "'onlyC' label_details should be 'C', got: {:?}",
                only_c.label_details
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that union sorting also applies to properties, not just methods.
#[tokio::test]
async fn test_completion_union_sort_properties() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///union_props.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Circle {\n",
        "    public string $color;\n",
        "    public float $radius;\n",
        "}\n",
        "class Square {\n",
        "    public string $color;\n",
        "    public float $side;\n",
        "}\n",
        "class Canvas {\n",
        "    /** @return Circle|Square */\n",
        "    public function getShape() {}\n",
        "    public function draw(): void {\n",
        "        $s = $this->getShape();\n",
        "        $s->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 14,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return union completion results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            // Shared property: color
            let color = items.iter().find(|i| i.label == "color").unwrap();
            // Branch-only properties: radius (Circle), side (Square)
            let radius = items.iter().find(|i| i.label == "radius").unwrap();
            let side = items.iter().find(|i| i.label == "side").unwrap();

            assert!(
                color.sort_text.as_deref().unwrap().starts_with("0_"),
                "Shared property 'color' should be intersection, got: {:?}",
                color.sort_text
            );
            assert!(
                radius.sort_text.as_deref().unwrap().starts_with("1_"),
                "Branch-only 'radius' should have '1_' prefix, got: {:?}",
                radius.sort_text
            );
            assert!(
                side.sort_text.as_deref().unwrap().starts_with("1_"),
                "Branch-only 'side' should have '1_' prefix, got: {:?}",
                side.sort_text
            );

            // Intersection sorts before branch-only
            assert!(
                color.sort_text.as_deref().unwrap() < radius.sort_text.as_deref().unwrap(),
                "'color' should sort before 'radius'"
            );
            assert!(
                color.sort_text.as_deref().unwrap() < side.sort_text.as_deref().unwrap(),
                "'color' should sort before 'side'"
            );

            // All completion items now have label_details description
            let color_desc = color
                .label_details
                .as_ref()
                .and_then(|ld| ld.description.as_deref());
            assert_eq!(
                color_desc,
                Some("Circle|Square"),
                "'color' label_details should show 'Circle|Square', got: {:?}",
                color.label_details
            );
            assert!(
                radius
                    .label_details
                    .as_ref()
                    .and_then(|ld| ld.description.as_deref())
                    == Some("Circle"),
                "'radius' label_details should be 'Circle', got: {:?}",
                radius.label_details
            );
            assert!(
                side.label_details
                    .as_ref()
                    .and_then(|ld| ld.description.as_deref())
                    == Some("Square"),
                "'side' label_details should be 'Square', got: {:?}",
                side.label_details
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Array element access after method call on non-$this variable ───────────

/// When a variable holds an instance (`$src = new Foo()`) and a method on
/// that instance returns an array type (`Pen[]`), array element access on
/// the result should resolve to the element type.
///
/// This is the non-`$this` counterpart of
/// `test_chained_method_call_extract_raw_type_chain` which uses
/// `$this->getRepo()->findAll()`.
#[tokio::test]
async fn test_array_access_after_method_call_on_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_method_array_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen {\n",
        "    public function write(): string { return ''; }\n",
        "    public function color(): string { return ''; }\n",
        "}\n",
        "\n",
        "class PenSource {\n",
        "    /** @return Pen[] */\n",
        "    public function fetchAll(): array { return []; }\n",
        "}\n",
        "\n",
        "class Demo {\n",
        "    public function run(): void {\n",
        "        $src = new PenSource();\n",
        "        $pens = $src->fetchAll();\n",
        "        $pens[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$pens[0]->` on line 15
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 15,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for array element after $var->method() assignment"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("write")),
                "Should include write() from Pen via $src->fetchAll()[0]->, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("color")),
                "Should include color() from Pen via $src->fetchAll()[0]->, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same as above but the intermediate variable is accessed directly:
/// `$first = $pens[0]; $first->`
#[tokio::test]
async fn test_variable_assigned_from_array_access_on_variable_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_method_array_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen {\n",
        "    public function write(): string { return ''; }\n",
        "}\n",
        "\n",
        "class PenSource {\n",
        "    /** @return Pen[] */\n",
        "    public function fetchAll(): array { return []; }\n",
        "}\n",
        "\n",
        "class Demo {\n",
        "    public function run(): void {\n",
        "        $src = new PenSource();\n",
        "        $pens = $src->fetchAll();\n",
        "        $first = $pens[0];\n",
        "        $first->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$first->` on line 15
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 15,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return completions for $first assigned from $pens[0]"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("write")),
                "Should include write() from Pen via intermediate $first = $pens[0], got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Array Functions with $var->property Arguments ──────────────────────────

/// `array_filter($src->members, ...)` where `$src` is a local variable
/// (not `$this`) should preserve the element type from the property's
/// `@var` annotation.
#[tokio::test]
async fn test_completion_array_filter_var_property_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///af_var_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen {\n",
        "    public function write(): void {}\n",
        "}\n",
        "class Source {\n",
        "    /** @var list<Pen> */\n",
        "    public array $members;\n",
        "}\n",
        "class Demo {\n",
        "    public function test(Source $src): void {\n",
        "        $active = array_filter($src->members, fn(Pen $p) => true);\n",
        "        $active[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 11: `        $active[0]->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for array_filter with $src->members arg"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("write")),
                "Should include write() from Pen, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `array_values($src->members)` where `$src` is a local variable
/// should preserve the element type.
#[tokio::test]
async fn test_completion_array_values_var_property_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///av_var_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen {\n",
        "    public function write(): void {}\n",
        "}\n",
        "class Source {\n",
        "    /** @var list<Pen> */\n",
        "    public array $members;\n",
        "}\n",
        "class Demo {\n",
        "    public function test(Source $src): void {\n",
        "        $vals = array_values($src->members);\n",
        "        $vals[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 11: `        $vals[0]->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for array_values with $src->members arg"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("write")),
                "Should include write() from Pen, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `end($src->members)->write()` — inline call without an intermediate
/// variable should resolve the element type.
#[tokio::test]
async fn test_completion_end_inline_var_property_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///end_var_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen {\n",
        "    public function write(): void {}\n",
        "}\n",
        "class Source {\n",
        "    /** @var list<Pen> */\n",
        "    public array $members;\n",
        "}\n",
        "class Demo {\n",
        "    public function test(Source $src): void {\n",
        "        end($src->members)->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 10: `        end($src->members)->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 28,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for end($src->members)->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("write")),
                "Should include write() from Pen, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `array_map(fn($pen) => $pen, $src->members)` where `$src` is a
/// local variable should infer the element type from the property.
#[tokio::test]
async fn test_completion_array_map_var_property_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///am_var_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen {\n",
        "    public function write(): void {}\n",
        "}\n",
        "class Source {\n",
        "    /** @var list<Pen> */\n",
        "    public array $members;\n",
        "}\n",
        "class Demo {\n",
        "    public function test(Source $src): void {\n",
        "        $mapped = array_map(fn($pen) => $pen, $src->members);\n",
        "        $mapped[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Line 11: `        $mapped[0]->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for array_map with $src->members arg"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("write")),
                "Should include write() from Pen via array_map fallback, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Arrow function: outer-scope variable capture ───────────────────────────

/// Arrow functions in PHP automatically capture variables from the
/// enclosing scope.  A variable assigned before the arrow function
/// must remain resolvable inside the arrow function body.
#[tokio::test]
async fn test_completion_outer_scope_variable_inside_arrow_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///arrow_outer_scope.php").unwrap();
    let text = concat!(
        "<?php\n",                             // 0
        "class Feature {\n",                   // 1
        "    public int $id = 0;\n",           // 2
        "    public string $name = '';\n",     // 3
        "}\n",                                 // 4
        "class FeatureVariation {\n",          // 5
        "    public int $feature_id = 0;\n",   // 6
        "}\n",                                 // 7
        "class Service {\n",                   // 8
        "    public function run(): void {\n", // 9
        "        $feature = new Feature();\n", // 10
        "        $x = array_map(fn(FeatureVariation $v): bool => $v->feature_id === $feature->, []);\n", // 11
        "    }\n", // 12
        "}\n",     // 13
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$feature->` on line 11
    // `        $x = array_map(fn(FeatureVariation $v): bool => $v->feature_id === $feature->`
    //  85 chars to the end of `->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 85,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve outer-scope $feature inside arrow function"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("id")),
                "Should include id property from outer-scope $feature, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from outer-scope $feature, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: parameter type resolution inside a function wrapped in
/// `if (! function_exists('...'))` guard — the pattern used by Laravel
/// helpers and many other PHP libraries.
#[tokio::test]
async fn test_completion_param_inside_function_exists_guard() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guarded_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Country {\n",
        "    public function isActiveCountry(): bool { return true; }\n",
        "    public function getCode(): string { return ''; }\n",
        "}\n",
        "\n",
        "if (!function_exists('getHTMLCountryFlagFromLangCode')) {\n",
        "    function getHTMLCountryFlagFromLangCode(Country $langCode, string $height = ''): string\n",
        "    {\n",
        "        $langCode->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $langCode-> inside function_exists guard"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"isActiveCountry"),
                "Should include 'isActiveCountry' from Country, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getCode"),
                "Should include 'getCode' from Country, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: parameter type resolution inside a function at the top level
/// (no `function_exists` guard) still works as before — regression guard.
#[tokio::test]
async fn test_completion_param_inside_top_level_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///toplevel_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Country {\n",
        "    public function isActiveCountry(): bool { return true; }\n",
        "}\n",
        "\n",
        "function getFlag(Country $langCode): string\n",
        "{\n",
        "    $langCode->\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $langCode-> inside top-level function"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"isActiveCountry"),
                "Should include 'isActiveCountry' from Country, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Null-safe method call chain variable assignment ────────────────────────

/// When a variable is assigned from a null-safe method call chain like
/// `$x = $obj?->getItems()->last()`, the type engine should resolve
/// through the `?->` call the same way it resolves through `->`.
#[tokio::test]
async fn test_completion_nullsafe_method_chain_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nullsafe_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class NsEngine {\n",
        "    public function horsepower(): int { return 100; }\n",
        "    public function rev(): NsEngine { return $this; }\n",
        "}\n",
        "class NsCarFactory {\n",
        "    public function buildEngine(): NsEngine { return new NsEngine(); }\n",
        "}\n",
        "class NsGarage {\n",
        "    private ?NsCarFactory $factory;\n",
        "    public function test(): void {\n",
        "        $engine = $this->factory?->buildEngine();\n",
        "        $engine->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 12,
                    character: 17,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Completion should return results for $engine-> after null-safe chain assignment"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"horsepower"),
                "Should include 'horsepower' from NsEngine, got: {:?}",
                names
            );
            assert!(
                names.contains(&"rev"),
                "Should include 'rev' from NsEngine, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Deeper chain: `$x = $a?->b()?->c()` — null-safe in the middle of a chain.
#[tokio::test]
async fn test_completion_nullsafe_mid_chain_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nullsafe_mid_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class NsMcPart {\n",
        "    public function weight(): int { return 5; }\n",
        "}\n",
        "class NsMcAssembly {\n",
        "    public function mainPart(): NsMcPart { return new NsMcPart(); }\n",
        "}\n",
        "class NsMcMachine {\n",
        "    public function getAssembly(): ?NsMcAssembly { return null; }\n",
        "}\n",
        "function testNsMc(NsMcMachine $m): void {\n",
        "    $part = $m->getAssembly()?->mainPart();\n",
        "    $part->\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 12,
                    character: 12,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Completion should return results for $part-> after ?-> mid-chain"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"weight"),
                "Should include 'weight' from NsMcPart, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Cross-file null-safe method call chain ─────────────────────────────────

/// When a variable is assigned from a cross-file null-safe method call chain
/// like `$x = $obj?->getItems()->last()`, the type engine should resolve
/// through the `?->` call across file boundaries.
#[tokio::test]
async fn test_completion_nullsafe_method_chain_cross_file() {
    let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            (
                "src/Subscription.php",
                "<?php\nnamespace App;\nclass Subscription {\n    public function price(): float { return 0.0; }\n    public function isActive(): bool { return true; }\n}\n",
            ),
            (
                "src/SubscriptionCollection.php",
                "<?php\nnamespace App;\nclass SubscriptionCollection {\n    public function lastPeriod(): ?Subscription { return null; }\n}\n",
            ),
            (
                "src/Agreement.php",
                "<?php\nnamespace App;\nclass Agreement {\n    public function getPeriods(): SubscriptionCollection { return new SubscriptionCollection(); }\n}\n",
            ),
            (
                "src/Customer.php",
                "<?php\nnamespace App;\nclass Customer {\n    public function getLatestAgreement(): ?Agreement { return null; }\n}\n",
            ),
        ],
    );

    let uri = "file:///src/ProfileBuilder.php";
    let content = concat!(
        "<?php\n",
        "namespace App;\n",
        "class ProfileBuilder {\n",
        "    public function build(Customer $customer): void {\n",
        "        $agreement = $customer->getLatestAgreement();\n",
        "        $lastPeriod = $agreement?->getPeriods()->lastPeriod();\n",
        "        $lastPeriod->\n",
        "    }\n",
        "}\n",
    );

    backend
        .did_open(tower_lsp::lsp_types::DidOpenTextDocumentParams {
            text_document: tower_lsp::lsp_types::TextDocumentItem {
                uri: tower_lsp::lsp_types::Url::parse(uri).unwrap(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: tower_lsp::lsp_types::Url::parse(uri).unwrap(),
                },
                position: Position {
                    line: 6,
                    character: 21,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Completion should return results for $lastPeriod-> after cross-file null-safe chain"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"price"),
                "Should include 'price' from Subscription, got: {:?}",
                names
            );
            assert!(
                names.contains(&"isActive"),
                "Should include 'isActive' from Subscription, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Ternary / null-coalesce RHS resolution ─────────────────────────────────

/// Short ternary as RHS: `$p = $cond ?: new Printer()` should resolve `$p`.
#[tokio::test]
async fn test_completion_short_ternary_rhs_instantiation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///short_ternary.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Printer {\n",
        "    public function print(): void {}\n",
        "}\n",
        "class Demo {\n",
        "    public function run(): void {\n",
        "        $p = null ?: new Printer();\n",
        "        $p->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for short ternary RHS"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"print"),
                "Should include 'print' from Printer via ternary, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Full ternary as RHS with instantiation on both branches.
#[tokio::test]
async fn test_completion_full_ternary_rhs_instantiation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///full_ternary.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Renderer {\n",
        "    public function render(): string { return ''; }\n",
        "}\n",
        "class Page {\n",
        "    public function show(bool $flag): void {\n",
        "        $r = $flag ? new Renderer() : new Renderer();\n",
        "        $r->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for full ternary RHS"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"render"),
                "Should include 'render' from Renderer via ternary, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Nullable type resolution ───────────────────────────────────────────────

/// A parameter typed `?ClassName` should resolve to `ClassName` for completion.
#[tokio::test]
async fn test_completion_nullable_param_type_hint() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nullable_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Invoice {\n",
        "    public function total(): int { return 0; }\n",
        "    public function isPaid(): bool { return false; }\n",
        "}\n",
        "class Processor {\n",
        "    public function handle(?Invoice $inv): void {\n",
        "        $inv->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for ?Invoice parameter"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"total"),
                "Should include 'total' from Invoice via ?Invoice param, got: {:?}",
                names
            );
            assert!(
                names.contains(&"isPaid"),
                "Should include 'isPaid' from Invoice via ?Invoice param, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// A `@var ?ClassName` docblock annotation should resolve for completion.
#[tokio::test]
async fn test_completion_nullable_var_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nullable_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): string { return ''; }\n",
        "}\n",
        "class Page {\n",
        "    public function show(): void {\n",
        "        /** @var ?Widget $w */\n",
        "        $w = getWidget();\n",
        "        $w->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for @var ?Widget"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"render"),
                "Should include 'render' from Widget via @var ?Widget, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// A nullable return type `?ClassName` should resolve when chaining.
#[tokio::test]
async fn test_completion_nullable_return_type_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nullable_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function total(): int { return 0; }\n",
        "}\n",
        "class OrderService {\n",
        "    public function find(int $id): ?Order { return null; }\n",
        "    public function test(): void {\n",
        "        $order = $this->find(1);\n",
        "        $order->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for nullable return type ?Order"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"total"),
                "Should include 'total' from Order via ?Order return, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Null-init + conditional reassign ───────────────────────────────────────

/// When a variable is initialized as `$x = null` and then conditionally
/// reassigned inside a foreach loop, the type from the reassignment
/// should be available after the loop.
#[tokio::test]
async fn test_null_init_foreach_reassign() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b11_foreach.php").unwrap();
    let text = concat!(
        "<?php\n",                                             // 0
        "class Transaction {\n",                               // 1
        "    public function commit(): void {}\n",             // 2
        "}\n",                                                 // 3
        "class Svc {\n",                                       // 4
        "    /** @param list<string> $items */\n",             // 5
        "    public function process(array $items): void {\n", // 6
        "        $tx = null;\n",                               // 7
        "        foreach ($items as $item) {\n",               // 8
        "            $tx = new Transaction();\n",              // 9
        "        }\n",                                         // 10
        "        if ($tx) {\n",                                // 11
        "            $tx->\n",                                 // 12
        "        }\n",                                         // 13
        "    }\n",                                             // 14
        "}\n",                                                 // 15
    );

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
                position: Position {
                    line: 12,
                    character: 17,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions after null-init + foreach reassign"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"commit"),
                "Should include Transaction's commit() after foreach reassign, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Variable-to-variable assignment inside foreach: `$found = $pen` where
/// `$pen` is the foreach value variable. The RHS variable must be resolved
/// through the foreach value type inference.
#[tokio::test]
async fn test_null_init_foreach_var_to_var_reassign() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b11_var_to_var.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class Pen {\n",                                        // 1
        "    public function write(): void {}\n",               // 2
        "    public function color(): string { return ''; }\n", // 3
        "}\n",                                                  // 4
        "class Svc {\n",                                        // 5
        "    /** @param list<Pen> $pens */\n",                  // 6
        "    public function find(array $pens): void {\n",      // 7
        "        $found = null;\n",                             // 8
        "        foreach ($pens as $pen) {\n",                  // 9
        "            if ($pen->color() === 'blue') {\n",        // 10
        "                $found = $pen;\n",                     // 11
        "            }\n",                                      // 12
        "        }\n",                                          // 13
        "        if ($found) {\n",                              // 14
        "            $found->\n",                               // 15
        "        }\n",                                          // 16
        "    }\n",                                              // 17
        "}\n",                                                  // 18
    );

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
                position: Position {
                    line: 15,
                    character: 20,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for var-to-var reassign from foreach value"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"write"),
                "Should include Pen's write() after var-to-var reassign from foreach value, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same as above but with a nested if inside the foreach (the real-world
/// pattern from the bug report: conditional reassignment inside a loop).
#[tokio::test]
async fn test_null_init_foreach_nested_if_reassign() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b11_foreach_nested.php").unwrap();
    let text = concat!(
        "<?php\n",                                             // 0
        "class Transaction {\n",                               // 1
        "    public function commit(): void {}\n",             // 2
        "}\n",                                                 // 3
        "class Svc {\n",                                       // 4
        "    /** @param list<string> $items */\n",             // 5
        "    public function process(array $items): void {\n", // 6
        "        $tx = null;\n",                               // 7
        "        foreach ($items as $item) {\n",               // 8
        "            if ($item === 'start') {\n",              // 9
        "                $tx = new Transaction();\n",          // 10
        "            }\n",                                     // 11
        "        }\n",                                         // 12
        "        if ($tx) {\n",                                // 13
        "            $tx->\n",                                 // 14
        "        }\n",                                         // 15
        "    }\n",                                             // 16
        "}\n",                                                 // 17
    );

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
                position: Position {
                    line: 14,
                    character: 17,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions after null-init + foreach + nested if reassign"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"commit"),
                "Should include Transaction's commit() after foreach + nested if reassign, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Pattern 2: `$x = $arr[$key] ?? null` inside a foreach,
/// followed by a guard clause. The null-coalesce RHS (`null`) resolves
/// to empty, so `$x` gets the type from the LHS array access.
#[tokio::test]
async fn test_null_coalesce_guard_inside_foreach() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b11_coalesce.php").unwrap();
    let text = concat!(
        "<?php\n",                                            // 0
        "class Day {\n",                                      // 1
        "    public string $from;\n",                         // 2
        "}\n",                                                // 3
        "class Svc {\n",                                      // 4
        "    /** @param array<string, Day> $schedule */\n",   // 5
        "    public function run(array $schedule): void {\n", // 6
        "        $keys = ['mon', 'tue'];\n",                  // 7
        "        foreach ($keys as $key) {\n",                // 8
        "            $day = $schedule[$key] ?? null;\n",      // 9
        "            if (!$day) { continue; }\n",             // 10
        "            $day->\n",                               // 11
        "        }\n",                                        // 12
        "    }\n",                                            // 13
        "}\n",                                                // 14
    );

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
                position: Position {
                    line: 11,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for null-coalesce + guard inside foreach"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"from"),
                "Should include Day's 'from' property after coalesce + guard, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same pattern but with a while loop instead of foreach.
#[tokio::test]
async fn test_null_init_while_reassign() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b11_while.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function total(): int { return 0; }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $order = null;\n",
        "        while ($row = rand(0,1)) {\n",
        "            $order = new Order();\n",
        "        }\n",
        "        if ($order) {\n",
        "            $order->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 11,
                    character: 20,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions after null-init + while reassign"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"total"),
                "Should include Order's total() after while reassign, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Null-init + reassign inside a for loop body.
#[tokio::test]
async fn test_null_init_for_reassign() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b11_for.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $w = null;\n",
        "        for ($i = 0; $i < 5; $i++) {\n",
        "            $w = new Widget();\n",
        "        }\n",
        "        if ($w) {\n",
        "            $w->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 11,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions after null-init + for reassign"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"render"),
                "Should include Widget's render() after for reassign, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file version: null-init + foreach + PSR-4 class.
#[tokio::test]
async fn test_null_init_foreach_reassign_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[
            (
                "src/Invoice.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Invoice {\n",
                    "    public function send(): void {}\n",
                    "}\n",
                ),
            ),
            (
                "src/Processor.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Processor {\n",
                    "    /** @param list<string> $ids */\n",
                    "    public function run(array $ids): void {\n",
                    "        $invoice = null;\n",
                    "        foreach ($ids as $id) {\n",
                    "            $invoice = new Invoice();\n",
                    "        }\n",
                    "        if ($invoice) {\n",
                    "            $invoice->\n",
                    "        }\n",
                    "    }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///src/Processor.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: std::fs::read_to_string(_dir.path().join("src/Processor.php")).unwrap(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 10,
                    character: 24,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for cross-file null-init + foreach reassign"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"send"),
                "Should include Invoice's send() after foreach reassign (cross-file), got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ── Null coalesce (`??`) type refinement ────────────────────────────────────

/// When the LHS of `??` is non-nullable (`new Foo()`), the RHS is dead
/// code and the result should resolve to the LHS type only.
#[tokio::test]
async fn test_null_coalesce_non_nullable_lhs_ignores_rhs() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nc_non_nullable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Alpha {\n",
        "    public function alphaMethod(): void {}\n",
        "}\n",
        "class Beta {\n",
        "    public function betaMethod(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    public function test(): void {\n",
        "        $x = new Alpha() ?? new Beta();\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 10,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"alphaMethod"),
                "Should include Alpha's method (non-nullable LHS), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"betaMethod"),
                "Should NOT include Beta's method (RHS is dead code), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When the LHS of `??` is nullable (`?Foo` return type), the result
/// should be the non-null part of the LHS unioned with the RHS.
#[tokio::test]
async fn test_null_coalesce_nullable_lhs_unions_with_rhs() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nc_nullable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen {\n",
        "    public function write(): void {}\n",
        "}\n",
        "class Pencil {\n",
        "    public function sketch(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /** @return ?Pen */\n",
        "    public function maybePen(): ?Pen { return null; }\n",
        "    public function test(): void {\n",
        "        $x = $this->maybePen() ?? new Pencil();\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 12,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"write"),
                "Should include Pen's write() (nullable LHS, null stripped), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"sketch"),
                "Should include Pencil's sketch() (RHS of ??), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When the LHS of `??` is a variable with unknown/mixed type, the RHS
/// should still resolve.
#[tokio::test]
async fn test_null_coalesce_unknown_lhs_resolves_rhs() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nc_unknown.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Fallback {\n",
        "    public function fallbackMethod(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    public function test(mixed $input): void {\n",
        "        $x = $input ?? new Fallback();\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 7,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"fallbackMethod"),
                "Should include Fallback's method when LHS is unknown, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Array access on an untyped base resolves to `mixed` (not an empty
/// untyped result), so the `??` RHS still contributes its members.
#[tokio::test]
async fn test_null_coalesce_array_access_untyped_base_resolves_rhs() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nc_arr_untyped.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Fallback {\n",
        "    public function fallbackMethod(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    public function test($params): void {\n",
        "        $x = $params['key'] ?? new Fallback();\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 7,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"fallbackMethod"),
                "Should include Fallback's method, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Non-nullable LHS via clone expression — RHS should be ignored.
#[tokio::test]
async fn test_null_coalesce_clone_lhs_ignores_rhs() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nc_clone.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Original {\n",
        "    public function origMethod(): void {}\n",
        "}\n",
        "class Other {\n",
        "    public function otherMethod(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    public function test(Original $o): void {\n",
        "        $x = clone $o ?? new Other();\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 10,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"origMethod"),
                "Should include Original's method (clone is non-nullable), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"otherMethod"),
                "Should NOT include Other's method (RHS is dead code after clone), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Variable inside constructor args of self-reassignment ──────────────────

/// When `$request = new Bar(arg: $request->…)`, the `$request` inside
/// the constructor arguments should still resolve to the *previous* type
/// (Foo), not the type being assigned (Bar).  PHP evaluates all RHS
/// arguments before performing the assignment.
#[tokio::test]
async fn test_completion_var_inside_reassignment_rhs_sees_old_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b13_reassign_rhs.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public string $uuid = '';\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "class Bar {\n",
        "    public function __construct(public string $name = '') {}\n",
        "    public function barMethod(): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run(Foo $request): void {\n",
        "        $request = new Bar(\n",
        "            name: $request->\n",
        "        );\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$request->` on the line inside constructor args (line 12)
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 28,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $request-> inside constructor args"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include Foo's members (the original param type)
            assert!(
                labels.iter().any(|l| l.starts_with("uuid")),
                "Should include Foo's uuid property inside reassignment RHS, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("fooMethod")),
                "Should include Foo's fooMethod inside reassignment RHS, got: {:?}",
                labels
            );
            // Should NOT include Bar's members (the type being assigned)
            assert!(
                !labels.iter().any(|l| l.starts_with("barMethod")),
                "Should NOT include Bar's barMethod inside reassignment RHS, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Inline @var cast should not override variable type in RHS ──────────────

/// When `/** @var array<string, mixed> */ $data = $data->toArray()`, the
/// `$data` on the RHS should still resolve to the *previous* type (Data),
/// not the `@var` cast type.  The `@var` annotation only applies after
/// the assignment completes.
#[tokio::test]
async fn test_completion_inline_var_cast_does_not_override_rhs() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b15_inline_var_rhs.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Data {\n",                                        // 1
        "    public function toArray(): array { return []; }\n", // 2
        "    public function count(): int { return 0; }\n",      // 3
        "}\n",                                                   // 4
        "class Test {\n",                                        // 5
        "    public function run(Data $data): array {\n",        // 6
        "        /** @var array<string, mixed> */\n",            // 7
        "        $data = $data->toArray();\n",                   // 8
        "    }\n",                                               // 9
        "}\n",                                                   // 10
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$data->` on line 8, inside the RHS of the assignment.
    // Character 23 is right after `$data->` (8 indent + "$data = $data->" = 23).
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 23,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $data-> inside RHS"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include Data's members (the original param type)
            assert!(
                labels.iter().any(|l| l.starts_with("toArray")),
                "Should include Data's toArray method inside reassignment RHS, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("count")),
                "Should include Data's count method inside reassignment RHS, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Ensure the `@var` cast still applies *after* the
/// assignment (i.e. on the next line) so we don't break the normal
/// inline `@var` override behaviour.
#[tokio::test]
async fn test_completion_inline_var_cast_applies_after_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b15_inline_var_after.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Data {\n",
        "    public function toArray(): array { return []; }\n",
        "}\n",
        "class Wrapper {\n",
        "    public string $name;\n",
        "    public function getLabel(): string { return ''; }\n",
        "}\n",
        "class Test {\n",
        "    public function run(Data $data): void {\n",
        "        /** @var Wrapper */\n",
        "        $data = $data->toArray();\n",
        "        $data->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$data->` on line 12 (after the assignment)
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $data-> after @var assignment"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Should include Wrapper's members (the @var override type)
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include Wrapper's name property after @var assignment, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getLabel")),
                "Should include Wrapper's getLabel method after @var assignment, got: {:?}",
                labels
            );
            // Should NOT include Data's members
            assert!(
                !labels.iter().any(|l| l.starts_with("toArray")),
                "Should NOT include Data's toArray after @var override, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `class-string<T>` parameter annotation should resolve static
/// member access (`$class::`) against the bound type `T`.
#[tokio::test]
async fn test_completion_class_string_param_static_access() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///class_string_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class CustomerModel {\n",
        "    public const KEY = 'customer';\n",
        "    public static function from(array $values): static {}\n",
        "    public static string $table = 'customers';\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    /**\n",
        "     * @template T of CustomerModel\n",
        "     * @param class-string<T> $class\n",
        "     * @return T|null\n",
        "     */\n",
        "    private function parseModel(string $class): ?CustomerModel {\n",
        "        $class::\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 14,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for class-string<T> $class::"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"KEY"),
                "Should include constant 'KEY' from CustomerModel, got: {:?}",
                names
            );
            assert!(
                names.contains(&"from"),
                "Should include static method 'from' from CustomerModel, got: {:?}",
                names
            );
            assert!(
                names.contains(&"$table"),
                "Should include static property '$table' from CustomerModel, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `class-string<BackedEnum>` parameter, static call on a
/// concrete class (not a template) should also resolve.
#[tokio::test]
async fn test_completion_class_string_param_backed_enum_static_access() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///class_string_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface BackedEnum {\n",
        "    public static function cases(): array;\n",
        "    public static function from(string|int $value): static;\n",
        "}\n",
        "\n",
        "class OptionList {\n",
        "    /**\n",
        "     * @param class-string<BackedEnum> $class\n",
        "     */\n",
        "    public function getOptions(string $class): array {\n",
        "        $class::\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for class-string<BackedEnum> $class::"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"cases"),
                "Should include static method 'cases' from BackedEnum, got: {:?}",
                names
            );
            assert!(
                names.contains(&"from"),
                "Should include static method 'from' from BackedEnum, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `class-string<T>` in a standalone function should also resolve
/// static member access against the template bound.
#[tokio::test]
async fn test_completion_class_string_param_standalone_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///class_string_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public const LEGS = 4;\n",
        "    public static function species(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template T of Animal\n",
        " * @param class-string<T> $class\n",
        " * @return T\n",
        " */\n",
        "function createAnimal(string $class): Animal {\n",
        "    $class::\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for class-string<T> in standalone function"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"LEGS"),
                "Should include constant 'LEGS' from Animal, got: {:?}",
                names
            );
            assert!(
                names.contains(&"species"),
                "Should include static method 'species' from Animal, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `class-string<BackedEnum>`, foreach over `$class::cases()`
/// should resolve `$item` to the bound type so that `$item->name`
/// and `$item->value` are available.
#[tokio::test]
async fn test_completion_class_string_foreach_cases() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///class_string_foreach.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface BackedEnum {\n",
        "    public string $name;\n",
        "    public string|int $value;\n",
        "    public static function cases(): array;\n",
        "    public static function from(string|int $value): static;\n",
        "}\n",
        "\n",
        "class Status implements BackedEnum {\n",
        "    public string $name;\n",
        "    public string|int $value;\n",
        "    public static function cases(): array {}\n",
        "    public static function from(string|int $value): static {}\n",
        "    public function getLabel(): string {}\n",
        "}\n",
        "\n",
        "class OptionList {\n",
        "    /**\n",
        "     * @param class-string<BackedEnum> $class\n",
        "     */\n",
        "    public function getOptions(string $class): array {\n",
        "        $items = $class::cases();\n",
        "        $class::\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Verify that $class:: resolves to BackedEnum's static members
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 22,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for class-string<BackedEnum> $class::"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"cases"),
                "Should include static method 'cases' from BackedEnum, got: {:?}",
                names
            );
            assert!(
                names.contains(&"from"),
                "Should include static method 'from' from BackedEnum, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `class-string<BackedEnum>` parameter: foreach over `$class::cases()`
/// should resolve `$item` to the bound type so that `$item->name`
/// and `$item->value` are available via instance member completion.
#[tokio::test]
async fn test_completion_class_string_foreach_cases_item_members() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///class_string_foreach_item.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface UnitEnum {\n",
        "    /** @return static[] */\n",
        "    public static function cases(): array;\n",
        "    public readonly string $name;\n",
        "}\n",
        "interface BackedEnum extends UnitEnum {\n",
        "    public static function from(string|int $value): static;\n",
        "    public readonly string|int $value;\n",
        "}\n",
        "\n",
        "class OptionList {\n",
        "    /**\n",
        "     * @param class-string<BackedEnum> $class\n",
        "     */\n",
        "    public function getOptions(string $class): array {\n",
        "        foreach ($class::cases() as $item) {\n",
        "            $item->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 17,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $item-> inside foreach over $class::cases()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"name"),
                "Should include property 'name' from UnitEnum on foreach item, got: {:?}",
                names
            );
            assert!(
                names.contains(&"value"),
                "Should include property 'value' from BackedEnum on foreach item, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `class-string<BackedEnum>` parameter: `$class::from()` should
/// resolve the `static` return type to `BackedEnum`, making instance
/// members available on the result via chaining.
#[tokio::test]
async fn test_completion_class_string_static_return_type_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///class_string_static_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface UnitEnum {\n",
        "    /** @return static[] */\n",
        "    public static function cases(): array;\n",
        "    public readonly string $name;\n",
        "}\n",
        "interface BackedEnum extends UnitEnum {\n",
        "    public static function from(string|int $value): static;\n",
        "    public readonly string|int $value;\n",
        "}\n",
        "\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param class-string<BackedEnum> $class\n",
        "     */\n",
        "    public function resolve(string $class): void {\n",
        "        $result = $class::from('foo');\n",
        "        $result->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 17,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $result-> after $class::from()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"name"),
                "Should include property 'name' from BackedEnum on static return, got: {:?}",
                names
            );
            assert!(
                names.contains(&"value"),
                "Should include property 'value' from BackedEnum on static return, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `class-string<T>` in a standalone function: `$class::from()` should
/// resolve through the template bound and make instance members available.
#[tokio::test]
async fn test_completion_class_string_template_standalone_function_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///class_string_tpl_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface UnitEnum {\n",
        "    /** @return static[] */\n",
        "    public static function cases(): array;\n",
        "    public readonly string $name;\n",
        "}\n",
        "interface BackedEnum extends UnitEnum {\n",
        "    public static function from(string|int $value): static;\n",
        "    public readonly string|int $value;\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template T of BackedEnum\n",
        " * @param class-string<T> $class\n",
        " * @return T\n",
        " */\n",
        "function createEnum(string $class): BackedEnum {\n",
        "    $item = $class::from('test');\n",
        "    $item->\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 18,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $item-> after $class::from() in standalone function"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"name"),
                "Should include property 'name' from BackedEnum via template bound, got: {:?}",
                names
            );
            assert!(
                names.contains(&"value"),
                "Should include property 'value' from BackedEnum via template bound, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Loop-body assignments visible inside the same iteration ──────────

/// When a variable is initialized as `null` and reassigned later in the
/// same foreach body, the assignment type should be visible earlier in
/// the loop (from a previous iteration).  Combined with `!== null`
/// narrowing, the variable should resolve to the assigned class type.
#[tokio::test]
async fn test_loop_body_assignment_visible_inside_foreach() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b20_foreach_inside.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Carbon {\n",
        "    public function diffInDays(Carbon $other): int { return 0; }\n",
        "    public function startOfDay(): Carbon { return new self(); }\n",
        "}\n",
        "class Period {\n",
        "    public Carbon $ending;\n",
        "}\n",
        "class Svc {\n",
        "    /** @param list<Period> $periods */\n",
        "    public function run(array $periods): void {\n",
        "        $lastPaidEnd = null;\n",
        "        foreach ($periods as $period) {\n",
        "            if ($lastPaidEnd !== null) {\n",
        "                $lastPaidEnd->\n",
        "            }\n",
        "            $lastPaidEnd = $period->ending->startOfDay();\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 14,
                    character: 30,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for loop-carried assignment inside foreach"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"diffInDays"),
                "Should include Carbon's diffInDays() from loop-carried assignment, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same scenario as the foreach test but with a while loop.
#[tokio::test]
async fn test_loop_body_assignment_visible_inside_while() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b20_while_inside.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Record {\n",
        "    public function save(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $prev = null;\n",
        "        while ($row = rand(0,1)) {\n",
        "            if ($prev !== null) {\n",
        "                $prev->\n",
        "            }\n",
        "            $prev = new Record();\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 9,
                    character: 23,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for loop-carried assignment inside while"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"save"),
                "Should include Record's save() from loop-carried assignment in while, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same scenario with a for loop.
#[tokio::test]
async fn test_loop_body_assignment_visible_inside_for() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b20_for_inside.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function label(): string { return ''; }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $last = null;\n",
        "        for ($i = 0; $i < 10; $i++) {\n",
        "            if ($last !== null) {\n",
        "                $last->\n",
        "            }\n",
        "            $last = new Item();\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 9,
                    character: 23,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for loop-carried assignment inside for"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"label"),
                "Should include Item's label() from loop-carried assignment in for, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Loop-carried assignment inside a nested if within a foreach body.
/// The assignment is inside a conditional branch, but should still be
/// visible on subsequent iterations.
#[tokio::test]
async fn test_loop_body_nested_if_assignment_visible_inside_foreach() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b20_nested_if.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Marker {\n",
        "    public function position(): string { return ''; }\n",
        "}\n",
        "class Svc {\n",
        "    /** @param list<int> $values */\n",
        "    public function run(array $values): void {\n",
        "        $marker = null;\n",
        "        foreach ($values as $val) {\n",
        "            if ($marker !== null) {\n",
        "                $marker->\n",
        "            }\n",
        "            if ($val > 10) {\n",
        "                $marker = new Marker();\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 10,
                    character: 26,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions for nested-if loop-carried assignment"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"position"),
                "Should include Marker's position() from nested-if loop-carried assignment, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When the cursor is inside a foreach body and the assignment is
/// before the cursor (same iteration), the existing behaviour should
/// still work — this is a regression guard.
#[tokio::test]
async fn test_loop_body_assignment_before_cursor_still_works() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b20_before_cursor.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Gadget {\n",
        "    public function activate(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /** @param list<int> $ids */\n",
        "    public function run(array $ids): void {\n",
        "        $g = null;\n",
        "        foreach ($ids as $id) {\n",
        "            $g = new Gadget();\n",
        "            $g->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 10,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should return completions when assignment is before cursor in loop"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                names.contains(&"activate"),
                "Should include Gadget's activate() when assignment is before cursor, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── __call return type fallback ────────────────────────────────────────────

/// When a class defines `__call` with a self-referencing return type
/// (`$this`, `static`, or `self`), calling an unknown method should
/// fall back to `__call`'s return type so the chain continues.
/// This is the pattern used by Eloquent Builder for dynamic `where{Column}`
/// calls.
#[tokio::test]
async fn test_completion_magic_call_self_return_preserves_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///magic_call_chain.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                           // 0
        "class Item {\n",                                                                    // 1
        "    public function save(): void {}\n",                                             // 2
        "    public function getTotal(): float { return 0.0; }\n",                           // 3
        "}\n",                                                                               // 4
        "class FluentBuilder {\n",                                                           // 5
        "    public function __call(string $name, array $args): static { return $this; }\n", // 6
        "    public function where(string $col, mixed $val): static { return $this; }\n",    // 7
        "    public function first(): Item { return new Item(); }\n",                        // 8
        "}\n",                                                                               // 9
        "class Runner {\n",                                                                  // 10
        "    public function run(): void {\n",                                               // 11
        "        $b = new FluentBuilder();\n",                                               // 12
        "        $b->where('active', true)->whereSomeDynamic(1)->first()->\n",               // 13
        "    }\n",                                                                           // 14
        "}\n",                                                                               // 15
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `->first()->` on line 13
    // "        $b->where('active', true)->whereSomeDynamic(1)->first()->"
    //  0         1         2         3         4         5         6
    //  0123456789012345678901234567890123456789012345678901234567890123456
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 66,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results after dynamic __call method in chain"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("save")),
                "Should include save from Item (resolved through __call chain), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getTotal")),
                "Should include getTotal from Item (resolved through __call chain), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `__call` returns `mixed`, the fallback should not produce any
/// class completions (same as before the fallback was added).
#[tokio::test]
async fn test_completion_magic_call_mixed_return_no_completions() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///magic_call_mixed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dynamic {\n",
        "    public function __call(string $name, array $args): mixed { return null; }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $d = new Dynamic();\n",
        "        $d->anything()->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // mixed return type should not produce class-member completions
    if let Some(CompletionResponse::Array(items)) = result {
        let method_items: Vec<_> = items
            .iter()
            .filter(|i| {
                i.kind == Some(CompletionItemKind::METHOD)
                    || i.kind == Some(CompletionItemKind::PROPERTY)
            })
            .collect();
        assert!(
            method_items.is_empty(),
            "Should not offer method/property completions when __call returns mixed, got: {:?}",
            method_items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }
}

/// When `__call` returns a concrete class type, calling an unknown method
/// should resolve to that class's members.
#[tokio::test]
async fn test_completion_magic_call_concrete_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///magic_call_concrete.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Result {\n",
        "    public function getData(): array { return []; }\n",
        "    public function isOk(): bool { return true; }\n",
        "}\n",
        "class Proxy {\n",
        "    public function __call(string $name, array $args): Result { return new Result(); }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $p = new Proxy();\n",
        "        $p->anyMethod()->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
                character: 26,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results when __call returns a concrete class"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getData")),
                "Should include getData from Result via __call return type, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("isOk")),
                "Should include isOk from Result via __call return type, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// __call fallback should work when __call is inherited from a parent class.
#[tokio::test]
async fn test_completion_magic_call_inherited_self_return() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///magic_call_inherited.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class BaseBuilder {\n",
        "    public function __call(string $name, array $args): static { return $this; }\n",
        "    public function execute(): array { return []; }\n",
        "}\n",
        "class ChildBuilder extends BaseBuilder {\n",
        "    public function limit(int $n): static { return $this; }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $b = new ChildBuilder();\n",
        "        $b->dynamicWhere(1)->limit(5)->anotherDynamic(2)->execute();\n",
        "        $b->dynamicWhere(1)->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$b->dynamicWhere(1)->` on line 12
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 29,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results after inherited __call"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("limit")),
                "Should include limit from ChildBuilder via inherited __call, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("execute")),
                "Should include execute from BaseBuilder via inherited __call, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Variable assigned from a dynamic __call method should also resolve.
#[tokio::test]
async fn test_completion_magic_call_variable_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///magic_call_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function save(): void {}\n",
        "}\n",
        "class FluentBuilder {\n",
        "    public function __call(string $name, array $args): static { return $this; }\n",
        "    public function first(): Item { return new Item(); }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $b = new FluentBuilder();\n",
        "        $filtered = $b->whereDynamic(1);\n",
        "        $item = $filtered->first();\n",
        "        $item->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for variable assigned through __call chain"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("save")),
                "Should include save from Item via __call chain assignment, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a `@method` tag has no return type and the class defines `__call`
/// with a typed return, the `@method` should inherit `__call`'s return type.
#[tokio::test]
async fn test_completion_magic_call_method_tag_inherits_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tag_inherits_call.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @method whereStatus(string $status)\n",
        " * @method string getName()\n",
        " */\n",
        "class QueryBuilder {\n",
        "    public function __call(string $name, array $args): static { return $this; }\n",
        "    public function get(): array { return []; }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $b = new QueryBuilder();\n",
        "        $b->whereStatus('active')->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$b->whereStatus('active')->` on line 12
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 37,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for @method without return type inheriting from __call"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // whereStatus has no return type in @method, should inherit
            // static from __call, so chain continues with QueryBuilder members
            assert!(
                labels.iter().any(|l| l.starts_with("get")),
                "Should include get from QueryBuilder (inherited __call return type), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("whereStatus")),
                "Should include whereStatus from @method tag, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a `@method` tag declares a return type that contradicts
/// `__call`'s native return type (`static`), the native type wins.
/// `@method Result fetch()` is a lie when `__call` natively returns
/// `static`, because `Result` does not extend `ApiClient`.  The
/// chain resolves to `ApiClient` (via `static`) instead.
#[tokio::test]
async fn test_completion_magic_call_method_tag_own_return_type_wins() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tag_own_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Result {\n",
        "    public function getData(): array { return []; }\n",
        "}\n",
        "/**\n",
        " * @method Result fetch()\n",
        " */\n",
        "class ApiClient {\n",
        "    public function __call(string $name, array $args): static { return $this; }\n",
        "    public function status(): int { return 200; }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $c = new ApiClient();\n",
        "        $c->fetch()->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$c->fetch()->` on line 14
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 14,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return ApiClient members (native static wins over @method lie)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // @method Result is a lie: Result does not extend ApiClient,
            // so __call's native `static` wins → ApiClient members.
            assert!(
                labels.iter().any(|l| l.starts_with("fetch")),
                "Should include fetch from ApiClient (__call static wins), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("status")),
                "Should include status from ApiClient (__call static wins), got: {:?}",
                labels
            );
            assert!(
                !labels.iter().any(|l| l.starts_with("getData")),
                "Should NOT include getData from Result (the @method lied), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a `@method` tag declares a return type that contradicts
/// `__call`'s native return type, the native type wins ("lie detection").
/// Here `__call` natively returns `Cement`, so `@method Frog getThing()`
/// is rejected and the chain resolves to `Cement` members instead.
#[tokio::test]
async fn test_completion_magic_call_method_tag_lie_detection_native_wins() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tag_lie.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Frog {\n",
        "    public function ribbit(): void {}\n",
        "}\n",
        "class Cement {\n",
        "    public function harden(): void {}\n",
        "    public function pour(): void {}\n",
        "}\n",
        "/**\n",
        " * @method Frog getThing()\n",
        " */\n",
        "class Factory {\n",
        "    public function __call(string $name, array $args): Cement { return new Cement(); }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $f = new Factory();\n",
        "        $f->getThing()->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$f->getThing()->` on line 17
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 17,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results when @method lies about return type"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // __call natively returns Cement — @method Frog is a lie.
            // We should see Cement members, not Frog members.
            assert!(
                labels.iter().any(|l| l.starts_with("harden")),
                "Should include harden from Cement (__call native type wins over @method lie), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("pour")),
                "Should include pour from Cement, got: {:?}",
                labels
            );
            assert!(
                !labels.iter().any(|l| l.starts_with("ribbit")),
                "Should NOT include ribbit from Frog (the @method lied), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `__call` has no native return type but has a docblock `@return`,
/// `@method` return types are trusted because there is no runtime
/// constraint to contradict them.
#[tokio::test]
async fn test_completion_magic_call_no_native_type_trusts_method_tag() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tag_no_native.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Result {\n",
        "    public function getData(): array { return []; }\n",
        "}\n",
        "/**\n",
        " * @method Result fetch()\n",
        " */\n",
        "class ApiClient {\n",
        "    /** @return static */\n",
        "    public function __call(string $name, array $args) { return $this; }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $c = new ApiClient();\n",
        "        $c->fetch()->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$c->fetch()->` on line 14
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 14,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return Result members when __call has no native type"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // __call has no native type hint — @method Result is trusted
            assert!(
                labels.iter().any(|l| l.starts_with("getData")),
                "Should include getData from Result (@method trusted when no native type), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `@method` declares a return type that is a valid narrowing of
/// `__call`'s native type (e.g. subclass), the narrowed type is used.
#[tokio::test]
async fn test_completion_magic_call_method_tag_valid_narrowing() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tag_narrow.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function breathe(): void {}\n",
        "}\n",
        "class Dog extends Animal {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "/**\n",
        " * @method Dog getDog()\n",
        " */\n",
        "class Kennel {\n",
        "    public function __call(string $name, array $args): Animal { return new Animal(); }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        $k = new Kennel();\n",
        "        $k->getDog()->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$k->getDog()->` on line 16
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 16,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return Dog members when @method narrows __call's return type"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Dog extends Animal, so @method Dog is a valid narrowing
            // of __call's native return type Animal.
            assert!(
                labels.iter().any(|l| l.starts_with("bark")),
                "Should include bark from Dog (valid narrowing of Animal), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("breathe")),
                "Should include breathe inherited from Animal, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `__callStatic` fallback works the same way as `__call` for static calls.
#[tokio::test]
async fn test_completion_magic_call_static_fallback() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///call_static_fallback.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Result {\n",
        "    public function getData(): array { return []; }\n",
        "}\n",
        "class Facade {\n",
        "    public static function __callStatic(string $name, array $args): Result { return new Result(); }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(): void {\n",
        "        Facade::anyMethod()->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `Facade::anyMethod()->` on line 9
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 29,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results via __callStatic fallback"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getData")),
                "Should include getData from Result via __callStatic, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Foreach over a variadic parameter with a class type resolves the
/// loop variable to the element type.
#[tokio::test]
async fn test_completion_foreach_variadic_param_class_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///variadic_foreach.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class HtmlString {\n",
        "    public function toHtml(): string { return ''; }\n",
        "}\n",
        "class Formatter {\n",
        "    public function format(HtmlString ...$items): void {\n",
        "        foreach ($items as $value) {\n",
        "            $value->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach value from variadic param"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("toHtml")),
                "Should include toHtml method from HtmlString, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Foreach over a variadic parameter with a union type containing a
/// class resolves the loop variable so that instanceof narrowing can
/// work on it.
#[tokio::test]
async fn test_completion_foreach_variadic_param_union_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///variadic_union_foreach.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class HtmlString {\n",
        "    public function toHtml(): string { return ''; }\n",
        "}\n",
        "class ShortTexts {\n",
        "    public static function get(int $id, HtmlString|int|string ...$placeholders): void {\n",
        "        foreach ($placeholders as $value) {\n",
        "            if ($value instanceof HtmlString) {\n",
        "                $value->\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results after instanceof narrowing on variadic foreach value"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("toHtml")),
                "Should include toHtml from HtmlString after instanceof narrowing, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Foreach over a variadic parameter in a standalone function (not a
/// method) resolves the loop variable to the element type.
#[tokio::test]
async fn test_completion_foreach_variadic_param_standalone_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///variadic_func_foreach.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public string $name;\n",
        "    public function render(): string { return ''; }\n",
        "}\n",
        "function processWidgets(Widget ...$widgets): void {\n",
        "    foreach ($widgets as $w) {\n",
        "        $w->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for foreach value from variadic function param"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("name")),
                "Should include name property from Widget, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("render")),
                "Should include render method from Widget, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Foreach variable type from @param array shape ──────────────────────────

/// When a parameter is annotated with `@param array<int, array{bundle: Product, count: int}>`,
/// iterating over it with foreach should propagate the value type so that
/// `$entry['bundle']->` resolves to `Product`.
#[tokio::test]
async fn test_foreach_variable_type_from_param_array_shape() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_param_shape.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                    // 0
        "class Product {\n",                                                          // 1
        "    public string $sku;\n",                                                  // 2
        "    public function parentProduct(): Product { return new self(); }\n",      // 3
        "}\n",                                                                        // 4
        "\n",                                                                         // 5
        "class Listener {\n",                                                         // 6
        "    /** @param array<int, array{bundle: Product, count: int}> $counts */\n", // 7
        "    public function handle(array $counts): void {\n",                        // 8
        "        foreach ($counts as $entry) {\n",                                    // 9
        "            $entry['bundle']->\n",                                           // 10
        "        }\n",                                                                // 11
        "    }\n",                                                                    // 12
        "}\n",                                                                        // 13
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor at line 10 after `$entry['bundle']->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 34,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $entry['bundle']-> inside foreach over array shape param"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("sku")),
                "Should include 'sku' from Product via array shape foreach. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("parentProduct")),
                "Should include 'parentProduct' from Product via array shape foreach. Got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Deep variable chain tests ──────────────────────────────────────────────

/// Deep variable assignment chain inside a method body: each variable
/// depends on the previous one.  The recursive resolution walks
/// backwards through the chain (depth 5) to resolve the final type.
#[tokio::test]
async fn test_completion_deep_variable_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///deep_var_chain.php").unwrap();
    // 5-level chain: param $a → $b → $c → $d → $e → $final
    let text = concat!(
        "<?php\n",                                                                 // 0
        "class DeepA {\n",                                                         // 1
        "    public function getB(): DeepB { return new DeepB(); }\n",             // 2
        "}\n",                                                                     // 3
        "class DeepB {\n",                                                         // 4
        "    public function getC(): DeepC { return new DeepC(); }\n",             // 5
        "}\n",                                                                     // 6
        "class DeepC {\n",                                                         // 7
        "    public function getD(): DeepD { return new DeepD(); }\n",             // 8
        "}\n",                                                                     // 9
        "class DeepD {\n",                                                         // 10
        "    public function getE(): DeepE { return new DeepE(); }\n",             // 11
        "}\n",                                                                     // 12
        "class DeepE {\n",                                                         // 13
        "    public function getFinal(): DeepFinal { return new DeepFinal(); }\n", // 14
        "}\n",                                                                     // 15
        "class DeepFinal {\n",                                                     // 16
        "    public function deepResult(): string { return ''; }\n",               // 17
        "}\n",                                                                     // 18
        "class DeepChainTest {\n",                                                 // 19
        "    public function test(DeepA $a) {\n",                                  // 20
        "        $b = $a->getB();\n",                                              // 21
        "        $c = $b->getC();\n",                                              // 22
        "        $d = $c->getD();\n",                                              // 23
        "        $e = $d->getE();\n",                                              // 24
        "        $final = $e->getFinal();\n",                                      // 25
        "        $final->\n",                                                      // 26
        "    }\n",                                                                 // 27
        "}\n",                                                                     // 28
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 26,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Deep variable chain (5 levels) should resolve"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"deepResult"),
                "Should include 'deepResult' from DeepFinal at end of 5-level chain. Got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// 10-level variable assignment chain. Regression guard for the elimination of the old
/// `MAX_VAR_RESOLUTION_DEPTH=4` limit. Each level returns a different class, so the
/// chain is only fully resolved if there is no artificial depth cap.
#[tokio::test]
async fn test_completion_very_deep_variable_chain_no_depth_limit() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///very_deep_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Level1 { public function next(): Level2 { return new Level2(); } }\n",
        "class Level2 { public function next(): Level3 { return new Level3(); } }\n",
        "class Level3 { public function next(): Level4 { return new Level4(); } }\n",
        "class Level4 { public function next(): Level5 { return new Level5(); } }\n",
        "class Level5 { public function next(): Level6 { return new Level6(); } }\n",
        "class Level6 { public function next(): Level7 { return new Level7(); } }\n",
        "class Level7 { public function next(): Level8 { return new Level8(); } }\n",
        "class Level8 { public function next(): Level9 { return new Level9(); } }\n",
        "class Level9 { public function next(): Level10 { return new Level10(); } }\n",
        "class Level10 { public function finalMethod(): string { return ''; } }\n",
        "class DeepChainTest {\n",
        "    public function test(): void {\n",
        "        $a = new Level1();\n",
        "        $b = $a->next();\n",
        "        $c = $b->next();\n",
        "        $d = $c->next();\n",
        "        $e = $d->next();\n",
        "        $f = $e->next();\n",
        "        $g = $f->next();\n",
        "        $h = $g->next();\n",
        "        $i = $h->next();\n",
        "        $j = $i->next();\n",
        "        $j->\n",
        "    }\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 23,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "10-level variable chain should resolve without hitting a depth limit"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"finalMethod"),
                "Should include 'finalMethod' from Level10 at end of 10-level chain. Got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Deep variable chain in a standalone function (not inside a class).
/// Verifies that function-parameter types are picked up and chained
/// through intermediate variable assignments.
#[tokio::test]
async fn test_completion_deep_variable_chain_standalone_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///deep_func_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ChainStart {\n",
        "    public function step1(): ChainMid { return new ChainMid(); }\n",
        "}\n",
        "class ChainMid {\n",
        "    public function step2(): ChainEnd { return new ChainEnd(); }\n",
        "}\n",
        "class ChainEnd {\n",
        "    public function finalValue(): int { return 0; }\n",
        "}\n",
        "function processChain(ChainStart $start) {\n",
        "    $mid = $start->step1();\n",
        "    $end = $mid->step2();\n",
        "    $end->\n",
        "}\n",
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 10,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Variable chain in standalone function should resolve"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"finalValue"),
                "Should include 'finalValue' from ChainEnd. Got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Multiple parameters referenced in assignment chains: `$a` and `$b`
/// are both params, and both feed into separate variables that are used
/// later.  Both parameter types should be available for RHS resolution
/// when resolving the derived variables.
#[tokio::test]
async fn test_completion_cross_parameter_reference_in_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///cross_param.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                     // 0
        "class ParamA {\n",                                                            // 1
        "    public function toResult(): ParamResult { return new ParamResult(); }\n", // 2
        "}\n",                                                                         // 3
        "class ParamB {\n",                                                            // 4
        "    public function toOther(): ParamOther { return new ParamOther(); }\n",    // 5
        "}\n",                                                                         // 6
        "class ParamResult {\n",                                                       // 7
        "    public function output(): void {}\n",                                     // 8
        "}\n",                                                                         // 9
        "class ParamOther {\n",                                                        // 10
        "    public function info(): string { return ''; }\n",                         // 11
        "}\n",                                                                         // 12
        "class CrossParamTest {\n",                                                    // 13
        "    public function test(ParamA $a, ParamB $b) {\n",                          // 14
        "        $fromA = $a->toResult();\n",                                          // 15
        "        $fromB = $b->toOther();\n",                                           // 16
        "        $fromA->\n",                                                          // 17
        "    }\n",                                                                     // 18
        "}\n",                                                                         // 19
    );

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
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 17,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Cross-parameter reference chain should resolve"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            assert!(
                names.contains(&"output"),
                "Should include 'output' from ParamResult. Got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_falsy_narrowing_after_throw_guard() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///falsy_guard.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class XmlNode {\n",
        "    public function children(): array { return []; }\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Factory {\n",
        "    /** @return XmlNode|false */\n",
        "    public static function load(string $s) { return new XmlNode(); }\n",
        "    function test(): void {\n",
        "        $xml = self::load('<root/>');\n",
        "        if (!$xml) {\n",
        "            throw new \\RuntimeException('fail');\n",
        "        }\n",
        "        $xml->\n",
        "    }\n",
        "}\n",
    );

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
                position: Position {
                    line: 13,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Completion should return results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // After `if (!$xml) { throw; }`, $xml should be narrowed
            // to XmlNode (false removed), so its methods are available.
            assert!(
                method_names.contains(&"children"),
                "Should include 'children' from XmlNode after falsy guard, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getName"),
                "Should include 'getName' from XmlNode after falsy guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_null_coalescing_with_throw_narrows_nullable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_null_coalesce_throw.php").unwrap();

    let php = r#"<?php
class Item { public function getName(): string { return ''; } }
/** @return Item|null */
function findItem(): ?Item { return new Item(); }
$item = findItem() ?? throw new \Exception();
$item->
"#;

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: php.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 5,
                    character: 7,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Completion should return results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "After ?? throw, $item should be narrowed to Item (not nullable) and include 'getName', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}
