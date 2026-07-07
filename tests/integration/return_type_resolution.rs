use crate::common::create_test_backend;
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Function Return Type Resolution ────────────────────────────────────────

/// Test: `app()->abort()` — function call return type used to resolve member.
/// The function `app()` returns `Application`, so clicking on `abort` should
/// jump to the `abort` method on the `Application` class.
#[tokio::test]
async fn test_goto_definition_function_return_type_method_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Application {\n",
        "    public function abort(int $code): void {}\n",
        "    public function make(string $class): object {}\n",
        "}\n",
        "\n",
        "function app(): Application {\n",
        "    return new Application();\n",
        "}\n",
        "\n",
        "class Controller {\n",
        "    public function handle(): void {\n",
        "        app()->abort(404);\n",
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

    // Click on "abort" in `app()->abort(404)` on line 12
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve app()->abort via function return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "function abort is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Method Return Type Chain Resolution ────────────────────────────────────

/// Test: `$this->getConnection()->query()` — method call chain where
/// getConnection() returns Connection class.
#[tokio::test]
async fn test_goto_definition_method_return_type_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Connection {\n",
        "    public function query(string $sql): void {}\n",
        "    public function beginTransaction(): void {}\n",
        "}\n",
        "\n",
        "class Database {\n",
        "    public function getConnection(): Connection {\n",
        "        return new Connection();\n",
        "    }\n",
        "\n",
        "    public function run(): void {\n",
        "        $this->getConnection()->query('SELECT 1');\n",
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

    // Click on "query" in `$this->getConnection()->query(...)` on line 12
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 33,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->getConnection()->query via method return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "function query is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Static Method Return Type Chain Resolution ─────────────────────────────

/// Test: `Model::query()->where()` — static method call chain where
/// query() returns a Builder instance.
#[tokio::test]
async fn test_goto_definition_static_method_return_type_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function where(string $col): self { return $this; }\n",
        "    public function get(): array { return []; }\n",
        "}\n",
        "\n",
        "class Model {\n",
        "    public static function query(): Builder {\n",
        "        return new Builder();\n",
        "    }\n",
        "\n",
        "    public function example(): void {\n",
        "        Model::query()->where('id');\n",
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

    // Click on "where" in `Model::query()->where('id')` on line 12
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve Model::query()->where via static method return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "function where is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Variable Assignment from Function Call ─────────────────────────────────

/// Test: `$var = app(); $var->abort()` — variable assigned from a function
/// call whose return type is used to resolve the member.
#[tokio::test]
async fn test_goto_definition_variable_from_function_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Application {\n",
        "    public function abort(int $code): void {}\n",
        "}\n",
        "\n",
        "function app(): Application {\n",
        "    return new Application();\n",
        "}\n",
        "\n",
        "class Controller {\n",
        "    public function handle(): void {\n",
        "        $instance = app();\n",
        "        $instance->abort(404);\n",
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

    // Click on "abort" in `$instance->abort(404)` on line 12
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 21,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $instance->abort via function call return type assignment"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "function abort is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Variable Assignment from Method Call ───────────────────────────────────

/// Test: `$var = $this->method(); $var->member()` — variable assigned from
/// a method call whose return type resolves the chained member.
#[tokio::test]
async fn test_goto_definition_variable_from_method_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function getLogger(): Logger {\n",
        "        return new Logger();\n",
        "    }\n",
        "\n",
        "    public function run(): void {\n",
        "        $log = $this->getLogger();\n",
        "        $log->info('hello');\n",
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

    // Click on "info" in `$log->info('hello')` on line 12
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $log->info via $this->getLogger() return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "function info is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Variable Assignment from Static Method Call ────────────────────────────

/// Test: `$var = ClassName::create(); $var->method()` — variable assigned
/// from a static method call whose return type resolves the member.
#[tokio::test]
async fn test_goto_definition_variable_from_static_method_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function where(string $col): self { return $this; }\n",
        "}\n",
        "\n",
        "class Model {\n",
        "    public static function query(): Builder {\n",
        "        return new Builder();\n",
        "    }\n",
        "\n",
        "    public function example(): void {\n",
        "        $qb = Model::query();\n",
        "        $qb->where('active');\n",
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

    // Click on "where" in `$qb->where('active')` on line 12
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $qb->where via Model::query() return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "function where is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Class Index: classes from autoload files ───────────────────────────────

/// Test: Classes defined in autoload_files.php entries are indexed and
/// resolvable for go-to-definition even though they don't follow PSR-4.
#[tokio::test]
async fn test_goto_definition_class_from_autoload_files() {
    use std::fs;

    let dir = tempfile::tempdir().expect("failed to create temp dir");

    // Create composer.json
    fs::write(
        dir.path().join("composer.json"),
        r#"{ "autoload": { "psr-4": {} } }"#,
    )
    .unwrap();

    // Create vendor/composer/autoload_files.php with a reference to
    // a file that defines a class (not following PSR-4).
    let composer_dir = dir.path().join("vendor/composer");
    fs::create_dir_all(&composer_dir).unwrap();

    let helpers_path = "vendor/some-package/helpers.php";
    let helpers_content = concat!(
        "<?php\n",
        "namespace SomePackage;\n",
        "\n",
        "class Helper {\n",
        "    public function doWork(): void {}\n",
        "}\n",
        "\n",
        "function create_helper(): Helper {\n",
        "    return new Helper();\n",
        "}\n",
    );

    // Write the helpers file
    let full_helpers = dir.path().join(helpers_path);
    fs::create_dir_all(full_helpers.parent().unwrap()).unwrap();
    fs::write(&full_helpers, helpers_content).unwrap();

    // Write autoload_files.php
    fs::write(
        composer_dir.join("autoload_files.php"),
        "<?php\nreturn array(\n    'abc123' => $vendorDir . '/some-package/helpers.php',\n);\n",
    )
    .unwrap();

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);

    // Simulate server initialization — parse autoload files
    let autoload_files = phpantom_lsp::composer::parse_autoload_files(dir.path(), "vendor");
    for file_path in &autoload_files {
        if let Ok(content) = fs::read_to_string(file_path) {
            let uri = format!("file://{}", file_path.display());
            backend.update_ast(&uri, &content);

            // Also register functions
            let functions = backend.parse_functions(&content);
            {
                let mut fmap = backend.global_functions().write();
                for func in functions {
                    let fqn = if let Some(ref ns) = func.namespace {
                        format!("{}\\{}", ns, &func.name)
                    } else {
                        func.name.to_string()
                    };
                    fmap.insert(fqn.clone(), (uri.clone(), func.clone()));
                    if func.namespace.is_some() {
                        fmap.or_insert_with(func.name.to_string(), || (uri.clone(), func.clone()));
                    }
                }
            }
        }
    }

    // Verify the Helper class was indexed
    let helper = backend.get_classes_for_uri(&format!("file://{}", full_helpers.display()));
    assert!(
        helper.is_some(),
        "Helper class should be indexed from autoload file"
    );
    let classes = helper.unwrap();
    assert!(
        classes.iter().any(|c| c.name == "Helper"),
        "Should find Helper class in the autoload file classes"
    );

    // Verify the class_index has the FQN
    let has_fqn = backend
        .fqn_uri_index()
        .read()
        .contains_key("SomePackage\\Helper");
    assert!(
        has_fqn,
        "class_index should contain FQN SomePackage\\Helper"
    );
}

// ─── Completion via Function Return Type ────────────────────────────────────

/// Test: completion after `app()->` should show methods from the class
/// returned by the `app()` function.
#[tokio::test]
async fn test_completion_via_function_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Application {\n",
        "    public function abort(int $code): void {}\n",
        "    public function make(string $class): object {}\n",
        "}\n",
        "\n",
        "function app(): Application {\n",
        "    return new Application();\n",
        "}\n",
        "\n",
        "class Controller {\n",
        "    public function handle(): void {\n",
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

    // Trigger completion after `app()->`  on line 12, character 15
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
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
    assert!(result.is_some(), "Should return completions");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<String> = items
                .iter()
                .filter_map(|i| i.filter_text.clone().or_else(|| Some(i.label.clone())))
                .collect();
            assert!(
                names.iter().any(|n| n == "abort"),
                "Should include 'abort' from Application class. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n == "make"),
                "Should include 'make' from Application class. Got: {:?}",
                names
            );
        }
        other => panic!("Expected Array, got: {:?}", other),
    }
}

/// Test: completion after `$this->getConnection()->` should show methods
/// from the class returned by `getConnection()`.
#[tokio::test]
async fn test_completion_via_method_return_type_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Connection {\n",
        "    public function query(string $sql): void {}\n",
        "    public function beginTransaction(): void {}\n",
        "}\n",
        "\n",
        "class Database {\n",
        "    public function getConnection(): Connection {\n",
        "        return new Connection();\n",
        "    }\n",
        "\n",
        "    public function run(): void {\n",
        "        $this->getConnection()->\n",
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

    // Trigger completion after `$this->getConnection()->` on line 12
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return completions");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<String> = items
                .iter()
                .filter_map(|i| i.filter_text.clone().or_else(|| Some(i.label.clone())))
                .collect();
            assert!(
                names.iter().any(|n| n == "query"),
                "Should include 'query' from Connection class. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n == "beginTransaction"),
                "Should include 'beginTransaction' from Connection class. Got: {:?}",
                names
            );
        }
        other => panic!("Expected Array, got: {:?}", other),
    }
}

/// Test: completion after `Model::query()->` should show methods from
/// the class returned by the static `query()` method.
#[tokio::test]
async fn test_completion_via_static_method_return_type_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function where(string $col): self { return $this; }\n",
        "    public function get(): array { return []; }\n",
        "}\n",
        "\n",
        "class Model {\n",
        "    public static function query(): Builder {\n",
        "        return new Builder();\n",
        "    }\n",
        "\n",
        "    public function example(): void {\n",
        "        Model::query()->\n",
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

    // Trigger completion after `Model::query()->` on line 12
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return completions");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<String> = items
                .iter()
                .filter_map(|i| i.filter_text.clone().or_else(|| Some(i.label.clone())))
                .collect();
            assert!(
                names.iter().any(|n| n == "where"),
                "Should include 'where' from Builder class. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n == "get"),
                "Should include 'get' from Builder class. Got: {:?}",
                names
            );
        }
        other => panic!("Expected Array, got: {:?}", other),
    }
}

// ─── Function Return Type with Nullable ─────────────────────────────────────

/// Test: function with nullable return type `?Application` should still
/// resolve to the Application class for chaining.
#[tokio::test]
async fn test_goto_definition_nullable_function_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Application {\n",
        "    public function abort(int $code): void {}\n",
        "}\n",
        "\n",
        "function app(): ?Application {\n",
        "    return new Application();\n",
        "}\n",
        "\n",
        "class Controller {\n",
        "    public function handle(): void {\n",
        "        app()->abort(404);\n",
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

    // Click on "abort" in `app()->abort(404)` on line 11
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve app()->abort even with nullable return type ?Application"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "function abort is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Function with Arguments Before Arrow ───────────────────────────────────

/// Test: `someFunc($arg1, $arg2)->method()` — function call with arguments
/// should still resolve correctly, parentheses are balanced.
#[tokio::test]
async fn test_goto_definition_function_with_args_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Response {\n",
        "    public function send(): void {}\n",
        "    public function status(): int { return 200; }\n",
        "}\n",
        "\n",
        "function response(string $body, int $code): Response {\n",
        "    return new Response();\n",
        "}\n",
        "\n",
        "class Controller {\n",
        "    public function handle(): void {\n",
        "        response('hello', 200)->send();\n",
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

    // Click on "send" in `response('hello', 200)->send()` on line 12
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 33,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve response('hello', 200)->send via function return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "function send is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Cross-file function return type with autoload ──────────────────────────

/// Test: A function defined in an autoload file returns a class defined
/// via PSR-4. Resolution should work across both index types.
#[tokio::test]
async fn test_goto_definition_function_return_type_cross_file() {
    use std::fs;

    let dir = tempfile::tempdir().expect("failed to create temp dir");

    // composer.json with PSR-4 mapping
    fs::write(
        dir.path().join("composer.json"),
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
    )
    .unwrap();

    // Create the Application class via PSR-4
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(
        src_dir.join("Application.php"),
        concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Application {\n",
            "    public function abort(int $code): void {}\n",
            "    public function make(string $class): object {}\n",
            "}\n",
        ),
    )
    .unwrap();

    // Create helpers file with app() function
    let helpers_path = dir.path().join("src/helpers.php");
    fs::write(
        &helpers_path,
        concat!(
            "<?php\n",
            "use App\\Application;\n",
            "\n",
            "function app(): Application {\n",
            "    return new Application();\n",
            "}\n",
        ),
    )
    .unwrap();

    // Create autoload_files.php
    let composer_dir = dir.path().join("vendor/composer");
    fs::create_dir_all(&composer_dir).unwrap();
    fs::write(
        composer_dir.join("autoload_files.php"),
        "<?php\nreturn array(\n    'abc' => $baseDir . '/src/helpers.php',\n);\n",
    )
    .unwrap();

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);

    // Simulate loading autoload files
    let autoload_files = phpantom_lsp::composer::parse_autoload_files(dir.path(), "vendor");
    for file_path in &autoload_files {
        if let Ok(content) = fs::read_to_string(file_path) {
            let uri = format!("file://{}", file_path.display());
            backend.update_ast(&uri, &content);

            let functions = backend.parse_functions(&content);
            {
                let mut fmap = backend.global_functions().write();
                for func in functions {
                    let fqn = if let Some(ref ns) = func.namespace {
                        format!("{}\\{}", ns, &func.name)
                    } else {
                        func.name.to_string()
                    };
                    fmap.insert(fqn.clone(), (uri.clone(), func.clone()));
                    if func.namespace.is_some() {
                        fmap.or_insert_with(func.name.to_string(), || (uri.clone(), func.clone()));
                    }
                }
            }
        }
    }

    // Open a file that uses app()->abort()
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Application;\n",
        "\n",
        "class Controller {\n",
        "    public function handle(): void {\n",
        "        app()->abort(404);\n",
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

    // Click on "abort" in `app()->abort(404)` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve app()->abort cross-file via autoload function + PSR-4 class"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // The abort method should be found in the Application.php file
            assert!(
                location.uri.as_str().contains("Application.php"),
                "Should jump to Application.php, got: {}",
                location.uri
            );
            assert_eq!(
                location.range.start.line, 4,
                "function abort is declared on line 4 in Application.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Method return type: self ───────────────────────────────────────────────

/// Test: Method returning `self` should resolve back to the same class
/// for chaining like `$this->where('x')->get()` inside the same class.
/// NOTE: Cross-variable chaining like `$qb->where()->orderBy()` requires
/// resolving both the variable AND the method return type, which is a
/// more complex multi-step resolution.  This test validates the simpler
/// case of `$this->method()->method()` within the same class.
#[tokio::test]
async fn test_goto_definition_method_returning_self_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class QueryBuilder {\n",
        "    public function where(string $col): self { return $this; }\n",
        "    public function orderBy(string $col): self { return $this; }\n",
        "    public function get(): array { return []; }\n",
        "\n",
        "    public function findActive(): void {\n",
        "        $this->where('active')->orderBy('name');\n",
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

    // Click on "orderBy" in `$this->where('active')->orderBy('name')` on line 7
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve chained method call via 'self' return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "function orderBy is declared on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Completion: variable assigned from function call ───────────────────────

/// Test: Completion on `$instance->` where `$instance = app()` should
/// offer methods from the class returned by `app()`.
#[tokio::test]
async fn test_completion_variable_assigned_from_function_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Application {\n",
        "    public function abort(int $code): void {}\n",
        "    public function make(string $class): object {}\n",
        "}\n",
        "\n",
        "function app(): Application {\n",
        "    return new Application();\n",
        "}\n",
        "\n",
        "class Controller {\n",
        "    public function handle(): void {\n",
        "        $instance = app();\n",
        "        $instance->\n",
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

    // Trigger completion after `$instance->` on line 13
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return completions");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<String> = items
                .iter()
                .filter_map(|i| i.filter_text.clone().or_else(|| Some(i.label.clone())))
                .collect();
            assert!(
                names.iter().any(|n| n == "abort"),
                "Should include 'abort' from Application class via function call assignment. Got: {:?}",
                names
            );
        }
        other => panic!("Expected Array, got: {:?}", other),
    }
}

// ─── Completion: variable assigned from method call ─────────────────────────

/// Test: Completion on `$log->` where `$log = $this->getLogger()` should
/// offer methods from Logger.
#[tokio::test]
async fn test_completion_variable_assigned_from_method_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "    public function error(string $msg): void {}\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function getLogger(): Logger {\n",
        "        return new Logger();\n",
        "    }\n",
        "\n",
        "    public function run(): void {\n",
        "        $log = $this->getLogger();\n",
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

    // Trigger completion after `$log->` on line 13
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return completions");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<String> = items
                .iter()
                .filter_map(|i| i.filter_text.clone().or_else(|| Some(i.label.clone())))
                .collect();
            assert!(
                names.iter().any(|n| n == "info"),
                "Should include 'info' from Logger class via method call assignment. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n == "error"),
                "Should include 'error' from Logger class via method call assignment. Got: {:?}",
                names
            );
        }
        other => panic!("Expected Array, got: {:?}", other),
    }
}

// ─── Guarded Function Definition Resolution ────────────────────────────────

/// Test: goto-definition on `session()` where the function is defined inside
/// an `if (! function_exists('session'))` guard — the pattern used by Laravel
/// helpers.php and many other PHP libraries.
///
/// This tests the full end-to-end flow:
///   1. An autoload file defines `session()` inside an if-guard.
///   2. The server parses the autoload file and registers the function.
///   3. A user file calls `session()`.
///   4. Goto-definition on `session` jumps to the function declaration.
#[tokio::test]
async fn test_goto_definition_function_inside_function_exists_guard() {
    use std::fs;

    let dir = tempfile::tempdir().expect("failed to create temp dir");

    // Create composer.json
    fs::write(
        dir.path().join("composer.json"),
        r#"{ "autoload": { "psr-4": {} } }"#,
    )
    .unwrap();

    // Create vendor/composer/autoload_files.php pointing to helpers.php
    let composer_dir = dir.path().join("vendor/composer");
    fs::create_dir_all(&composer_dir).unwrap();
    fs::write(
        composer_dir.join("autoload_files.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "$baseDir = dirname($vendorDir);\n",
            "return array(\n",
            "    'abc123' => $baseDir . '/helpers.php',\n",
            ");\n",
        ),
    )
    .unwrap();

    // Create helpers.php with functions inside function_exists guards
    fs::write(
        dir.path().join("helpers.php"),
        concat!(
            "<?php\n",
            "\n",
            "if (! function_exists('app')) {\n",
            "    function app(?string $abstract = null): Application\n",
            "    {\n",
            "        return Container::getInstance();\n",
            "    }\n",
            "}\n",
            "\n",
            "if (! function_exists('session')) {\n",
            "    /**\n",
            "     * Get / set the specified session value.\n",
            "     */\n",
            "    function session($key = null, $default = null)\n",
            "    {\n",
            "        if (is_null($key)) {\n",
            "            return app('session');\n",
            "        }\n",
            "        return app('session')->get($key, $default);\n",
            "    }\n",
            "}\n",
        ),
    )
    .unwrap();

    let workspace_root = dir.path().to_path_buf();
    let backend = Backend::new_test_with_workspace(workspace_root.clone(), vec![]);

    // Simulate initialized — this triggers the byte-level autoload
    // file scan followed by an eager full parse of every autoload file.
    // Functions inside `function_exists()` guards live at brace depth 1
    // and are missed by the lightweight scanner, but the eager parse
    // picks them up so the first interactive request never has to parse
    // them on the hot path.
    backend.initialized(InitializedParams {}).await;

    // The guarded functions are eagerly parsed into global_functions
    // during initialization, so the first lookup hits the fast path.
    {
        let fmap = backend.global_functions().read();
        assert!(
            fmap.contains_key("session"),
            "session() should be eagerly parsed into global_functions during init"
        );
        assert!(
            fmap.contains_key("app"),
            "app() should be eagerly parsed into global_functions during init"
        );
    }

    // The autoload file paths should be stored for last-resort lazy parsing.
    {
        let paths = backend.autoload_file_paths().read();
        assert!(
            !paths.is_empty(),
            "autoload_file_paths should contain the helpers.php path"
        );
    }

    // Open a user file that calls session()
    let uri = Url::parse("file:///user_code.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class CartSession\n",
        "{\n",
        "    public static function getCartPublicId(): ?string\n",
        "    {\n",
        "        $id = session()->get(self::CART_SESSION_KEY);\n",
        "        return $id;\n",
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

    // Goto definition on "session" (line 7: `$id = session()->get(...)`)
    // "session" starts at character 14
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve goto-definition for session() defined inside if-guard"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // The function declaration `function session(` should be found
            // in helpers.php
            let path = location.uri.to_file_path().unwrap();
            let filename = path.file_name().unwrap().to_str().unwrap();
            assert_eq!(
                filename, "helpers.php",
                "Should jump to helpers.php where session() is defined"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Test: goto-definition on `app()->abort()` where `app()` is defined inside
/// an `if (! function_exists('app'))` guard and returns `Application`.
/// This combines guarded-function parsing with return-type resolution.
#[tokio::test]
async fn test_goto_definition_guarded_function_return_type_resolution() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Application {\n",
        "    public function abort(int $code): void {}\n",
        "    public function make(string $class): object {}\n",
        "}\n",
        "\n",
        "if (! function_exists('app')) {\n",
        "    function app(): Application {\n",
        "        return new Application();\n",
        "    }\n",
        "}\n",
        "\n",
        "class Controller {\n",
        "    public function handle(): void {\n",
        "        app()->abort(404);\n",
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

    // Click on "abort" in `app()->abort(404)` on line 14
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 14,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve app()->abort via guarded function return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "abort() is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Inline Conditional Return Type (no variable assignment) ────────────────

/// Test: `app(SessionManager::class)->callCustomCreator2()` — when a function
/// with a conditional return type is called inline (not assigned to a variable),
/// goto-definition should still resolve the class from the text arguments.
#[tokio::test]
async fn test_goto_definition_inline_conditional_return_class_string() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
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
        "        app(SessionManager::class)->callCustomCreator2();\n",
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

    // Click on "callCustomCreator2" in `app(SessionManager::class)->callCustomCreator2()`
    // on line 13
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 14,
                character: 36,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve app(SessionManager::class)->callCustomCreator2 via conditional return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "callCustomCreator2() is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Test: `auth('web')->login()` — inline call with non-null argument should
/// resolve to the else branch of an `is null` conditional return type.
#[tokio::test]
async fn test_goto_definition_inline_conditional_return_non_null() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
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
        "        auth('web')->login();\n",
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

    // Click on "login" in `auth('web')->login()` on line 17
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 17,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve auth('web')->login via conditional return type (non-null arg)"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 6,
                "login() is declared on line 6"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Variable assigned from chained method call ─────────────────────────────

/// Test: `$conn = $this->getDatabase()->getConnection(); $conn->`
/// should resolve the variable through the chained method calls and
/// offer methods from Connection.
#[tokio::test]
async fn test_completion_variable_assigned_from_chained_method_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Connection {\n",
        "    public function query(string $sql): void {}\n",
        "    public function beginTransaction(): void {}\n",
        "}\n",
        "\n",
        "class Database {\n",
        "    public function getConnection(): Connection {\n",
        "        return new Connection();\n",
        "    }\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function getDatabase(): Database {\n",
        "        return new Database();\n",
        "    }\n",
        "\n",
        "    public function run(): void {\n",
        "        $conn = $this->getDatabase()->getConnection();\n",
        "        $conn->\n",
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

    // Trigger completion after `$conn->` on line 19
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 19,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return completions");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<String> = items
                .iter()
                .filter_map(|i| i.filter_text.clone().or_else(|| Some(i.label.clone())))
                .collect();
            assert!(
                names.iter().any(|n| n == "query"),
                "Should include 'query' from Connection class via chained method call. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n == "beginTransaction"),
                "Should include 'beginTransaction' from Connection class via chained method call. Got: {:?}",
                names
            );
        }
        other => panic!("Expected Array, got: {:?}", other),
    }
}

/// Test: `$conn = $this->getDatabase()->getConnection(); $conn->query()`
/// should resolve go-to-definition through the chained method call variable.
#[tokio::test]
async fn test_goto_definition_variable_from_chained_method_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Connection {\n",
        "    public function query(string $sql): void {}\n",
        "    public function beginTransaction(): void {}\n",
        "}\n",
        "\n",
        "class Database {\n",
        "    public function getConnection(): Connection {\n",
        "        return new Connection();\n",
        "    }\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function getDatabase(): Database {\n",
        "        return new Database();\n",
        "    }\n",
        "\n",
        "    public function run(): void {\n",
        "        $conn = $this->getDatabase()->getConnection();\n",
        "        $conn->query('SELECT 1');\n",
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

    // Click on "query" in `$conn->query('SELECT 1')` on line 19
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 19,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve definition via chained method call variable"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "query() is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── @return $this docblock support ─────────────────────────────────────────

/// Test: `@return $this` in a docblock resolves the same way as `static`,
/// enabling fluent method chaining.
#[tokio::test]
async fn test_goto_definition_docblock_return_this_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                              // 0
        "class QueryBuilder {\n",                                               // 1
        "    /** @return $this */\n",                                           // 2
        "    public function where(string $col): static { return $this; }\n",   // 3
        "    /** @return $this */\n",                                           // 4
        "    public function orderBy(string $col): static { return $this; }\n", // 5
        "    public function get(): array { return []; }\n",                    // 6
        "\n",                                                                   // 7
        "    public function findActive(): void {\n",                           // 8
        "        $this->where('active')->orderBy('name');\n",                   // 9
        "    }\n",                                                              // 10
        "}\n",                                                                  // 11
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

    // Click on "orderBy" in `$this->where('active')->orderBy('name')` on line 9
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve chained method call via '@return $this'"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 5,
                "orderBy is declared on line 5"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Test: `@return $this` without a native type hint still resolves.
#[tokio::test]
async fn test_goto_definition_docblock_return_this_no_native_hint() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class Builder {\n",                                    // 1
        "    /** @return $this */\n",                           // 2
        "    public function setName(string $name) {\n",        // 3
        "        return $this;\n",                              // 4
        "    }\n",                                              // 5
        "    public function build(): string { return ''; }\n", // 6
        "\n",                                                   // 7
        "    public function test(): void {\n",                 // 8
        "        $this->setName('foo')->build();\n",            // 9
        "    }\n",                                              // 10
        "}\n",                                                  // 11
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

    // Click on "build" in `$this->setName('foo')->build()` on line 9
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 33,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve chained method via '@return $this' without native hint"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 6,
                "build() is declared on line 6"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Test: `@return $this` resolves to the child class when called on an
/// instance of a subclass (same behaviour as `static`).
#[tokio::test]
async fn test_completion_docblock_return_this_inherits_to_child() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                               // 0
        "class BaseBuilder {\n",                                                 // 1
        "    /** @return $this */\n",                                            // 2
        "    public function setName(string $name): static { return $this; }\n", // 3
        "}\n",                                                                   // 4
        "class ChildBuilder extends BaseBuilder {\n",                            // 5
        "    public function childOnly(): void {}\n",                            // 6
        "    public function test(): void {\n",                                  // 7
        "        $this->setName('x')->\n",                                       // 8
        "    }\n",                                                               // 9
        "}\n",                                                                   // 10
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
                    character: 29,
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
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                names.contains(&"childOnly"),
                "After @return $this on inherited method, should see child-class methods, got: {:?}",
                names
            );
            assert!(
                names.contains(&"setName"),
                "Should still see inherited setName, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: Completion on a variable assigned from a method with `@return $this`.
#[tokio::test]
async fn test_completion_variable_from_return_this_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                              // 0
        "class Configurator {\n",                                               // 1
        "    /** @return $this */\n",                                           // 2
        "    public function setOption(string $k): static { return $this; }\n", // 3
        "    public function apply(): void {}\n",                               // 4
        "\n",                                                                   // 5
        "    public function test(): void {\n",                                 // 6
        "        $c = $this->setOption('debug');\n",                            // 7
        "        $c->\n",                                                       // 8
        "    }\n",                                                              // 9
        "}\n",                                                                  // 10
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

    assert!(result.is_some(), "Completion should return results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                names.contains(&"setOption"),
                "Variable from @return $this should offer class methods, got: {:?}",
                names
            );
            assert!(
                names.contains(&"apply"),
                "Variable from @return $this should offer class methods, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}
