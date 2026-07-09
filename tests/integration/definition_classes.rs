use crate::common::{create_psr4_workspace, create_test_backend};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Same-File Goto Definition Tests ────────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_same_file_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(): void {}\n",
        "}\n",
        "class Service {\n",
        "    public function run(Logger $logger): void {\n",
        "        $logger->info();\n",
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

    // Click on "Logger" in the parameter type hint on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 27,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve same-file class definition"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 1, "Logger is defined on line 1");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_same_file_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Cacheable {\n",
        "    public function getCacheKey(): string;\n",
        "}\n",
        "class Repository {\n",
        "    public function cache(Cacheable $item): void {}\n",
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

    // Click on "Cacheable" in the parameter type hint on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 30,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve same-file interface definition"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "Cacheable is defined on line 1"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Cross-File PSR-4 Goto Definition Tests ─────────────────────────────────

#[tokio::test]
async fn test_goto_definition_cross_file_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Logger.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "\n",
                "class Logger {\n",
                "    public function info(string $msg): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///service.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Service {\n",
        "    public function run(Logger $logger): void {\n",
        "        $logger->info('hello');\n",
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

    // Click on "Logger" in the parameter type hint on line 4
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 4,
                character: 27,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve cross-file PSR-4 class definition"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Logger.php"),
                "Should point to src/Logger.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 3,
                "Logger class defined on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_after_target_did_close() {
    // Regression test for issue #99: go-to-definition stops working
    // after the target file is closed via textDocument/didClose.
    let backend = create_test_backend();
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    let text_b = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class ClassB {\n",
        "    public function doSomething() {}\n",
        "}\n",
    );
    std::fs::write(src_dir.join("ClassB.php"), text_b).unwrap();

    let uri_b = Url::from_file_path(src_dir.join("ClassB.php")).unwrap();

    // Open ClassB so it gets indexed.
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri_b.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text_b.to_string(),
            },
        })
        .await;

    // Close ClassB — simulates VS Code peek preview closing.
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri_b.clone() },
        })
        .await;

    // Open ClassA which references ClassB.
    let uri_a = Url::from_file_path(src_dir.join("ClassA.php")).unwrap();
    let text_a = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class ClassA {\n",
        "    public function test() {\n",
        "        $b = new ClassB();\n",
        "        $b->doSomething();\n",
        "    }\n",
        "}\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri_a.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text_a.to_string(),
            },
        })
        .await;

    // Go-to-definition on "ClassB" (line 5, char 21 = inside "ClassB")
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_a.clone() },
            position: Position {
                line: 5,
                character: 21,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Go-to-definition should work after target file is closed (issue #99)"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri_b, "Should jump to ClassB.php");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // Also test member access: $b->doSomething() (line 6, char 14)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_a },
            position: Position {
                line: 6,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Go-to-definition on member should work after target file is closed (issue #99)"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                location.uri, uri_b,
                "Member definition should jump to ClassB.php"
            );
        }
        other => panic!("Expected Scalar location for member, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_cross_file_with_use_statement() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/",
                    "App\\Contracts\\": "src/Contracts/"
                }
            }
        }"#,
        &[(
            "src/Contracts/Repository.php",
            concat!(
                "<?php\n",
                "namespace App\\Contracts;\n",
                "\n",
                "interface Repository {\n",
                "    public function find(int $id): mixed;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///service.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "\n",
        "use App\\Contracts\\Repository;\n",
        "\n",
        "class UserService {\n",
        "    public function __construct(private Repository $repo) {}\n",
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

    // Click on "Repository" in the constructor parameter on line 6
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 47,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve class imported via use statement"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Contracts/Repository.php"),
                "Should point to Repository.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 3,
                "Repository interface defined on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_on_use_statement_name() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "\n",
                "class User {\n",
                "    public string $name;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Controllers;\n",
        "\n",
        "use App\\Models\\User;\n",
        "\n",
        "class UserController {\n",
        "    public function show(User $user): void {}\n",
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

    // Click on the use statement FQN "App\\Models\\User" on line 3
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 3,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve goto-def on use statement FQN"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Models/User.php"),
                "Should point to User.php, got: {:?}",
                path
            );
            assert_eq!(location.range.start.line, 3, "User class defined on line 3");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_cross_file_with_grouped_use_statement() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Project\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/CatalogIndex/Services/Validation/ComplementsScenarioValidation.php",
                concat!(
                    "<?php\n",
                    "namespace Project\\CatalogIndex\\Services\\Validation;\n\n",
                    "class ComplementsScenarioValidation {}\n",
                ),
            ),
            (
                "src/CatalogIndex/Services/Validation/ProductByModelValidation.php",
                concat!(
                    "<?php\n",
                    "namespace Project\\CatalogIndex\\Services\\Validation;\n\n",
                    "class ProductByModelValidation {}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Project\\CatalogIndex\\Controllers;\n\n",
        "use Project\\CatalogIndex\\Services\\Validation\\{ComplementsScenarioValidation, ProductByModelValidation};\n\n",
        "class ProductController {\n",
        "    public function test(ComplementsScenarioValidation $a): ProductByModelValidation {\n",
        "        return new ProductByModelValidation();\n",
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

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve class imported via grouped use statement"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with(
                    "src/CatalogIndex/Services/Validation/ComplementsScenarioValidation.php"
                ),
                "Should point to ComplementsScenarioValidation.php, got: {:?}",
                path
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_on_grouped_use_statement_name() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Project\\": "src/"
                }
            }
        }"#,
        &[(
            "src/CatalogIndex/Services/Validation/ComplementsScenarioValidation.php",
            concat!(
                "<?php\n",
                "namespace Project\\CatalogIndex\\Services\\Validation;\n\n",
                "class ComplementsScenarioValidation {}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Project\\CatalogIndex\\Controllers;\n\n",
        "use Project\\CatalogIndex\\Services\\Validation\\{ComplementsScenarioValidation};\n\n",
        "class ProductController {}\n",
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

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 53,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve class name inside grouped use statement"
    );
}

#[tokio::test]
async fn test_goto_definition_class_reference_via_namespace() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Enums/Status.php",
            concat!(
                "<?php\n",
                "namespace App\\Enums;\n",
                "\n",
                "enum Status: string {\n",
                "    case Active = 'active';\n",
                "    case Inactive = 'inactive';\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///model.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Enums;\n",
        "\n",
        "class Model {\n",
        "    protected $casts = [\n",
        "        'status' => Status::class,\n",
        "    ];\n",
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

    // Click on "Status" in Status::class on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve namespace-relative class reference"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Enums/Status.php"),
                "Should point to Status.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 3,
                "Status enum defined on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_return_type_hint() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Collection.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "\n",
                "class Collection {\n",
                "    public function first(): mixed { return null; }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///repo.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Repository {\n",
        "    public function getAll(): Collection {\n",
        "        return new Collection();\n",
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

    // Click on "Collection" in the return type on line 4
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 4,
                character: 33,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(result.is_some(), "Should resolve return type hint");

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Collection.php"),
                "Should point to Collection.php, got: {:?}",
                path
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Edge Cases ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_unresolvable_returns_none() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\n$x = 42;\n";

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Click on a number — no class to resolve
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 1,
                character: 5,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    // "42" gets extracted as a word but can't be resolved to any class
    assert!(result.is_none(), "Non-class symbol should return None");
}

#[tokio::test]
async fn test_goto_definition_whitespace_returns_none() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\n    \n";

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 1,
                character: 2,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(result.is_none(), "Whitespace should return None");
}

#[tokio::test]
async fn test_goto_definition_vendor_cross_file() {
    // Vendor classes are resolved via the Composer classmap, not vendor
    // PSR-4.  This test verifies that a cold Ctrl+Click on a vendor class
    // (never loaded by completion/hover) resolves through the classmap.
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("failed to write composer.json");

    // Create the vendor PHP file on disk.
    let vendor_file = dir
        .path()
        .join("vendor/monolog/monolog/src/Monolog/Logger.php");
    std::fs::create_dir_all(vendor_file.parent().unwrap()).unwrap();
    std::fs::write(
        &vendor_file,
        concat!(
            "<?php\n",
            "namespace Monolog;\n",
            "\n",
            "class Logger {\n",
            "    public function info(string $msg): void {}\n",
            "}\n",
        ),
    )
    .unwrap();

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);

    // Populate class index with the vendor class.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Monolog\\Logger".to_string(),
            Url::from_file_path(&vendor_file).unwrap().to_string(),
        );
    }

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Monolog\\Logger;\n",
        "\n",
        "class App {\n",
        "    public function boot(Logger $log): void {}\n",
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

    // Click on "Logger" in the parameter type hint on line 6
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 30,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(result.is_some(), "Should resolve vendor class via classmap");

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("vendor/monolog/monolog/src/Monolog/Logger.php"),
                "Should point to vendor Logger.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 3,
                "Logger class defined on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_trait() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Traits/Auditable.php",
            concat!(
                "<?php\n",
                "namespace App\\Traits;\n",
                "\n",
                "trait Auditable {\n",
                "    public function getAuditLog(): array { return []; }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///model.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "\n",
        "use App\\Traits\\Auditable;\n",
        "\n",
        "class Order {\n",
        "    use Auditable;\n",
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

    // Click on "Auditable" in `use Auditable;` inside the class on line 6
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 10,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(result.is_some(), "Should resolve trait via use statement");

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Traits/Auditable.php"),
                "Should point to Auditable.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 3,
                "Auditable trait defined on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_extends_class() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/BaseModel.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "\n",
                "abstract class BaseModel {\n",
                "    public function save(): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///user.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class User extends BaseModel {\n",
        "    public string $name;\n",
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

    // Click on "BaseModel" in the extends clause on line 3
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 3,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(result.is_some(), "Should resolve parent class in extends");

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/BaseModel.php"),
                "Should point to BaseModel.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 3,
                "BaseModel class defined on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Regression test: when a file uses `use Foo\Bar\Controller as BaseController`,
/// go-to-definition on `BaseController` should navigate to the imported class,
/// NOT the same-file class whose short name (`Controller`) happens to match
/// the short name of the imported FQN.
#[tokio::test]
async fn test_goto_definition_aliased_use_does_not_match_same_file_short_name() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/",
                    "Illuminate\\": "vendor/illuminate/"
                }
            }
        }"#,
        &[(
            "vendor/illuminate/Routing/Controller.php",
            concat!(
                "<?php\n",
                "namespace Illuminate\\Routing;\n",
                "\n",
                "class Controller {\n",
                "    public function middleware() {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Http\\Controllers;\n",
        "\n",
        "use Illuminate\\Routing\\Controller as BaseController;\n",
        "\n",
        "class Controller extends BaseController\n",
        "{\n",
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

    // Click on "BaseController" in the extends clause on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 30,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve aliased BaseController to Illuminate\\Routing\\Controller"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("vendor/illuminate/Routing/Controller.php"),
                "Should point to vendor Controller.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 3,
                "Controller class defined on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}
