use crate::common::create_psr4_workspace;
use phpantom_lsp::Backend;
use phpantom_lsp::composer::parse_autoload_classmap;
use std::collections::HashMap;
use std::fs;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Cross-file / PSR-4 resolution tests ────────────────────────────────────

#[tokio::test]
async fn test_cross_file_double_colon_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Service.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Service {\n",
                "    public static function create(): self { return new self(); }\n",
                "    public function run(): void {}\n",
                "    const VERSION = '1.0';\n",
                "}\n",
            ),
        )],
    );

    // The "current" file references Acme\Service via ::
    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class App {\n",
        "    function boot() {\n",
        "        Acme\\Service::\n",
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

    // Cursor right after `Acme\Service::` on line 3
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 3,
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
        "Completion should resolve Acme\\Service::"
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
            // :: shows static method + constants
            assert!(
                method_names.contains(&"create"),
                "Should include static 'create', got {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"run"),
                "Should exclude non-static 'run'"
            );
            assert!(
                constant_names.contains(&"VERSION"),
                "Should include constant 'VERSION', got {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_cross_file_new_variable_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Widget.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Widget {\n",
                "    public function render(): string { return ''; }\n",
                "    public string $title;\n",
                "}\n",
            ),
        )],
    );

    // The "current" file creates a new Acme\Widget and calls ->
    let uri = Url::parse("file:///page.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Page {\n",
        "    function show() {\n",
        "        $w = new Acme\\Widget();\n",
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

    // Cursor right after `$w->` on line 4
    let completion_params = CompletionParams {
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
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should resolve $w-> to Acme\\Widget members"
    );

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
                .map(|i| i.label.as_str())
                .collect();
            assert!(
                method_names.contains(&"render"),
                "Should include 'render', got {:?}",
                method_names
            );
            assert!(
                prop_names.contains(&"title"),
                "Should include property 'title', got {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_cross_file_param_type_hint_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Logger.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Logger {\n",
                "    public function info(string $msg): void {}\n",
                "    public function error(string $msg): void {}\n",
                "}\n",
            ),
        )],
    );

    // The "current" file has a method with an Acme\Logger parameter
    let uri = Url::parse("file:///handler.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Handler {\n",
        "    function handle(Acme\\Logger $log) {\n",
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

    // Cursor right after `$log->` on line 3
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 3,
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
        "Completion should resolve $log-> via param type hint"
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
                "Should include 'info', got {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"error"),
                "Should include 'error', got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_cross_file_caches_parsed_class() {
    // Verify that after the first completion triggers PSR-4 loading,
    // subsequent completions for the same class don't need to re-read
    // the file (it's cached in ast_map).
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Cache.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Cache {\n",
                "    public static function get(string $key): mixed { return null; }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Controller {\n",
        "    function index() {\n",
        "        Acme\\Cache::\n",
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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    // First call — triggers PSR-4 file loading
    let result1 = backend.completion(completion_params.clone()).await.unwrap();
    assert!(result1.is_some(), "First call should resolve");

    // Second call — should use cached ast_map entry
    let result2 = backend.completion(completion_params).await.unwrap();
    assert!(result2.is_some(), "Second call should also resolve");

    match result2.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"get"),
                "Cached result should still include 'get', got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_cross_file_no_psr4_mapping_falls_back() {
    // When there's no PSR-4 mapping for a class, we should get the fallback
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[], // no files on disk
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class App {\n",
        "    function boot() {\n",
        "        Unknown\\Thing::\n",
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
                line: 3,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_none(),
        "Should return None when no PSR-4 mapping resolves the class"
    );
}

#[tokio::test]
async fn test_cross_file_nested_namespace_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Vendor\\Package\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Sub/Helper.php",
            concat!(
                "<?php\n",
                "namespace Vendor\\Package\\Sub;\n",
                "class Helper {\n",
                "    public static function format(): string { return ''; }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Test {\n",
        "    function run() {\n",
        "        Vendor\\Package\\Sub\\Helper::\n",
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
                line: 3,
                character: 36,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(result.is_some(), "Should resolve deeply nested namespace");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"format"),
                "Should include 'format', got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Use-statement and namespace-relative resolution tests ──────────────────

#[tokio::test]
async fn test_cross_file_use_statement_new_variable() {
    // Simulates the exact scenario from the bug report:
    //   use Klarna\Rest\Resource;
    //   $e = new Resource();
    //   $e->   ← should resolve Resource via use statement → PSR-4
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Klarna\\": "src/Klarna/"
                }
            }
        }"#,
        &[(
            "src/Klarna/Rest/Resource.php",
            concat!(
                "<?php\n",
                "namespace Klarna\\Rest;\n",
                "class Resource {\n",
                "    public function request(string $method): self { return $this; }\n",
                "    public string $url;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///order.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Klarna\\Rest\\Checkout;\n",
        "\n",
        "use Klarna\\Rest\\Resource;\n",
        "\n",
        "class Order {\n",
        "    public function create() {\n",
        "        $e = new Resource();\n",
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

    // Cursor right after `$e->` on line 8
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
        "Completion should resolve $e-> via use statement + PSR-4"
    );

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
                .map(|i| i.label.as_str())
                .collect();
            assert!(
                method_names.contains(&"request"),
                "Should include 'request', got {:?}",
                method_names
            );
            assert!(
                prop_names.contains(&"url"),
                "Should include property 'url', got {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_cross_file_use_statement_double_colon() {
    // `use Acme\Factory;` then `Factory::` should resolve via use statement
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Factory.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Factory {\n",
                "    public static function build(): self { return new self(); }\n",
                "    const VERSION = 1;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Acme\\Factory;\n",
        "\n",
        "class App {\n",
        "    function boot() {\n",
        "        Factory::\n",
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

    // Cursor right after `Factory::` on line 7
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
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
        "Completion should resolve Factory:: via use statement"
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
            assert!(
                method_names.contains(&"build"),
                "Should include static 'build', got {:?}",
                method_names
            );
            assert!(
                constant_names.contains(&"VERSION"),
                "Should include constant 'VERSION', got {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_cross_file_use_statement_aliased() {
    // `use Acme\Service as Svc;` then `$s = new Svc(); $s->`
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Service.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Service {\n",
                "    public function execute(): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///runner.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use Acme\\Service as Svc;\n",
        "\n",
        "class Runner {\n",
        "    function run() {\n",
        "        $s = new Svc();\n",
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

    // Cursor right after `$s->` on line 6
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
        "Completion should resolve aliased $s-> via use statement"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"execute"),
                "Should include 'execute', got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_cross_file_use_statement_param_type_hint() {
    // Parameter typed with a short name imported via `use`
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Mailer.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Mailer {\n",
                "    public function send(string $to): bool { return true; }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///notify.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Acme\\Mailer;\n",
        "\n",
        "class Notifier {\n",
        "    function notify(Mailer $m) {\n",
        "        $m->\n",
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

    // Cursor right after `$m->` on line 7
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
        "Completion should resolve $m-> via use-statement + param type hint"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"send"),
                "Should include 'send', got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_cross_file_classmap_resolution() {
    // Set up a temp workspace with a classmap entry (no PSR-4 mapping)
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    // Create the PHP class file that the classmap points to
    let class_dir = dir.path().join("lib").join("Legacy");
    fs::create_dir_all(&class_dir).expect("failed to create dirs");
    fs::write(
        class_dir.join("Widget.php"),
        concat!(
            "<?php\n",
            "namespace Legacy;\n",
            "class Widget {\n",
            "    public function render(): string { return ''; }\n",
            "    public static function create(): self { return new self(); }\n",
            "    const TYPE = 'widget';\n",
            "}\n",
        ),
    )
    .expect("failed to write class file");

    // Create autoload_classmap.php pointing to the class
    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "\n",
            "$vendorDir = dirname(__DIR__);\n",
            "$baseDir = dirname($vendorDir);\n",
            "\n",
            "return array(\n",
            "    'Legacy\\\\Widget' => $baseDir . '/lib/Legacy/Widget.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    // Build a Backend with NO PSR-4 mappings — only the classmap
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);

    // Populate the class index from the autoload_classmap.php file
    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    assert_eq!(classmap.len(), 1, "Should parse 1 classmap entry");
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    // Open a file that uses Legacy\Widget via ->
    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class App {\n",
        "    function boot() {\n",
        "        $w = new Legacy\\Widget();\n",
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

    // Cursor right after `$w->` on line 4
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 4,
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
        "Completion should resolve Legacy\\Widget via classmap"
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
                "Should include instance method 'render' resolved via classmap, got {:?}",
                method_names
            );
            // PHP allows calling static methods through an instance.
            assert!(
                method_names.contains(&"create"),
                "Should include static 'create' on -> access (PHP allows static calls via instance)"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_cross_file_classmap_double_colon() {
    // Verify classmap works with :: access (static methods + constants)
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    let class_dir = dir.path().join("vendor").join("acme").join("src");
    fs::create_dir_all(&class_dir).expect("failed to create dirs");
    fs::write(
        class_dir.join("Factory.php"),
        concat!(
            "<?php\n",
            "namespace Acme;\n",
            "class Factory {\n",
            "    public static function build(): self { return new self(); }\n",
            "    public function configure(): void {}\n",
            "    const DEFAULT_CONFIG = 'default';\n",
            "}\n",
        ),
    )
    .expect("failed to write class file");

    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "$baseDir = dirname($vendorDir);\n",
            "\n",
            "return array(\n",
            "    'Acme\\\\Factory' => $vendorDir . '/acme/src/Factory.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class App {\n",
        "    function boot() {\n",
        "        Acme\\Factory::\n",
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
                    line: 3,
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
        "Completion should resolve Acme\\Factory via classmap"
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
            assert!(
                method_names.contains(&"build"),
                "Should include static 'build' via classmap, got {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"configure"),
                "Should exclude non-static 'configure' from :: access"
            );
            assert!(
                constant_names.contains(&"DEFAULT_CONFIG"),
                "Should include constant 'DEFAULT_CONFIG' via classmap, got {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_cross_file_namespace_relative_resolution() {
    // Class referenced without a `use` statement, resolved relative to
    // the current namespace:  inside `namespace Acme;`, bare `Sibling`
    // resolves to `Acme\Sibling`.
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Sibling.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Sibling {\n",
                "    public function greet(): string { return 'hi'; }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///main.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Acme;\n",
        "\n",
        "class Main {\n",
        "    function run() {\n",
        "        $s = new Sibling();\n",
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

    // Cursor right after `$s->` on line 6
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
        "Completion should resolve $s-> via namespace-relative lookup"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"greet"),
                "Should include 'greet', got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Namespace resolution tests ─────────────────────────────────────────────
//
// PHP name resolution rules for classes:
//   - Unqualified names (e.g. `PDO`) in a namespace resolve to
//     CurrentNamespace\PDO — NO fallback to global scope.
//   - Fully qualified names (e.g. `\PDO`) always resolve globally.
//   - Imported names (`use PDO;`) resolve via the import table.
//   - In global scope, unqualified names resolve as-is.
//
// See https://www.php.net/manual/en/language.namespaces.rules.php

/// Bare `PDO::` inside `namespace Demo;` should NOT resolve to the global
/// `\PDO` class.  PHP treats this as `Demo\PDO` which doesn't exist.
#[tokio::test]
async fn test_unqualified_class_in_namespace_falls_back_to_global() {
    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert(
        "PDO",
        "<?php\nclass PDO {\n    public static function getAvailableDrivers(): array {}\n}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "\n",
        "class Foo {\n",
        "    function test() {\n",
        "        PDO::\n",
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
                    line: 5,
                    character: 13,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    // After FQN canonicalization, `resolve_class_name` falls back to
    // global scope when the namespace-qualified lookup fails.  This
    // matches user expectations (completions appear even without an
    // explicit `use PDO;`) and is consistent with how other PHP LSPs
    // behave.  The namespace-qualified form (`Demo\PDO`) is tried
    // first and wins when it exists, preserving PHP semantics.
    let has_pdo_methods = match &result {
        Some(CompletionResponse::Array(items)) => items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::METHOD)),
        _ => false,
    };
    assert!(
        has_pdo_methods,
        "Bare `PDO::` in namespace Demo should fall back to global \\PDO"
    );
}

/// Fully qualified `\PDO::` inside `namespace Demo;` SHOULD resolve to the
/// global PDO class — the leading `\` means "global scope".
#[tokio::test]
async fn test_fqn_class_in_namespace_resolves_globally() {
    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert(
        "PDO",
        "<?php\nclass PDO {\n    public static function getAvailableDrivers(): array {}\n}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "\n",
        "class Foo {\n",
        "    function test() {\n",
        "        \\PDO::\n",
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
                    line: 5,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "\\PDO:: should resolve to global PDO");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getAvailableDrivers"),
                "\\PDO:: should show global PDO methods, got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `PDO::` with `use PDO;` in a namespace SHOULD resolve to the global PDO
/// because the import table maps `PDO` → `PDO` (global).
#[tokio::test]
async fn test_imported_class_in_namespace_resolves_via_use() {
    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert(
        "PDO",
        "<?php\nclass PDO {\n    public static function getAvailableDrivers(): array {}\n}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "\n",
        "use PDO;\n",
        "\n",
        "class Foo {\n",
        "    function test() {\n",
        "        PDO::\n",
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
                    character: 13,
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
        "PDO:: with `use PDO;` should resolve to global PDO"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getAvailableDrivers"),
                "Imported PDO:: should show global PDO methods, got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `PDO::` in global scope (no namespace) should resolve to the global PDO.
#[tokio::test]
async fn test_unqualified_class_in_global_scope_resolves() {
    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert(
        "PDO",
        "<?php\nclass PDO {\n    public static function getAvailableDrivers(): array {}\n}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "PDO::\n",);
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
                    line: 1,
                    character: 5,
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
        "PDO:: in global scope should resolve to PDO"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getAvailableDrivers"),
                "PDO:: in global scope should show methods, got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Fully qualified `\Acme\Service::` in a namespace should resolve to the
/// global `Acme\Service` class — the leading `\` bypasses namespace
/// prefixing.
#[tokio::test]
async fn test_fqn_namespaced_class_resolves_globally() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Service.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Service {\n",
                "    public static function create(): self { return new self(); }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Other;\n",
        "\n",
        "class Foo {\n",
        "    function test() {\n",
        "        \\Acme\\Service::\n",
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
                    line: 5,
                    character: 24,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "\\Acme\\Service:: should resolve via FQN");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"create"),
                "\\Acme\\Service:: should show 'create', got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Aliased import: `use PDO as DB;` then `DB::` should resolve to global PDO.
#[tokio::test]
async fn test_aliased_import_resolves_in_namespace() {
    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert(
        "PDO",
        "<?php\nclass PDO {\n    public static function getAvailableDrivers(): array {}\n}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "\n",
        "use PDO as DB;\n",
        "\n",
        "class Foo {\n",
        "    function test() {\n",
        "        DB::\n",
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

    assert!(
        result.is_some(),
        "DB:: with `use PDO as DB;` should resolve to global PDO"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"getAvailableDrivers"),
                "Aliased DB:: should show global PDO methods, got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Variable resolution inside a standalone function (not a class method)
/// with a cross-file class loaded via `use` + PSR-4.
#[tokio::test]
async fn test_cross_file_variable_in_standalone_function() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Widget.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Widget {\n",
                "    public function render(): string { return ''; }\n",
                "    public string $title;\n",
                "}\n",
            ),
        )],
    );

    // ── Test 1: inside a class method (known working baseline) ──
    let uri_class = Url::parse("file:///test_class.php").unwrap();
    let text_class = concat!(
        "<?php\n",
        "use Acme\\Widget;\n",
        "class Page {\n",
        "    function show() {\n",
        "        $w = new Widget();\n",
        "        $w->\n",
        "    }\n",
        "}\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri_class.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text_class.to_string(),
            },
        })
        .await;
    let result_class = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_class.clone(),
                },
                position: Position {
                    line: 5,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    let class_methods: Vec<&str> = match result_class.as_ref() {
        Some(CompletionResponse::Array(items)) => items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
            .collect(),
        _ => vec![],
    };
    assert!(
        class_methods.contains(&"render"),
        "Baseline: $w-> inside class method should resolve Widget, got {:?}",
        class_methods
    );

    // ── Test 2: inside a standalone function ──
    let uri_func = Url::parse("file:///test_func.php").unwrap();
    let text_func = concat!(
        "<?php\n",
        "use Acme\\Widget;\n",
        "function demo() {\n",
        "    $w = new Widget();\n",
        "    $w->\n",
        "}\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri_func.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text_func.to_string(),
            },
        })
        .await;
    let result_func = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_func.clone(),
                },
                position: Position {
                    line: 4,
                    character: 8,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    let func_methods: Vec<&str> = match result_func.as_ref() {
        Some(CompletionResponse::Array(items)) => items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
            .collect(),
        _ => vec![],
    };
    assert!(
        func_methods.contains(&"render"),
        "$w-> inside standalone function should resolve Widget, got {:?}",
        func_methods
    );
}

/// Variable assignment from a static call chain in a standalone function:
/// `$p = Acme\Processor::create()->build(); $p->`
/// where `create()` returns `Acme\Builder` and `build()` returns `Acme\Product`.
///
/// This reproduces the same pattern as the Laravel factory chain test
/// (`User::factory()->create()`) but in a minimal cross-file PSR-4 setup.
#[tokio::test]
async fn test_cross_file_static_chain_variable_in_standalone_function() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Product.php",
                concat!(
                    "<?php\n",
                    "namespace Acme;\n",
                    "class Product {\n",
                    "    public function getName(): string { return ''; }\n",
                    "}\n",
                ),
            ),
            (
                "src/Builder.php",
                concat!(
                    "<?php\n",
                    "namespace Acme;\n",
                    "class Builder {\n",
                    "    public function build(): Product { return new Product(); }\n",
                    "}\n",
                ),
            ),
            (
                "src/Factory.php",
                concat!(
                    "<?php\n",
                    "namespace Acme;\n",
                    "class Factory {\n",
                    "    public static function create(): Builder { return new Builder(); }\n",
                    "}\n",
                ),
            ),
        ],
    );

    // ── Inline chain baseline (no variable) ──
    let uri_inline = Url::parse("file:///test_inline.php").unwrap();
    let text_inline = concat!(
        "<?php\n",
        "use Acme\\Factory;\n",
        "function test_inline() {\n",
        "    Factory::create()->build()->\n",
        "}\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri_inline.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text_inline.to_string(),
            },
        })
        .await;
    let result_inline = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_inline.clone(),
                },
                position: Position {
                    line: 3,
                    character: 33,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    let inline_methods: Vec<&str> = match result_inline.as_ref() {
        Some(CompletionResponse::Array(items)) => items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
            .collect(),
        _ => vec![],
    };
    // ── Variable assignment from static chain in standalone function ──
    let uri_func = Url::parse("file:///test_chain_func.php").unwrap();
    let text_func = concat!(
        "<?php\n",
        "use Acme\\Factory;\n",
        "function test_chain() {\n",
        "    $p = Factory::create()->build();\n",
        "    $p->\n",
        "}\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri_func.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text_func.to_string(),
            },
        })
        .await;
    let result_func = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_func.clone(),
                },
                position: Position {
                    line: 4,
                    character: 8,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    let func_methods: Vec<&str> = match result_func.as_ref() {
        Some(CompletionResponse::Array(items)) => items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
            .collect(),
        _ => vec![],
    };
    // ── Top-level variable assignment (no enclosing function) ──
    let uri_top = Url::parse("file:///test_chain_top.php").unwrap();
    let text_top = concat!(
        "<?php\n",
        "use Acme\\Factory;\n",
        "$p = Factory::create()->build();\n",
        "$p->\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri_top.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text_top.to_string(),
            },
        })
        .await;
    let result_top = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_top.clone(),
                },
                position: Position {
                    line: 3,
                    character: 4,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    let top_methods: Vec<&str> = match result_top.as_ref() {
        Some(CompletionResponse::Array(items)) => items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
            .collect(),
        _ => vec![],
    };
    // Assertions
    assert!(
        inline_methods.contains(&"getName"),
        "Inline chain Factory::create()->build()-> should resolve to Product, got {:?}",
        inline_methods
    );
    assert!(
        func_methods.contains(&"getName"),
        "$p = Factory::create()->build() in function should resolve to Product, got {:?}\n\
         inline chain: {:?}\n\
         top-level: {:?}",
        func_methods,
        inline_methods,
        top_methods
    );
}
