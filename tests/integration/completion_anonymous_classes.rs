use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Basic anonymous class completion ───────────────────────────────────────

/// `$this->` inside an anonymous class body should resolve to the
/// anonymous class's own members.
#[tokio::test]
async fn test_completion_this_inside_anonymous_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_class_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$handler = new class {\n",
        "    public string $name;\n",
        "    public function greet(): string { return ''; }\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "};\n",
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
                    line: 5,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"greet"),
                "Should include anonymous class method 'greet', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"test"),
                "Should include anonymous class method 'test', got: {:?}",
                method_names
            );
            assert!(
                prop_names.contains(&"name"),
                "Should include anonymous class property 'name', got: {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class with extends ───────────────────────────────────────────

/// An anonymous class that extends a named class should inherit the
/// parent's members through the normal inheritance chain.
#[tokio::test]
async fn test_completion_anonymous_class_extends_parent() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_extends.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class BaseHandler {\n",
        "    public function baseMethod(): string { return ''; }\n",
        "}\n",
        "$handler = new class extends BaseHandler {\n",
        "    public function handle(): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "};\n",
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

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"handle"),
                "Should include own method 'handle', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"baseMethod"),
                "Should include inherited method 'baseMethod', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class with implements ────────────────────────────────────────

/// An anonymous class implementing an interface should include interface
/// method stubs (or at least inherit the interface contract through the
/// merge logic).
#[tokio::test]
async fn test_completion_anonymous_class_implements_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_implements.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Renderable {\n",
        "    public function render(): string;\n",
        "}\n",
        "$widget = new class implements Renderable {\n",
        "    public function render(): string { return ''; }\n",
        "    public function extra(): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "};\n",
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
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"render"),
                "Should include 'render' method, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"extra"),
                "Should include 'extra' method, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class with trait ─────────────────────────────────────────────

/// An anonymous class that uses a trait should see the trait's members
/// through `$this->`.
#[tokio::test]
async fn test_completion_anonymous_class_uses_trait() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_trait.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Loggable {\n",
        "    public function log(): void {}\n",
        "}\n",
        "$worker = new class {\n",
        "    use Loggable;\n",
        "    public function run(): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "};\n",
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
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"log"),
                "Should include trait method 'log', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"run"),
                "Should include own method 'run', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class nested inside a named class method ─────────────────────

/// When an anonymous class is created inside a named class's method,
/// `$this->` should resolve to the anonymous class (innermost scope),
/// not the outer named class.
#[tokio::test]
async fn test_completion_anonymous_class_inside_named_class_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_nested.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Outer {\n",
        "    public function outerMethod(): void {}\n",
        "    public function factory() {\n",
        "        return new class {\n",
        "            public function innerMethod(): void {}\n",
        "            public function test() {\n",
        "                $this->\n",
        "            }\n",
        "        };\n",
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
                    character: 23,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"innerMethod"),
                "Should include anonymous class's own 'innerMethod', got: {:?}",
                method_names
            );
            // The outer class's method should NOT appear because $this
            // refers to the anonymous class, not the outer class.
            assert!(
                !method_names.contains(&"outerMethod"),
                "Should NOT include outer class's 'outerMethod', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class extends cross-file parent ──────────────────────────────

/// An anonymous class that extends a parent in a different file should
/// inherit the parent's members via PSR-4 resolution.
#[tokio::test]
async fn test_completion_anonymous_class_extends_cross_file() {
    let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
    let base_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "class BaseHandler {\n",
        "    public function baseMethod(): string { return ''; }\n",
        "    protected function helperMethod(): void {}\n",
        "}\n",
    );
    let main_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "$handler = new class extends BaseHandler {\n",
        "    public function handle(): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "};\n",
    );

    let (backend, dir) = create_psr4_workspace(
        composer,
        &[
            ("src/BaseHandler.php", base_file),
            ("src/main.php", main_file),
        ],
    );

    let uri = Url::from_file_path(dir.path().join("src/main.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: main_file.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
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
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"handle"),
                "Should include own method 'handle', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"baseMethod"),
                "Should include inherited 'baseMethod', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"helperMethod"),
                "Should include inherited protected 'helperMethod', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class in control flow ────────────────────────────────────────

/// Anonymous classes inside if/else blocks, loops, try/catch, etc. should
/// still be found by the parser.
#[tokio::test]
async fn test_completion_anonymous_class_in_if_block() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_in_if.php").unwrap();
    let text = concat!(
        "<?php\n",
        "if (true) {\n",
        "    $handler = new class {\n",
        "        public function ifMethod(): void {}\n",
        "        public function test() {\n",
        "            $this->\n",
        "        }\n",
        "    };\n",
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
                    line: 5,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"ifMethod"),
                "Should include 'ifMethod' from anonymous class inside if block, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_anonymous_class_in_try_catch() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_in_try.php").unwrap();
    let text = concat!(
        "<?php\n",
        "try {\n",
        "    $handler = new class {\n",
        "        public function tryMethod(): void {}\n",
        "        public function test() {\n",
        "            $this->\n",
        "        }\n",
        "    };\n",
        "} catch (Exception $e) {}\n",
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
                    line: 5,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"tryMethod"),
                "Should include 'tryMethod' from anonymous class inside try block, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_anonymous_class_in_foreach() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_in_foreach.php").unwrap();
    let text = concat!(
        "<?php\n",
        "foreach ([1, 2, 3] as $item) {\n",
        "    $handler = new class {\n",
        "        public function loopMethod(): void {}\n",
        "        public function test() {\n",
        "            $this->\n",
        "        }\n",
        "    };\n",
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
                    line: 5,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"loopMethod"),
                "Should include 'loopMethod' from anonymous class inside foreach, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class as function argument ───────────────────────────────────

/// Anonymous classes passed as function arguments should also be found.
#[tokio::test]
async fn test_completion_anonymous_class_as_function_argument() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_func_arg.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function process($handler) {}\n",
        "process(new class {\n",
        "    public function argMethod(): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "});\n",
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
                    line: 5,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"argMethod"),
                "Should include 'argMethod' from anonymous class passed as function arg, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class returned from function ─────────────────────────────────

/// Anonymous classes in return statements should be found.
#[tokio::test]
async fn test_completion_anonymous_class_in_return_statement() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function factory() {\n",
        "    return new class {\n",
        "        public function returnedMethod(): void {}\n",
        "        public function test() {\n",
        "            $this->\n",
        "        }\n",
        "    };\n",
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
                    line: 5,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"returnedMethod"),
                "Should include 'returnedMethod' from anonymous class in return, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class in closure ─────────────────────────────────────────────

/// Anonymous classes inside closures should be detected.
#[tokio::test]
async fn test_completion_anonymous_class_in_closure() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_in_closure.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$fn = function() {\n",
        "    return new class {\n",
        "        public function closureMethod(): void {}\n",
        "        public function test() {\n",
        "            $this->\n",
        "        }\n",
        "    };\n",
        "};\n",
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
                    line: 5,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"closureMethod"),
                "Should include 'closureMethod' from anonymous class in closure, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class with constructor promoted properties ───────────────────

/// Constructor-promoted properties should be available on anonymous
/// classes just like on named classes.
#[tokio::test]
async fn test_completion_anonymous_class_promoted_properties() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_promoted.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$obj = new class('hello') {\n",
        "    public function __construct(\n",
        "        public string $message,\n",
        "        private int $count = 0,\n",
        "    ) {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "};\n",
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

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                prop_names.contains(&"message"),
                "Should include promoted property 'message', got: {:?}",
                prop_names
            );
            assert!(
                prop_names.contains(&"count"),
                "Should include promoted property 'count', got: {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class inside switch ──────────────────────────────────────────

#[tokio::test]
async fn test_completion_anonymous_class_in_switch() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_in_switch.php").unwrap();
    let text = concat!(
        "<?php\n",
        "switch (true) {\n",
        "    case true:\n",
        "        $handler = new class {\n",
        "            public function switchMethod(): void {}\n",
        "            public function test() {\n",
        "                $this->\n",
        "            }\n",
        "        };\n",
        "        break;\n",
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
                    character: 23,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"switchMethod"),
                "Should include 'switchMethod' from anonymous class inside switch, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Outer class completion still works ─────────────────────────────────────

/// Verify that the outer named class's `$this->` still works correctly
/// when the file also contains an anonymous class.
#[tokio::test]
async fn test_completion_outer_class_not_affected_by_anonymous_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_outer_ok.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Outer {\n",
        "    public function outerOnly(): void {}\n",
        "    public function factory() {\n",
        "        return new class {\n",
        "            public function innerOnly(): void {}\n",
        "        };\n",
        "    }\n",
        "    public function test() {\n",
        "        $this->\n",
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
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"outerOnly"),
                "Outer class should still see its own 'outerOnly', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"factory"),
                "Outer class should still see its own 'factory', got: {:?}",
                method_names
            );
            // The anonymous class's members should NOT leak into the outer class.
            assert!(
                !method_names.contains(&"innerOnly"),
                "Outer class should NOT see anonymous class's 'innerOnly', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class with namespace (update_ast path) ───────────────────────

/// Tests the `update_ast` code path for anonymous classes in namespaced files.
/// The `update_ast_inner` method handles class extraction differently from
/// `parse_php`, so anonymous classes need to be found in both paths.
#[tokio::test]
async fn test_completion_anonymous_class_in_namespace() {
    let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
    let file = concat!(
        "<?php\n",
        "namespace App;\n",
        "$handler = new class {\n",
        "    public function nsMethod(): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "};\n",
    );

    let (backend, dir) = create_psr4_workspace(composer, &[("src/handler.php", file)]);

    let uri = Url::from_file_path(dir.path().join("src/handler.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: file.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
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
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"nsMethod"),
                "Should include 'nsMethod' from anonymous class in namespace, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Multiple anonymous classes in one file ─────────────────────────────────

/// When a file has multiple anonymous classes, `$this->` inside each one
/// should resolve to the correct anonymous class.
#[tokio::test]
async fn test_completion_multiple_anonymous_classes_in_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_multiple.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$first = new class {\n",
        "    public function firstMethod(): void {}\n",
        "};\n",
        "$second = new class {\n",
        "    public function secondMethod(): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "};\n",
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

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"secondMethod"),
                "Should include second anonymous class's 'secondMethod', got: {:?}",
                method_names
            );
            // First anonymous class's method should NOT appear in the second.
            assert!(
                !method_names.contains(&"firstMethod"),
                "Should NOT include first anonymous class's 'firstMethod', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Anonymous class with docblock property types ───────────────────────────

/// `@var` annotations on anonymous class properties should work for
/// chained completion just like on named classes.
#[tokio::test]
async fn test_completion_anonymous_class_property_docblock_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_prop_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "}\n",
        "$handler = new class {\n",
        "    /** @var Logger */\n",
        "    public $logger;\n",
        "    public function test() {\n",
        "        $this->logger->\n",
        "    }\n",
        "};\n",
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
                    character: 23,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"info"),
                "Should resolve @var type on anonymous class property and show Logger::info, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Go-to-definition for anonymous class members ───────────────────────────

/// Go-to-definition on a method inside an anonymous class should resolve
/// to the method's position within the anonymous class body.
#[tokio::test]
async fn test_goto_definition_anonymous_class_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_goto.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$handler = new class {\n",
        "    public function handle(): void {}\n",
        "    public function test() {\n",
        "        $this->handle();\n",
        "    }\n",
        "};\n",
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

    // Go-to-definition on `handle` (line 4, around character 18)
    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 4,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should resolve go-to-definition for method in anonymous class"
    );
    if let Some(GotoDefinitionResponse::Scalar(location)) = result {
        assert_eq!(location.uri, uri, "Should point to the same file");
        assert_eq!(
            location.range.start.line, 2,
            "Should jump to the handle method definition on line 2"
        );
    } else {
        panic!("Expected GotoDefinitionResponse::Scalar");
    }
}

// ─── Display name ───────────────────────────────────────────────────────────

/// The completion detail should show "anonymous class" rather than the
/// raw synthetic name like `__anonymous@156`.
#[tokio::test]
async fn test_completion_anonymous_class_detail_shows_friendly_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_detail.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$obj = new class {\n",
        "    public string $name;\n",
        "    public function greet(): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "};\n",
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
                    line: 5,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            // Check a method item — detail is now None (no class prefix),
            // and the raw `__anonymous@` name must not leak into any
            // user-visible field.
            let method_item = items
                .iter()
                .find(|i| {
                    i.kind == Some(CompletionItemKind::METHOD)
                        && i.filter_text.as_deref() == Some("greet")
                })
                .expect("Should find 'greet' method");
            assert!(
                !method_item
                    .detail
                    .as_deref()
                    .unwrap_or("")
                    .contains("__anonymous@"),
                "Method detail should NOT contain raw synthetic name, got: {:?}",
                method_item.detail
            );
            assert!(
                !method_item
                    .label_details
                    .as_ref()
                    .and_then(|ld| ld.description.as_deref())
                    .unwrap_or("")
                    .contains("__anonymous@"),
                "Method label_details should NOT contain raw synthetic name, got: {:?}",
                method_item.label_details
            );

            // Check a property item — detail shows the type hint, not the class.
            let prop_item = items
                .iter()
                .find(|i| {
                    i.kind == Some(CompletionItemKind::PROPERTY)
                        && i.filter_text.as_deref() == Some("name")
                })
                .expect("Should find 'name' property");
            assert_eq!(
                prop_item.detail.as_deref(),
                Some("string"),
                "Typed property detail should show the type hint"
            );
            assert!(
                !prop_item
                    .detail
                    .as_deref()
                    .unwrap_or("")
                    .contains("__anonymous@"),
                "Property detail should NOT contain raw synthetic name, got: {:?}",
                prop_item.detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Closure param type hint inside anonymous class method ──────────────────

/// When a closure with an explicitly typed parameter is passed as an argument
/// to a static method call inside an anonymous class method, the closure
/// parameter's type hint should resolve correctly.  This is the pattern used
/// by Laravel migrations: `return new class extends Migration { ... }`.
#[tokio::test]
async fn test_closure_param_type_hint_in_anonymous_class_static_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_closure_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Blueprint {\n",
        "    public function enum(string $column): static { return $this; }\n",
        "    public function after(string $column): static { return $this; }\n",
        "    public function default(mixed $value): static { return $this; }\n",
        "    public function dropColumn(string $column): void {}\n",
        "}\n",
        "class Migration {\n",
        "    public function up(): void {}\n",
        "    public function down(): void {}\n",
        "}\n",
        "class Schema {\n",
        "    public static function table(string $table, \\Closure $callback): void {}\n",
        "}\n",
        "return new class extends Migration {\n",
        "    public function up(): void {\n",
        "        Schema::table('orders', function (Blueprint $table): void {\n",
        "            $table->\n",
        "        });\n",
        "    }\n",
        "};\n",
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

    // Cursor right after `$table->` on line 17
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 17,
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
        "Completion should resolve $table via closure parameter type hint in anonymous class"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("enum")),
                "Should include enum method from Blueprint, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("dropColumn")),
                "Should include dropColumn method from Blueprint, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Two files whose anonymous class opens at the same byte offset must not
/// share one resolution.  Boilerplate-identical files make this ordinary:
/// every Laravel migration is `return new class extends Migration {` after
/// the same header, so they all put the brace at the same offset.
#[tokio::test]
async fn test_anonymous_classes_at_same_offset_in_two_files_do_not_collide() {
    let backend = create_test_backend();

    let first = Url::parse("file:///anon_offset_first.php").unwrap();
    let second = Url::parse("file:///anon_offset_second.php").unwrap();

    // Identical up to the method name, so both left braces land on the
    // same offset.
    let text = |method: &str| {
        format!(
            concat!(
                "<?php\n",
                "$a = new class {{\n",
                "    public function {}(): string {{ return ''; }}\n",
                "}};\n",
                "$a->\n",
            ),
            method
        )
    };

    for (uri, method) in [(&first, "alpha"), (&second, "betaa")] {
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "php".to_string(),
                    version: 1,
                    text: text(method),
                },
            })
            .await;
    }

    for (uri, own, other) in [(&first, "alpha", "betaa"), (&second, "betaa", "alpha")] {
        let result = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position {
                        line: 4,
                        character: 4,
                    },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .unwrap();

        let CompletionResponse::Array(items) = result.expect("completion results") else {
            panic!("Expected CompletionResponse::Array");
        };
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.starts_with(own)),
            "{uri} should offer its own {own}(), got: {labels:?}"
        );
        assert!(
            !labels.iter().any(|l| l.starts_with(other)),
            "{uri} must not offer the other file's {other}(), got: {labels:?}"
        );
    }
}

/// An edit that leaves the anonymous class's opening brace on the same
/// offset keeps its synthetic name, so the resolved-class cache entry from
/// the previous parse has to be evicted or it answers for the old body.
#[tokio::test]
async fn test_anonymous_class_edited_in_place_is_not_served_from_cache() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///anon_edit_in_place.php").unwrap();
    let text = |method: &str| {
        format!(
            concat!(
                "<?php\n",
                "$a = new class {{\n",
                "    public function {}(): string {{ return ''; }}\n",
                "}};\n",
                "$a->\n",
            ),
            method
        )
    };

    let complete = async |version: i32| {
        backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position {
                        line: 4,
                        character: 4,
                    },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .unwrap()
            .map(|r| match r {
                CompletionResponse::Array(items) => {
                    items.into_iter().map(|i| i.label).collect::<Vec<_>>()
                }
                _ => panic!("Expected CompletionResponse::Array"),
            })
            .unwrap_or_else(|| panic!("completion results for version {version}"))
    };

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text("alpha"),
            },
        })
        .await;

    // Resolve once so the anonymous class lands in the resolved-class cache.
    let before = complete(1).await;
    assert!(
        before.iter().any(|l| l.starts_with("alpha")),
        "should offer alpha() before the edit, got: {before:?}"
    );

    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text("betaa"),
            }],
        })
        .await;

    let after = complete(2).await;
    assert!(
        after.iter().any(|l| l.starts_with("betaa")),
        "should offer the renamed betaa() after the edit, got: {after:?}"
    );
    assert!(
        !after.iter().any(|l| l.starts_with("alpha")),
        "must not still offer the pre-edit alpha(), got: {after:?}"
    );
}
