use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Member Definition: Class Constants ─────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_class_constant_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class MyClass {\n",
        "    const MY_CONST = 42;\n",
        "    const OTHER = 'hello';\n",
        "\n",
        "    public function foo(): int {\n",
        "        return self::MY_CONST;\n",
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

    // Click on "MY_CONST" in `self::MY_CONST` on line 6
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve self::MY_CONST to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "const MY_CONST is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_self_const_in_class_level_attribute() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "#[Route(name: self::ROUTE)]\n",
        "class Ctrl {\n",
        "    const ROUTE = 'health-check';\n",
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

    // Click on "ROUTE" in `self::ROUTE` inside the class-level attribute.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve self::ROUTE inside a class-level attribute"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "const ROUTE is declared on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_class_constant_via_classname() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Status {\n",
        "    const ACTIVE = 1;\n",
        "    const INACTIVE = 0;\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function check(): int {\n",
        "        return Status::ACTIVE;\n",
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

    // Click on "ACTIVE" in `Status::ACTIVE` on line 8
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 8,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve Status::ACTIVE to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "const ACTIVE is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_class_constant_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Status.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "\n",
                "class Status {\n",
                "    const PENDING = 'pending';\n",
                "    const APPROVED = 'approved';\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///service.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class OrderService {\n",
        "    public function getDefault(): string {\n",
        "        return Status::PENDING;\n",
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

    // Click on "PENDING" in `Status::PENDING` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve cross-file Status::PENDING"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Status.php"),
                "Should point to Status.php, got: {:?}",
                path
            );
            assert_eq!(location.range.start.line, 4, "const PENDING is on line 4");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Member Definition: Array callables ─────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_array_callable_controller_action() {
    // The Laravel route shape: `[Controller::class, 'method']`.  Clicking the
    // method string should jump to the controller method (issue #176).
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class IndexPageController {\n",
        "    public function indexPage() {}\n",
        "}\n",
        "\n",
        "Route::get('/', [IndexPageController::class, 'indexPage']);\n",
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

    // Click on "indexPage" inside the callable array on line 5.
    let char = text.lines().nth(5).unwrap().find("indexPage").unwrap() as u32;
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: char + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve [IndexPageController::class, 'indexPage'] to the method"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "indexPage() is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Member Definition: Methods ─────────────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_method_via_this() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "\n",
        "    public function warn(string $msg): void {\n",
        "        $this->info($msg);\n",
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

    // Click on "info" in `$this->info(...)` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->info to its declaration"
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

#[tokio::test]
async fn test_goto_definition_static_method_via_classname() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public static function create(): self {\n",
        "        return new self();\n",
        "    }\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        Factory::create();\n",
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

    // Click on "create" in `Factory::create()` on line 9
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve Factory::create to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "function create is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_method_via_self() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Calculator {\n",
        "    public static function add(int $a, int $b): int {\n",
        "        return $a + $b;\n",
        "    }\n",
        "\n",
        "    public static function sum(array $nums): int {\n",
        "        return self::add($nums[0], $nums[1]);\n",
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

    // Click on "add" in `self::add(...)` on line 7
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 23,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve self::add to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "function add is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_method_cross_file() {
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
                "    public function error(string $msg): void {}\n",
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
        "        $logger->error('failed');\n",
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

    // Click on "error" in `$logger->error(...)` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(result.is_some(), "Should resolve cross-file $logger->error");

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Logger.php"),
                "Should point to Logger.php, got: {:?}",
                path
            );
            assert_eq!(location.range.start.line, 5, "function error is on line 5");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Member Definition: Properties ──────────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_property_via_this() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public int $age;\n",
        "\n",
        "    public function getName(): string {\n",
        "        return $this->name;\n",
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

    // Click on "name" in `$this->name` on line 6
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 23,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->name to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$name property is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_property_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Config.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "\n",
                "class Config {\n",
                "    public string $dbHost;\n",
                "    public int $dbPort;\n",
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
        "    public function connect(Config $cfg): void {\n",
        "        $host = $cfg->dbHost;\n",
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

    // Click on "dbHost" in `$cfg->dbHost` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(result.is_some(), "Should resolve cross-file $cfg->dbHost");

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Config.php"),
                "Should point to Config.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 4,
                "$dbHost property is on line 4"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Member Definition: Inherited Members ───────────────────────────────────

#[tokio::test]
async fn test_goto_definition_inherited_method() {
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
                "class BaseModel {\n",
                "    public function save(): void {}\n",
                "    public function delete(): void {}\n",
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
        "\n",
        "    public function update(): void {\n",
        "        $this->save();\n",
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

    // Click on "save" in `$this->save()` on line 7
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve inherited $this->save() to parent class"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/BaseModel.php"),
                "Should point to BaseModel.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 4,
                "function save is on line 4 of BaseModel.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_inherited_constant_via_parent() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    const VERSION = '1.0';\n",
        "}\n",
        "\n",
        "class Child extends Base {\n",
        "    public function getVersion(): string {\n",
        "        return parent::VERSION;\n",
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

    // Click on "VERSION" in `parent::VERSION` on line 7
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve parent::VERSION to Base class"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "const VERSION is declared on line 2 in Base"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Member Definition: Variable Type Inference ─────────────────────────────

#[tokio::test]
async fn test_goto_definition_method_on_new_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Mailer {\n",
        "    public function send(string $to): void {}\n",
        "    public function queue(string $to): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $mailer = new Mailer();\n",
        "        $mailer->send('user@example.com');\n",
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

    // Click on "send" in `$mailer->send(...)` on line 9
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $mailer->send via new Mailer() assignment"
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

// ─── Member Definition: Chained Access ──────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_chained_property_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Connection {\n",
        "    public function query(string $sql): void {}\n",
        "}\n",
        "\n",
        "class Database {\n",
        "    public Connection $conn;\n",
        "\n",
        "    public function run(): void {\n",
        "        $this->conn->query('SELECT 1');\n",
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

    // Click on "query" in `$this->conn->query(...)` on line 9
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->conn->query via chained property"
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

// ─── Member Definition: Promoted Properties ─────────────────────────────────

#[tokio::test]
async fn test_goto_definition_promoted_property() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function __construct(\n",
        "        private string $name,\n",
        "        private int $age,\n",
        "    ) {}\n",
        "\n",
        "    public function getName(): string {\n",
        "        return $this->name;\n",
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

    // Click on "name" in `$this->name` on line 8
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 8,
                character: 23,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->name to promoted property"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 3, "promoted $name is on line 3");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Member Definition: static:: keyword ────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_constant_via_static() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    const MAX_RETRIES = 3;\n",
        "\n",
        "    public function getMax(): int {\n",
        "        return static::MAX_RETRIES;\n",
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

    // Click on "MAX_RETRIES" in `static::MAX_RETRIES` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(result.is_some(), "Should resolve static::MAX_RETRIES");

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "const MAX_RETRIES is on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Member Definition: use statement + cross-file ──────────────────────────

#[tokio::test]
async fn test_goto_definition_method_cross_file_with_use_statement() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Lib\\": "lib/"
                }
            }
        }"#,
        &[(
            "lib/Cache.php",
            concat!(
                "<?php\n",
                "namespace Lib;\n",
                "\n",
                "class Cache {\n",
                "    public function get(string $key): mixed {}\n",
                "    public function set(string $key, mixed $val): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Lib\\Cache;\n",
        "\n",
        "class Service {\n",
        "    public function load(Cache $cache): void {\n",
        "        $cache->get('key');\n",
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

    // Click on "get" in `$cache->get(...)` on line 7
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
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
        "Should resolve $cache->get via use statement"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("lib/Cache.php"),
                "Should point to Cache.php, got: {:?}",
                path
            );
            assert_eq!(location.range.start.line, 4, "function get is on line 4");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Member Definition: cursor on class name still resolves class ───────────

#[tokio::test]
async fn test_goto_definition_cursor_on_classname_before_double_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Status {\n",
        "    const ACTIVE = 1;\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function check(): int {\n",
        "        return Status::ACTIVE;\n",
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

    // Click on "Status" (the class name, left side of ::) on line 7
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Cursor on class name before :: should resolve to the class"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 1, "class Status is on line 1");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Property vs Method Disambiguation ──────────────────────────────────────

/// When a class has both a property `$id` and a method `id()`, goto-definition
/// on `$user->id` (no parentheses) should navigate to the *property*, not the
/// method.
#[tokio::test]
async fn test_goto_definition_property_preferred_over_method_without_parens() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    //                                           line
    let text = concat!(
        "<?php\n",                              // 0
        "class User {\n",                       // 1
        "    public int $id;\n",                // 2  ← property declaration
        "    public string $name;\n",           // 3
        "\n",                                   // 4
        "    public function id(): int {\n",    // 5  ← method declaration
        "        return $this->id;\n",          // 6
        "    }\n",                              // 7
        "\n",                                   // 8
        "    public function test(): void {\n", // 9
        "        $user = new User();\n",        // 10
        "        $val = $user->id;\n",          // 11 ← property access (no parens)
        "        $val2 = $user->id();\n",       // 12 ← method call (with parens)
        "    }\n",                              // 13
        "}\n",                                  // 14
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

    // ── Case 1: `$user->id` (no parens) → should go to the $id property on line 2
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 24, // on "id" in `$user->id`
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $user->id to the property declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$user->id (no parens) should go to the $id property on line 2, not the id() method on line 5"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // ── Case 2: `$user->id()` (with parens) → should go to the id() method on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 25, // on "id" in `$user->id()`
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $user->id() to the method declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 5,
                "$user->id() (with parens) should go to the id() method on line 5, not the $id property on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Same disambiguation but via `$this->` inside the class itself.
#[tokio::test]
async fn test_goto_definition_this_property_vs_method_disambiguation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "class Order {\n",                         // 1
        "    public float $total;\n",              // 2  ← property
        "\n",                                      // 3
        "    public function total(): float {\n",  // 4  ← method
        "        return $this->total;\n",          // 5  ← property access
        "    }\n",                                 // 6
        "\n",                                      // 7
        "    public function display(): void {\n", // 8
        "        echo $this->total;\n",            // 9  ← property access
        "        echo $this->total();\n",          // 10 ← method call
        "    }\n",                                 // 11
        "}\n",                                     // 12
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

    // `$this->total` on line 9 (no parens) → property on line 2
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 23, // on "total" in `$this->total`
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                location.range.start.line, 2,
                "$this->total (no parens) should go to the $total property on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // `$this->total()` on line 10 (with parens) → method on line 4
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: 23, // on "total" in `$this->total()`
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                location.range.start.line, 4,
                "$this->total() (with parens) should go to the total() method on line 4"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ── @method tag: method name matches a type keyword ─────────────────────────

#[tokio::test]
async fn test_goto_definition_method_tag_name_matches_type_keyword() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_string.php").unwrap();
    let text = concat!(
        "<?php\n", // 0
        "/**\n",   // 1
        " * @method static string string(string $key, \\Closure|string|null $default = null)\n", // 2
        " */\n",                      // 3
        "class Config {\n",           // 4
        "}\n",                        // 5
        "\n",                         // 6
        "Config::string('hello');\n", // 7
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

    // Click on "string" in `Config::string('hello')` on line 7, character 8
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 10,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve Config::string() to the @method tag declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "@method string string(...) is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_method_on_static_call_with_nested_call_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_nested_arg.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                     // 0
        "class Country {}\n",                                                          // 1
        "\n",                                                                          // 2
        "class SettingsProvider {\n",                                                  // 3
        "    public function get(string $key): string { return ''; }\n",               // 4
        "}\n",                                                                         // 5
        "\n",                                                                          // 6
        "class Environment {\n",                                                       // 7
        "    public static function get(Country $env): self { return new self(); }\n", // 8
        "    public function settings(): SettingsProvider { return new SettingsProvider(); }\n", // 9
        "}\n",                                                                       // 10
        "\n",                                                                        // 11
        "class CurrentEnvironment {\n",                                              // 12
        "    public static function country(): Country { return new Country(); }\n", // 13
        "    public static function settings(): SettingsProvider {\n",               // 14
        "        return Environment::get(self::country())->settings();\n",           // 15
        "    }\n",                                                                   // 16
        "}\n",                                                                       // 17
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

    // Click on "settings" in `Environment::get(self::country())->settings()` on line 15
    // "settings" starts at character 50
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 15,
                character: 52,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve ->settings() after Environment::get(self::country()) to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 9,
                "Environment::settings() is declared on line 9"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Regression: member access must not fall through to standalone function ──

/// When the cursor is on `map` in `collect($x)->map(`, go-to-definition
/// must NOT resolve `map` as a standalone function.  If the owning class
/// can be determined, it should jump to `Collection::map()`.  If it
/// cannot (e.g. `collect` isn't indexed), the result should be `None` —
/// never a fallback to a global `map()` function.
///
/// This is the general pattern: any word on the right side of `->`,
/// `?->`, or `::` is a *member name*, not a standalone symbol.
#[tokio::test]
async fn test_goto_definition_member_does_not_fallthrough_to_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///fallthrough.php").unwrap();
    // `map` exists both as a method on Collection AND as a standalone
    // function.  Go-to-definition on the `->map` call must resolve to
    // the method, not the function.
    let text = concat!(
        "<?php\n",
        "class Collection {\n",
        "    public function map(callable $cb): static {}\n",
        "    public function values(): static {}\n",
        "}\n",
        "\n",
        "/** @return Collection */\n",
        "function collect($v): Collection { return new Collection(); }\n",
        "\n",
        "/** Standalone map function — must NOT be the target. */\n",
        "function map(array $arr, callable $cb): array { return []; }\n",
        "\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $result = collect([])->map(fn ($x) => $x);\n",
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

    // Click on `map` in `collect([])->map(` on line 14
    // Line 14: "        $result = collect([])->map(fn ($x) => $x);\n"
    //                                         ^ cursor on 'm' of 'map'
    let line_text = "        $result = collect([])->map(fn ($x) => $x);";
    let map_col = line_text.find("->map(").unwrap() + 2; // position of 'm'

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 14,
                character: map_col as u32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            // Must point to Collection::map on line 2, NOT the standalone
            // `function map(...)` on line 10.
            assert_eq!(
                location.range.start.line, 2,
                "Expected Collection::map on line 2, got line {}",
                location.range.start.line,
            );
        }
        None => {
            // Acceptable: subject resolution couldn't find Collection.
            // The important thing is that we did NOT jump to the
            // standalone `map()` function on line 10.
        }
        other => panic!(
            "Expected Scalar location pointing to Collection::map or None, got: {:?}",
            other
        ),
    }
}

/// When the owning class truly can't be resolved (unknown function in the
/// chain), go-to-definition on the member should return None — not jump
/// to a standalone function with the same name.
#[tokio::test]
async fn test_goto_definition_method_on_enum_returned_by_static_call_forward_ref() {
    // Exact reproduction of user report: class defined BEFORE the enum
    // (forward reference), empty method bodies, no enum cases.
    let backend = create_test_backend();

    let uri = Url::parse("file:///forward_ref_enum.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "final class CurrentEnvironment {\n",                // 1
        "    public static function country(): Country {\n", // 2
        "    }\n",                                           // 3
        "}\n",                                               // 4
        "\n",                                                // 5
        "enum Country: string {\n",                          // 6
        "    public function getName(): string {\n",         // 7
        "    }\n",                                           // 8
        "}\n",                                               // 9
        "\n",                                                // 10
        "CurrentEnvironment::country()->getName();\n",       // 11
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

    // Click on "getName" in `CurrentEnvironment::country()->getName()` on line 11
    let line_text = "CurrentEnvironment::country()->getName();";
    let col = line_text.find("getName").unwrap();

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: col as u32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve ->getName() on enum Country even when class is defined before enum (forward reference)"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 7,
                "Country::getName() is declared on line 7"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_method_on_enum_returned_by_static_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///static_enum_chain.php").unwrap();
    let text = concat!(
        "<?php\n",                                                       // 0
        "enum Country: string {\n",                                      // 1
        "    case DK = 'DK';\n",                                         // 2
        "    public function getName(): string { return 'Denmark'; }\n", // 3
        "}\n",                                                           // 4
        "\n",                                                            // 5
        "final class CurrentEnvironment {\n",                            // 6
        "    public static function country(): Country {\n",             // 7
        "        return Country::DK;\n",                                 // 8
        "    }\n",                                                       // 9
        "}\n",                                                           // 10
        "\n",                                                            // 11
        "CurrentEnvironment::country()->getName();\n",                   // 12
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

    // Click on "getName" in `CurrentEnvironment::country()->getName()` on line 12
    let line_text = "CurrentEnvironment::country()->getName();";
    let col = line_text.find("getName").unwrap();

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: col as u32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve ->getName() on enum Country returned by static call"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "Country::getName() is declared on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_method_on_enum_returned_by_static_call_cross_file() {
    // Reproduces the real-world bug: the enum (`Country`) lives in a
    // different namespace (`Vendor\Enums`) than the class that returns it
    // (`App\CurrentEnvironment`).  The return type `Country` is only
    // resolvable via the `use Vendor\Enums\Country` in the *source* file,
    // NOT in the caller's file.  This fails if return types are not
    // resolved to FQN at parse time.
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/",
                "Vendor\\Enums\\": "vendor/enums/"
            }
        }
    }"#;

    let country_php = r#"<?php
namespace Vendor\Enums;

enum Country: string {
    case DK = 'DK';

    public function getName(): string {
        return 'Denmark';
    }
}
"#;

    let env_php = r#"<?php
namespace App;

use Vendor\Enums\Country;

final class CurrentEnvironment {
    public static function country(): Country {
        return Country::DK;
    }
}
"#;

    let controller_php = r#"<?php
namespace App\Http;

use App\CurrentEnvironment;

class MyController {
    public function index(): void {
        CurrentEnvironment::country()->getName();
    }
}
"#;

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("vendor/enums/Country.php", country_php),
            ("src/CurrentEnvironment.php", env_php),
            ("src/Http/MyController.php", controller_php),
        ],
    );

    let controller_uri = {
        let path = _dir.path().join("src/Http/MyController.php");
        Url::from_file_path(&path).unwrap()
    };

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: controller_uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: controller_php.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Click on "getName" in `CurrentEnvironment::country()->getName()` on line 7
    let line_text = "        CurrentEnvironment::country()->getName();";
    let col = line_text.find("getName").unwrap();

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: controller_uri.clone(),
            },
            position: Position {
                line: 7,
                character: col as u32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve ->getName() on enum Country returned by cross-file static call \
         where Country is in a different namespace (Vendor\\Enums) than the returning \
         class (App\\CurrentEnvironment)"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let country_uri = {
                let path = _dir.path().join("vendor/enums/Country.php");
                Url::from_file_path(&path).unwrap()
            };
            assert_eq!(
                location.uri, country_uri,
                "Should jump to Country.php in the vendor namespace"
            );
            assert_eq!(
                location.range.start.line, 6,
                "Country::getName() is declared on line 6 in Country.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Class constant via `ClassName::CONST` at the top level (outside any class).
/// This mirrors the `example.php` playground where `User::TYPE_ADMIN` is used
/// at file scope, not inside a method body.
#[tokio::test]
async fn test_goto_definition_class_constant_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///top_level_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public const string TYPE_ADMIN = 'admin';\n",
        "    public const string TYPE_USER = 'user';\n",
        "}\n",
        "\n",
        "User::TYPE_ADMIN;\n",
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

    // Click on "TYPE_ADMIN" in `User::TYPE_ADMIN` on line 6
    let line_text = "User::TYPE_ADMIN;";
    let name_pos = line_text.find("TYPE_ADMIN").unwrap();

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: name_pos as u32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve User::TYPE_ADMIN at top level to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "const TYPE_ADMIN is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Inherited constant via `ClassName::CONST` at the top level.
/// The constant is declared in a parent class.
#[tokio::test]
async fn test_goto_definition_inherited_constant_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inherited_const_top.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Model {\n",
        "    public const string CONNECTION = 'default';\n",
        "}\n",
        "class User extends Model {\n",
        "}\n",
        "\n",
        "User::CONNECTION;\n",
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

    // Click on "CONNECTION" in `User::CONNECTION` on line 7
    let line_text = "User::CONNECTION;";
    let name_pos = line_text.find("CONNECTION").unwrap();

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: name_pos as u32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve User::CONNECTION (inherited) at top level to Model's declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "const CONNECTION is declared on line 2 in Model"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_static_property_via_classname() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///static_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    public static string $defaultLocale = 'en';\n",
        "    protected static int $timeout = 30;\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $locale = Config::$defaultLocale;\n",
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

    // Click on "defaultLocale" in `Config::$defaultLocale` on line 8
    // The `$` is at character 26, so 'd' starts at character 27.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 8,
                character: 27,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve Config::$defaultLocale to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$defaultLocale is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Static property via `::` accessed from outside the class, cross-file.
#[tokio::test]
async fn test_goto_definition_static_property_via_classname_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Config.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "class Config {\n",
                "    public static string $appName = 'PHPantom';\n",
                "    public static bool $debug = false;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///consumer.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Config;\n",
        "class Consumer {\n",
        "    public function run(): void {\n",
        "        $name = Config::$appName;\n",
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

    // Click on "appName" in `Config::$appName` on line 4
    let line_text = "        $name = Config::$appName;";
    let dollar_pos = line_text.find("::$appName").unwrap() + 3; // position of '$'
    let name_pos = dollar_pos + 1; // position of 'a' in appName

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: name_pos as u32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve Config::$appName cross-file to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // Should jump to the Config.php file, line 3 (0-indexed)
            assert!(
                location.uri.as_str().contains("Config.php"),
                "Should jump to Config.php, got: {}",
                location.uri
            );
            assert_eq!(
                location.range.start.line, 3,
                "$appName is declared on line 3 of Config.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Static property via `self::$prop` inside the same class.
#[tokio::test]
async fn test_goto_definition_static_property_via_self() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///self_static_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Registry {\n",
        "    private static array $items = [];\n",
        "\n",
        "    public static function count(): int {\n",
        "        return count(self::$items);\n",
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

    // Click on "items" in `self::$items` on line 5
    let line_text = "        return count(self::$items);";
    let dollar_pos = line_text.find("::$items").unwrap() + 3;
    let name_pos = dollar_pos + 1; // 'i' in items

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: name_pos as u32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve self::$items to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "$items is declared on line 2");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Member Definition: Unresolvable Members ────────────────────────────────

#[tokio::test]
async fn test_goto_definition_unresolvable_member_returns_none() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unresolvable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/** Standalone values function — must NOT be the target. */\n",
        "function values(): array { return []; }\n",
        "\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        unknown_function()->values();\n",
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

    // Click on `values` in `unknown_function()->values()` on line 6
    let line_text = "        unknown_function()->values();";
    let val_col = line_text.find("->values(").unwrap() + 2; // position of 'v'

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: val_col as u32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    // Must be None — we must NOT fall through to the standalone `values()`.
    assert!(
        result.is_none(),
        "Expected None when owning class is unresolvable, but got: {:?}. \
         This means the member name fell through to standalone function lookup.",
        result,
    );
}

// ─── Short-Name Collision: Child & Parent Share Same Simple Name ────────────

#[tokio::test]
async fn test_goto_definition_inherited_method_same_short_name() {
    // Reproduces issue #41: when a child class and its parent share the
    // same short name (e.g. App\Console\Kernel extends
    // Illuminate\Foundation\Console\Kernel), go-to-definition on an
    // inherited member should jump to the parent file, not get confused
    // by the child's matching short name.
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/",
                    "Framework\\": "vendor/framework/src/"
                }
            }
        }"#,
        &[(
            "vendor/framework/src/Console/Kernel.php",
            concat!(
                "<?php\n",
                "namespace Framework\\Console;\n",
                "\n",
                "class Kernel {\n",
                "    protected function load(string $path): void {}\n",
                "    protected function commands(): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///app_kernel.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Console;\n",
        "\n",
        "use Framework\\Console\\Kernel as ConsoleKernel;\n",
        "\n",
        "class Kernel extends ConsoleKernel {\n",
        "    protected function commands(): void {\n",
        "        $this->load('routes');\n",
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

    // Click on "load" in `$this->load('routes')` on line 7
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve inherited $this->load() to parent Framework\\Console\\Kernel"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("vendor/framework/src/Console/Kernel.php"),
                "Should point to Framework\\Console\\Kernel.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 4,
                "function load is on line 4 of Framework\\Console\\Kernel.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_inherited_method_same_short_name_no_alias() {
    // Variant of #41 without a use-alias: parent is referenced by its
    // FQN directly in the extends clause.
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/",
                    "Vendor\\": "vendor/src/"
                }
            }
        }"#,
        &[(
            "vendor/src/Kernel.php",
            concat!(
                "<?php\n",
                "namespace Vendor;\n",
                "\n",
                "class Kernel {\n",
                "    public function bootstrap(): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///app_kernel2.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Kernel extends \\Vendor\\Kernel {\n",
        "    public function run(): void {\n",
        "        $this->bootstrap();\n",
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

    // Click on "bootstrap" in `$this->bootstrap()` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve inherited $this->bootstrap() to parent Vendor\\Kernel"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("vendor/src/Kernel.php"),
                "Should point to Vendor\\Kernel.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 4,
                "function bootstrap is on line 4 of Vendor\\Kernel.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_own_method_same_short_name_as_parent() {
    // When the child overrides the method, go-to-definition should still
    // find the child's own declaration even though a parent with the same
    // short name exists.
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/",
                    "Framework\\": "vendor/framework/src/"
                }
            }
        }"#,
        &[(
            "vendor/framework/src/Console/Kernel.php",
            concat!(
                "<?php\n",
                "namespace Framework\\Console;\n",
                "\n",
                "class Kernel {\n",
                "    protected function commands(): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///app_kernel3.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Console;\n",
        "\n",
        "use Framework\\Console\\Kernel as ConsoleKernel;\n",
        "\n",
        "class Kernel extends ConsoleKernel {\n",
        "    protected function commands(): void {\n",
        "        // overridden\n",
        "    }\n",
        "\n",
        "    public function run(): void {\n",
        "        $this->commands();\n",
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

    // Click on "commands" in `$this->commands()` on line 11
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->commands() to the child's own override"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // The child overrides commands(), so it should point to the
            // current file, not the parent.
            assert_eq!(
                location.uri.as_str(),
                uri.as_str(),
                "Should point to current file (child override)"
            );
            assert_eq!(
                location.range.start.line, 6,
                "function commands is on line 6 of child Kernel"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── GTD self-reference suppression ─────────────────────────────────────────

/// Ctrl+Click on a method name at its own declaration site should return
/// self-location so editors can fall back to Find References.
#[tokio::test]
async fn test_goto_definition_method_declaration_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///self_ref_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class GtdSelfRef {\n",
        "    public function paramTypes(string $item): void {\n",
        "        echo $item;\n",
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

    // Click on "paramTypes" in the declaration `public function paramTypes(`
    // on line 2, starting at character 20.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "should point back to line 2");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// Ctrl+Click on a class name at its own declaration site should return
/// self-location so editors can fall back to Find References.
#[tokio::test]
async fn test_goto_definition_class_declaration_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///self_ref_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class MyUniqueTestClass {\n",
        "    public function foo(): void {}\n",
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

    // Click on "MyUniqueTestClass" in `class MyUniqueTestClass {` on line 1.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 10,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 1, "should point back to line 1");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// Ctrl+Click on a constant name inside its own `define()` call should
/// return `None` rather than jumping to itself.
#[tokio::test]
async fn test_goto_definition_define_constant_at_definition_returns_none() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///self_ref_define.php").unwrap();
    let text = concat!(
        "<?php\n",
        "define('APP_VERSION', '1.0.0');\n",
        "\n",
        "echo APP_VERSION;\n",
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

    // Click on "APP_VERSION" inside `define('APP_VERSION', ...)` on line 1.
    // The constant name starts at character 8 (after `define('`).
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_none(),
        "GTD on a constant name inside its own define() call should return None, got: {:?}",
        result
    );

    // Verify that clicking on the *usage* of APP_VERSION still works.
    let usage_params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 3,
                character: 7,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let usage_result = backend.goto_definition(usage_params).await.unwrap();
    assert!(
        usage_result.is_some(),
        "GTD on a constant usage should still resolve to the define() call"
    );
}

// ─── Member Definition: Array Function Results ──────────────────────────────

/// GTD on `$last->write()` where `$last = array_pop($pens)` and
/// `$pens = $this->getPens()` (same file, no namespaces).
#[tokio::test]
async fn test_goto_definition_method_on_array_pop_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_pop_gtd.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Pen {\n",                                         // 1
        "    public function write(): void {}\n",                // 2
        "}\n",                                                   // 3
        "class Holder {\n",                                      // 4
        "    /** @return list<Pen> */\n",                        // 5
        "    public function getPens(): array { return []; }\n", // 6
        "    public function test(): void {\n",                  // 7
        "        $pens = $this->getPens();\n",                   // 8
        "        $last = array_pop($pens);\n",                   // 9
        "        $last->write();\n",                             // 10
        "    }\n",                                               // 11
        "}\n",                                                   // 12
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

    // Click on "write" in `$last->write()` on line 10
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $last->write() via array_pop + $this->getPens()"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "write() is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// GTD on `$last->write()` where `$last = array_pop($pens)` and
/// `$pens = $src->roster()` — `$src` is a variable (not `$this`),
/// and classes live in a namespace across files.
#[tokio::test]
async fn test_goto_definition_method_on_array_pop_var_method_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Demo\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Pen.php",
                concat!(
                    "<?php\n",
                    "namespace Demo;\n",
                    "\n",
                    "class Pen {\n",
                    "    public function write(): void {}\n",
                    "}\n",
                ),
            ),
            (
                "src/Source.php",
                concat!(
                    "<?php\n",
                    "namespace Demo;\n",
                    "\n",
                    "class Source {\n",
                    "    /** @return list<Pen> */\n",
                    "    public function roster(): array { return []; }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///consumer.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "namespace Demo;\n",                              // 1
        "\n",                                             // 2
        "class Consumer {\n",                             // 3
        "    public function run(Source $src): void {\n", // 4
        "        $pens = $src->roster();\n",              // 5
        "        $last = array_pop($pens);\n",            // 6
        "        $last->write();\n",                      // 7
        "    }\n",                                        // 8
        "}\n",                                            // 9
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

    // Click on "write" in `$last->write()` on line 7
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $last->write() via array_pop + $src->roster() cross-file"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Pen.php"),
                "Should point to Pen.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 4,
                "write() is declared on line 4 of Pen.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// GTD on `$last->write()` where `$last = array_pop($pens)`,
/// `$pens = $src->roster()`, and the consumer lives in a *different*
/// namespace from Source/Pen and uses an explicit `use` import.
#[tokio::test]
async fn test_goto_definition_method_on_array_pop_different_namespace() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Demo\\": "src/Demo/",
                    "App\\": "src/App/"
                }
            }
        }"#,
        &[
            (
                "src/Demo/Pen.php",
                concat!(
                    "<?php\n",
                    "namespace Demo;\n",
                    "\n",
                    "class Pen {\n",
                    "    public function write(): void {}\n",
                    "}\n",
                ),
            ),
            (
                "src/Demo/Source.php",
                concat!(
                    "<?php\n",
                    "namespace Demo;\n",
                    "\n",
                    "class Source {\n",
                    "    /** @return list<Pen> */\n",
                    "    public function roster(): array { return []; }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///consumer.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "namespace App;\n",                               // 1
        "\n",                                             // 2
        "use Demo\\Source;\n",                            // 3
        "\n",                                             // 4
        "class Consumer {\n",                             // 5
        "    public function run(Source $src): void {\n", // 6
        "        $pens = $src->roster();\n",              // 7
        "        $last = array_pop($pens);\n",            // 8
        "        $last->write();\n",                      // 9
        "    }\n",                                        // 10
        "}\n",                                            // 11
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

    // Click on "write" in `$last->write()` on line 9
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $last->write() via array_pop + $src->roster() across namespaces"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Demo/Pen.php"),
                "Should point to Demo/Pen.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 4,
                "write() is declared on line 4 of Pen.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Member Definition: Generator Yield Inference ───────────────────────────

/// GTD on `$user->getEmail()` where `$user` is inferred from generator
/// yield type `@return \Generator<int, User>` (same file, method body).
#[tokio::test]
async fn test_goto_definition_method_on_generator_yield_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///gen_yield_gtd.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "class User {\n",                                 // 1
        "    public string $name;\n",                     // 2
        "    public function getEmail(): string {}\n",    // 3
        "}\n",                                            // 4
        "class UserRepository {\n",                       // 5
        "    /** @return \\Generator<int, User> */\n",    // 6
        "    public function findAll(): \\Generator {\n", // 7
        "        yield $user;\n",                         // 8
        "        $user->getEmail();\n",                   // 9
        "    }\n",                                        // 10
        "}\n",                                            // 11
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

    // Click on "getEmail" in `$user->getEmail()` on line 9
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $user->getEmail() via generator yield type inference"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "getEmail() is declared on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// GTD on `$product->title` where `$product` is inferred from a
/// key-value yield: `yield 0 => $product` with
/// `@return \Generator<int, Product>`.
#[tokio::test]
async fn test_goto_definition_property_on_generator_yield_pair_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///gen_yield_pair_gtd.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "class Product {\n",                              // 1
        "    public string $title;\n",                    // 2
        "}\n",                                            // 3
        "class ProductLoader {\n",                        // 4
        "    /** @return \\Generator<int, Product> */\n", // 5
        "    public function loadAll(): \\Generator {\n", // 6
        "        yield 0 => $product;\n",                 // 7
        "        $product->title;\n",                     // 8
        "    }\n",                                        // 9
        "}\n",                                            // 10
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

    // Click on "title" in `$product->title` on line 8
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 8,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $product->title via generator yield pair type inference"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "$title is declared on line 2");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// GTD on `$customer->name` where `$customer` is inferred from a
/// top-level generator function (not a method).
#[tokio::test]
async fn test_goto_definition_method_on_generator_yield_top_level_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///gen_yield_toplevel_gtd.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // 0
        "class Customer {\n",                            // 1
        "    public string $name;\n",                    // 2
        "    public function greet(): string {}\n",      // 3
        "}\n",                                           // 4
        "/** @return \\Generator<int, Customer> */\n",   // 5
        "function generateCustomers(): \\Generator {\n", // 6
        "    yield $customer;\n",                        // 7
        "    $customer->greet();\n",                     // 8
        "}\n",                                           // 9
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

    // Click on "greet" in `$customer->greet()` on line 8
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 8,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $customer->greet() via generator yield inference in top-level function"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "greet() is declared on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// GTD on `$user->getEmail()` where `$user` is inferred from generator
/// yield type and User is defined in another file via PSR-4.
#[tokio::test]
async fn test_goto_definition_method_on_generator_yield_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/User.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "\n",
                "class User {\n",
                "    public function getEmail(): string {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///repo.php").unwrap();
    let text = concat!(
        "<?php\n",                                        // 0
        "namespace App;\n",                               // 1
        "\n",                                             // 2
        "class UserRepository {\n",                       // 3
        "    /** @return \\Generator<int, User> */\n",    // 4
        "    public function findAll(): \\Generator {\n", // 5
        "        yield $user;\n",                         // 6
        "        $user->getEmail();\n",                   // 7
        "    }\n",                                        // 8
        "}\n",                                            // 9
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

    // Click on "getEmail" in `$user->getEmail()` on line 7
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $user->getEmail() via generator yield inference cross-file"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/User.php"),
                "Should point to User.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 4,
                "getEmail() is declared on line 4 of User.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Trait Use Alias / Insteadof Go-To-Definition ───────────────────────────

/// Clicking on the alias name in `method as alias` should jump to the
/// original method definition in the trait.
#[tokio::test]
async fn test_goto_definition_trait_alias_name_jumps_to_original_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_alias.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Notifiable {\n",
        "    public function routeNotificationFor(): mixed { return null; }\n",
        "}\n",
        "class User {\n",
        "    use Notifiable {\n",
        "        routeNotificationFor as _routeNotificationFor;\n",
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

    // Click on `_routeNotificationFor` (the alias name) on line 6
    let alias_col = text
        .lines()
        .nth(6)
        .unwrap()
        .find("_routeNotificationFor")
        .unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: alias_col as u32 + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve trait alias _routeNotificationFor to the original method"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // routeNotificationFor is declared on line 2 of the trait
            assert_eq!(
                location.range.start.line, 2,
                "Should point to routeNotificationFor in Notifiable trait (line 2)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Clicking on the original method name in `method as alias` should jump to
/// the method definition in the trait.
#[tokio::test]
async fn test_goto_definition_trait_alias_original_method_reference() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_alias_orig.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Notifiable {\n",
        "    public function routeNotificationFor(): mixed { return null; }\n",
        "}\n",
        "class User {\n",
        "    use Notifiable {\n",
        "        routeNotificationFor as _routeNotificationFor;\n",
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

    // Click on `routeNotificationFor` (the original method name) on line 6
    let method_col = text
        .lines()
        .nth(6)
        .unwrap()
        .find("routeNotificationFor")
        .unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: method_col as u32 + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve original method name routeNotificationFor in alias declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                location.range.start.line, 2,
                "Should point to routeNotificationFor in Notifiable trait (line 2)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Clicking on the qualified method name in `Trait::method as alias` should
/// jump to the method definition in the specified trait.
#[tokio::test]
async fn test_goto_definition_trait_alias_qualified_method_reference() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_alias_qual.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait TraitA {\n",
        "    public function shared(): void {}\n",
        "}\n",
        "trait TraitB {\n",
        "    public function shared(): void {}\n",
        "}\n",
        "class Widget {\n",
        "    use TraitA, TraitB {\n",
        "        TraitA::shared insteadof TraitB;\n",
        "        TraitB::shared as sharedFromB;\n",
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

    // Click on `shared` in `TraitB::shared as sharedFromB` on line 10
    let line10 = text.lines().nth(10).unwrap();
    let shared_col = line10.find("shared").unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: shared_col as u32 + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve TraitB::shared in alias declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // TraitB::shared is declared on line 5
            assert_eq!(
                location.range.start.line, 5,
                "Should point to shared() in TraitB (line 5)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Clicking on a trait name in `Trait::method insteadof OtherTrait` should
/// navigate to the trait definition.
#[tokio::test]
async fn test_goto_definition_trait_name_in_insteadof() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_insteadof.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait TraitA {\n",
        "    public function shared(): void {}\n",
        "}\n",
        "trait TraitB {\n",
        "    public function shared(): void {}\n",
        "}\n",
        "class Widget {\n",
        "    use TraitA, TraitB {\n",
        "        TraitA::shared insteadof TraitB;\n",
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

    // Click on `TraitB` in `insteadof TraitB` on line 9
    let line9 = text.lines().nth(9).unwrap();
    let insteadof_trait_col = line9.rfind("TraitB").unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: insteadof_trait_col as u32 + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve TraitB in insteadof to the trait definition"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // TraitB is declared on line 4
            assert_eq!(
                location.range.start.line, 4,
                "Should point to TraitB declaration (line 4)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Clicking on a method name in `Trait::method insteadof OtherTrait` should
/// jump to the method definition in the specified trait.
#[tokio::test]
async fn test_goto_definition_method_in_insteadof() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_insteadof_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait TraitA {\n",
        "    public function shared(): void {}\n",
        "}\n",
        "trait TraitB {\n",
        "    public function shared(): void {}\n",
        "}\n",
        "class Widget {\n",
        "    use TraitA, TraitB {\n",
        "        TraitA::shared insteadof TraitB;\n",
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

    // Click on `shared` in `TraitA::shared insteadof` on line 9
    let line9 = text.lines().nth(9).unwrap();
    let shared_col = line9.find("shared").unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: shared_col as u32 + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve method name in insteadof declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // TraitA::shared is declared on line 2
            assert_eq!(
                location.range.start.line, 2,
                "Should point to shared() in TraitA (line 2)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Clicking on a trait name in `Trait::method as alias` should navigate
/// to the trait definition.
#[tokio::test]
async fn test_goto_definition_trait_name_in_alias_adaptation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_alias_traitname.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait TraitA {\n",
        "    public function shared(): void {}\n",
        "}\n",
        "trait TraitB {\n",
        "    public function shared(): void {}\n",
        "}\n",
        "class Widget {\n",
        "    use TraitA, TraitB {\n",
        "        TraitA::shared insteadof TraitB;\n",
        "        TraitB::shared as sharedFromB;\n",
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

    // Click on `TraitB` in `TraitB::shared as sharedFromB` on line 10
    let line10 = text.lines().nth(10).unwrap();
    let trait_col = line10.find("TraitB").unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: trait_col as u32 + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve TraitB name in alias adaptation to its definition"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // TraitB is declared on line 4
            assert_eq!(
                location.range.start.line, 4,
                "Should point to TraitB declaration (line 4)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Cross-file: clicking on a trait alias name should jump to the method
/// definition in the trait source file.
#[tokio::test]
async fn test_goto_definition_trait_alias_cross_file() {
    let trait_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "trait HasNotifications {\n",
        "    public function routeNotificationFor(): mixed { return null; }\n",
        "}\n",
    );
    let class_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "class User {\n",
        "    use HasNotifications {\n",
        "        routeNotificationFor as _routeNotificationFor;\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            ("src/HasNotifications.php", trait_php),
            ("src/User.php", class_php),
        ],
    );

    let class_uri = Url::from_file_path(_dir.path().join("src/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: class_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: class_php.to_string(),
            },
        })
        .await;

    let trait_uri = Url::from_file_path(_dir.path().join("src/HasNotifications.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: trait_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: trait_php.to_string(),
            },
        })
        .await;

    // Click on `_routeNotificationFor` (alias) on line 4 of User.php
    let alias_col = class_php
        .lines()
        .nth(4)
        .unwrap()
        .find("_routeNotificationFor")
        .unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: class_uri.clone(),
            },
            position: Position {
                line: 4,
                character: alias_col as u32 + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve cross-file trait alias _routeNotificationFor"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("HasNotifications.php"),
                "Should point to HasNotifications.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 3,
                "routeNotificationFor is declared on line 3 of HasNotifications.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// When a class uses a trait with `foo as __foo` AND also declares its own
/// `foo()`, clicking `$this->__foo()` should jump to the trait's `foo()`
/// method, not the class's own `foo()`.
#[tokio::test]
async fn test_goto_definition_trait_alias_when_class_overrides_original_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_alias_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Foo {\n",
        "    public function foo(): string { return 'foo'; }\n",
        "}\n",
        "class User {\n",
        "    use Foo {\n",
        "        foo as __foo;\n",
        "    }\n",
        "    public function foo(): string {\n",
        "        return $this->__foo() . 'bar';\n",
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

    // Click on `__foo` in `$this->__foo()` on line 9
    let line9 = text.lines().nth(9).unwrap();
    let alias_col = line9.find("__foo").unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: alias_col as u32 + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->__foo() to the trait's foo() method"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // Foo::foo() is declared on line 2 (the trait method)
            assert_eq!(
                location.range.start.line, 2,
                "Should point to foo() in trait Foo (line 2), not User::foo() (line 8)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Clicking on the alias name in `foo as __foo` when the class also
/// declares its own `foo()` should jump to the trait's `foo()` method.
#[tokio::test]
async fn test_goto_definition_trait_alias_decl_when_class_overrides_original_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_alias_decl_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Foo {\n",
        "    public function foo(): string { return 'foo'; }\n",
        "}\n",
        "class User {\n",
        "    use Foo {\n",
        "        foo as __foo;\n",
        "    }\n",
        "    public function foo(): string {\n",
        "        return $this->__foo() . 'bar';\n",
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

    // Click on `__foo` in `foo as __foo;` on line 6
    let line6 = text.lines().nth(6).unwrap();
    let alias_col = line6.find("__foo").unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: alias_col as u32 + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve alias __foo in trait use block to the trait's foo()"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // Foo::foo() is declared on line 2
            assert_eq!(
                location.range.start.line, 2,
                "Should point to foo() in trait Foo (line 2), not User::foo() (line 8)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Clicking on the original method name `foo` in `foo as __foo` should
/// jump to the trait's `foo()` method even when the class overrides it.
#[tokio::test]
async fn test_goto_definition_trait_alias_original_name_when_class_overrides() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_alias_orig_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Foo {\n",
        "    public function foo(): string { return 'foo'; }\n",
        "}\n",
        "class User {\n",
        "    use Foo {\n",
        "        foo as __foo;\n",
        "    }\n",
        "    public function foo(): string {\n",
        "        return $this->__foo() . 'bar';\n",
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

    // Click on `foo` (the original method name) in `foo as __foo;` on line 6
    let line6 = text.lines().nth(6).unwrap();
    let foo_col = line6.find("foo").unwrap();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: foo_col as u32 + 1,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve original method name foo in alias adaptation"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // Foo::foo() is declared on line 2
            assert_eq!(
                location.range.start.line, 2,
                "Should point to foo() in trait Foo (line 2), not User::foo() (line 8)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── @see Tag GTD in Floating Docblocks ─────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_see_tag_in_array_no_namespace() {
    // Regression: @see tags in floating docblocks (e.g. inside array
    // literals, in files without a namespace) were not parsed because
    // the docblock wasn't attached to any AST node.
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_see_array.php").unwrap();
    // Line 0: <?php
    // Line 1: class SupervisorOptions {
    // Line 2:     public int $balanceCooldown = 3;
    // Line 3: }
    // Line 4: $defaults = [
    // Line 5:     'balanceCooldown' => 3, /** @see SupervisorOptions::$balanceCooldown */
    // Line 6: ];
    let text = concat!(
        "<?php\n",
        "class SupervisorOptions {\n",
        "    public int $balanceCooldown = 3;\n",
        "}\n",
        "$defaults = [\n",
        "    'balanceCooldown' => 3, /** @see SupervisorOptions::$balanceCooldown */\n",
        "];\n",
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

    // Click on "SupervisorOptions" in the @see tag (line 5)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 40, // within "SupervisorOptions"
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve @see class reference in floating docblock inside array literal"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                location.range.start.line, 1,
                "SupervisorOptions is defined on line 1"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // Click on "$balanceCooldown" in the @see tag (line 5)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 62, // within "$balanceCooldown"
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve @see member reference in floating docblock inside array literal"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                location.range.start.line, 2,
                "$balanceCooldown property is defined on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_see_tag_inline_in_expression() {
    // A floating /** @see ... */ after an expression statement, not
    // attached to any class or function.
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_see_inline.php").unwrap();
    // Line 0: <?php
    // Line 1: class Config {
    // Line 2:     public string $timeout = '30';
    // Line 3: }
    // Line 4:
    // Line 5: $timeout = 30; /** @see Config::$timeout */
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    public string $timeout = '30';\n",
        "}\n",
        "\n",
        "$timeout = 30; /** @see Config::$timeout */\n",
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

    // Click on "Config" in the @see tag (line 5)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 25, // within "Config"
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve @see class reference in trailing docblock after expression"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.range.start.line, 1, "Config is defined on line 1");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_see_tag_cross_file_no_namespace() {
    // @see in a file without namespace pointing to a class in another file.
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/SupervisorOptions.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "class SupervisorOptions {\n",
                "    public int $balanceCooldown = 3;\n",
                "}\n",
            ),
        )],
    );

    let config_uri = Url::parse("file:///config/horizon.php").unwrap();
    let config_text = concat!(
        "<?php\n",
        "use App\\Models\\SupervisorOptions;\n",
        "$defaults = [\n",
        "    'balanceCooldown' => 3, /** @see SupervisorOptions::$balanceCooldown */\n",
        "];\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: config_uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: config_text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Click on "SupervisorOptions" in the @see tag (line 3)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: config_uri.clone(),
            },
            position: Position {
                line: 3,
                character: 40, // within "SupervisorOptions"
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve @see class reference cross-file from a no-namespace config file"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("Models/SupervisorOptions.php"),
                "Should point to SupervisorOptions.php, got: {:?}",
                path
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_see_tag_self_member_in_class_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_self_see.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "\n",
        "    /**\n",
        "     * @see self::bar()\n",
        "     */\n",
        "    public function baz(): void {}\n",
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

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve @see self::bar() member reference"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.range.start.line, 2, "bar() is defined on line 2");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_chained_property_cross_file() {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/UserRepository.php",
                "<?php\nnamespace App;\n\nclass UserRepository {\n    public function getById(int $id): void {}\n}\n",
            ),
            (
                "src/UserService.php",
                "<?php\nnamespace App;\n\nuse App\\UserRepository;\n\nclass UserService {\n    private UserRepository $userRepository;\n\n    public function find(int $id): void {\n        $this->userRepository->getById($id);\n    }\n}\n",
            ),
        ],
    );

    // Open UserService.php
    let svc_path = dir.path().join("src/UserService.php");
    let svc_uri = Url::from_file_path(&svc_path).unwrap();
    let svc_content = std::fs::read_to_string(&svc_path).unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: svc_uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: svc_content,
        },
    };
    backend.did_open(open_params).await;

    // Click on "getById" in `$this->userRepository->getById($id)` on line 9
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: svc_uri.clone(),
            },
            position: Position {
                line: 9,
                character: 35,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->userRepository->getById() cross-file via PSR-4"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert!(
                location.uri.path().ends_with("UserRepository.php"),
                "Should jump to UserRepository.php, got: {}",
                location.uri
            );
            assert_eq!(
                location.range.start.line, 4,
                "getById is declared on line 4 in UserRepository.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_chained_property_cross_file_docblock() {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/UserRepository.php",
                "<?php\nnamespace App;\n\nclass UserRepository {\n    public function getById(int $id): void {}\n}\n",
            ),
            (
                "src/UserService.php",
                "<?php\nnamespace App;\n\nuse App\\UserRepository;\n\nclass UserService {\n    /** @var UserRepository */\n    private $userRepository;\n\n    public function find(int $id): void {\n        $this->userRepository->getById($id);\n    }\n}\n",
            ),
        ],
    );

    let svc_path = dir.path().join("src/UserService.php");
    let svc_uri = Url::from_file_path(&svc_path).unwrap();
    let svc_content = std::fs::read_to_string(&svc_path).unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: svc_uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: svc_content,
        },
    };
    backend.did_open(open_params).await;

    // Click on "getById" on line 10
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: svc_uri.clone(),
            },
            position: Position {
                line: 10,
                character: 35,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->userRepository->getById() cross-file via @var docblock"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert!(
                location.uri.path().ends_with("UserRepository.php"),
                "Should jump to UserRepository.php, got: {}",
                location.uri
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_chained_property_docblock_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // 0
        "class UserRepository {\n",                        // 1
        "    public function getById(int $id): void {}\n", // 2
        "}\n",                                             // 3
        "\n",                                              // 4
        "class UserService {\n",                           // 5
        "    /** @var UserRepository */\n",                // 6
        "    private $userRepository;\n",                  // 7
        "\n",                                              // 8
        "    public function find(int $id): void {\n",     // 9
        "        $this->userRepository->getById($id);\n",  // 10
        "    }\n",                                         // 11
        "}\n",                                             // 12
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

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: 35,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->userRepository->getById() via @var docblock type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "getById is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_chain_with_trait_and_promoted_props() {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/SomeTrait.php",
                "<?php\nnamespace App;\n\ntrait SomeTrait {\n    private function helper(): bool { return true; }\n}\n",
            ),
            (
                "src/Repository.php",
                "<?php\nnamespace App;\n\nclass Repository {\n    public function findById(int $id): void {}\n}\n",
            ),
            (
                "src/Service.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "\n",
                    "use App\\SomeTrait;\n",
                    "use App\\Repository;\n",
                    "\n",
                    "class Service {\n",
                    "    use SomeTrait;\n",
                    "\n",
                    "    private Repository $repository;\n",
                    "\n",
                    "    public function __construct(\n",
                    "        private Repository $otherRepo,\n",
                    "    ) {\n",
                    "        $this->repository = new Repository();\n",
                    "    }\n",
                    "\n",
                    "    public function handle(int $id): void {\n",
                    "        $this->repository->findById($id);\n",
                    "        $this->otherRepo->findById($id);\n",
                    "    }\n",
                    "}\n",
                ),
            ),
        ],
    );

    // Only open Service.php — Repository.php and SomeTrait.php stay closed.
    let svc_path = dir.path().join("src/Service.php");
    let svc_uri = Url::from_file_path(&svc_path).unwrap();
    let svc_content = std::fs::read_to_string(&svc_path).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: svc_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: svc_content.clone(),
            },
        })
        .await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: svc_uri.clone(),
            },
            position: Position {
                line: 18,
                character: 34,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->repository->findById() cross-file (regular property)"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert!(
                location.uri.path().ends_with("Repository.php"),
                "Should jump to Repository.php, got: {}",
                location.uri
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }

    let params2 = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: svc_uri.clone(),
            },
            position: Position {
                line: 19,
                character: 32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result2 = backend.goto_definition(params2).await.unwrap();
    assert!(
        result2.is_some(),
        "Should resolve $this->otherRepo->findById() cross-file (promoted property)"
    );
    match result2.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert!(
                location.uri.path().ends_with("Repository.php"),
                "Should jump to Repository.php, got: {}",
                location.uri
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_chain_untyped_property_constructor_assigned() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Repository {\n",                             // 1
        "    public function findById(int $id): void {}\n", // 2
        "}\n",                                              // 3
        "\n",                                               // 4
        "class Service {\n",                                // 5
        "    private $repository;\n",                       // 6
        "\n",                                               // 7
        "    public function __construct() {\n",            // 8
        "        $this->repository = new Repository();\n",  // 9
        "    }\n",                                          // 10
        "\n",                                               // 11
        "    public function handle(int $id): void {\n",    // 12
        "        $this->repository->findById($id);\n",      // 13
        "    }\n",                                          // 14
        "}\n",                                              // 15
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

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 33,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap().expect(
        "Should resolve $this->repository->findById() via constructor assignment type inference",
    );

    match result {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "findById is declared on line 2 (untyped constructor-assigned)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_chain_promoted_property_new_default() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class Repository {\n",                              // 1
        "    public function findById(int $id): void {}\n",  // 2
        "}\n",                                               // 3
        "\n",                                                // 4
        "class Service {\n",                                 // 5
        "    public function __construct(\n",                // 6
        "        private $repository = new Repository(),\n", // 7
        "    ) {}\n",                                        // 8
        "\n",                                                // 9
        "    public function handle(int $id): void {\n",     // 10
        "        $this->repository->findById($id);\n",       // 11
        "    }\n",                                           // 12
        "}\n",                                               // 13
    );

    let open_params: DidOpenTextDocumentParams = DidOpenTextDocumentParams {
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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 33,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    let result = result.expect(
        "Should resolve $this->repository->findById() via promoted property new-default type inference",
    );

    match result {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "findById is declared on line 2 (promoted new-default)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_chain_untyped_constructor_assigned_cross_file_fqn() {
    // The class name in `new Repository()` is a short name imported via `use`.
    // Verify that constructor-inferred types resolve to FQN so that
    // cross-file go-to-definition works without short-name fallback logic.
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Repository.php",
                "<?php\nnamespace App;\n\nclass Repository {\n    public function findById(int $id): void {}\n}\n",
            ),
            (
                "src/Service.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "\n",
                    "use App\\Repository;\n",
                    "\n",
                    "class Service {\n",
                    "    private $repository;\n",
                    "\n",
                    "    public function __construct() {\n",
                    "        $this->repository = new Repository();\n",
                    "    }\n",
                    "\n",
                    "    public function handle(int $id): void {\n",
                    "        $this->repository->findById($id);\n",
                    "    }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let svc_path = dir.path().join("src/Service.php");
    let svc_uri = Url::from_file_path(&svc_path).unwrap();
    let svc_content = std::fs::read_to_string(&svc_path).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: svc_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: svc_content,
            },
        })
        .await;

    // Click on "findById" in `$this->repository->findById($id)` on line 13
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: svc_uri.clone(),
            },
            position: Position {
                line: 13,
                character: 34,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->repository->findById() cross-file via constructor-inferred FQN"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert!(
                location.uri.path().ends_with("Repository.php"),
                "Should jump to Repository.php, got: {}",
                location.uri
            );
            assert_eq!(
                location.range.start.line, 4,
                "findById is declared on line 4 in Repository.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_chain_promoted_new_default_cross_file_fqn() {
    // Promoted parameter with `new ClassName()` default where the class
    // name is a short name imported via `use`. Verify FQN resolution.
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Repository.php",
                "<?php\nnamespace App;\n\nclass Repository {\n    public function findById(int $id): void {}\n}\n",
            ),
            (
                "src/Service.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "\n",
                    "use App\\Repository;\n",
                    "\n",
                    "class Service {\n",
                    "    public function __construct(\n",
                    "        private $repository = new Repository(),\n",
                    "    ) {}\n",
                    "\n",
                    "    public function handle(int $id): void {\n",
                    "        $this->repository->findById($id);\n",
                    "    }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let svc_path = dir.path().join("src/Service.php");
    let svc_uri = Url::from_file_path(&svc_path).unwrap();
    let svc_content = std::fs::read_to_string(&svc_path).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: svc_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: svc_content,
            },
        })
        .await;

    // Click on "findById" in `$this->repository->findById($id)` on line 11
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: svc_uri.clone(),
            },
            position: Position {
                line: 11,
                character: 34,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->repository->findById() cross-file via promoted-param new-default FQN"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert!(
                location.uri.path().ends_with("Repository.php"),
                "Should jump to Repository.php, got: {}",
                location.uri
            );
            assert_eq!(
                location.range.start.line, 4,
                "findById is declared on line 4 in Repository.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_untyped_inference_skipped_when_native_type_exists() {
    // A property with a native type hint should not be overridden by
    // constructor assignment inference, even if the constructor assigns
    // a different class.
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "class Dog {\n",                         // 1
        "    public function bark(): void {}\n", // 2
        "}\n",                                   // 3
        "class Cat {\n",                         // 4
        "    public function meow(): void {}\n", // 5
        "}\n",                                   // 6
        "\n",                                    // 7
        "class Shelter {\n",                     // 8
        "    private Dog $pet;\n",               // 9
        "\n",                                    // 10
        "    public function __construct() {\n", // 11
        "        $this->pet = new Cat();\n",     // 12
        "    }\n",                               // 13
        "\n",                                    // 14
        "    public function demo(): void {\n",  // 15
        "        $this->pet->bark();\n",         // 16
        "    }\n",                               // 17
        "}\n",                                   // 18
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

    // Click on "bark" — should resolve to Dog::bark(), not Cat
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 16,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->pet->bark() via native Dog type, not constructor-inferred Cat"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                location.range.start.line, 2,
                "bark() is on Dog (line 2), not Cat"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_untyped_inference_skipped_when_docblock_type_exists() {
    // A property with a @var docblock type should not be overridden by
    // constructor assignment inference.
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "class Dog {\n",                         // 1
        "    public function bark(): void {}\n", // 2
        "}\n",                                   // 3
        "class Cat {\n",                         // 4
        "    public function meow(): void {}\n", // 5
        "}\n",                                   // 6
        "\n",                                    // 7
        "class Shelter {\n",                     // 8
        "    /** @var Dog */\n",                 // 9
        "    private $pet;\n",                   // 10
        "\n",                                    // 11
        "    public function __construct() {\n", // 12
        "        $this->pet = new Cat();\n",     // 13
        "    }\n",                               // 14
        "\n",                                    // 15
        "    public function demo(): void {\n",  // 16
        "        $this->pet->bark();\n",         // 17
        "    }\n",                               // 18
        "}\n",                                   // 19
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

    // Click on "bark" — should resolve to Dog::bark() via @var, not Cat
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
        "Should resolve $this->pet->bark() via @var Dog, not constructor-inferred Cat"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                location.range.start.line, 2,
                "bark() is on Dog (line 2), not Cat"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}
