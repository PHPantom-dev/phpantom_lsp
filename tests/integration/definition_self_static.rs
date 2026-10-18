use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Goto Definition: new self() / new static() / new parent() ─────────────

#[tokio::test]
async fn test_goto_definition_new_self_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class SessionManager {\n",
        "    public function create(): self {\n",
        "        return new self();\n",
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

    // Click on "self" in `new self()` on line 3
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `new self()` to the enclosing class"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "class SessionManager is on line 1"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_new_static_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class SessionManager {\n",
        "    public function create(): static {\n",
        "        return new static();\n",
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

    // Click on "static" in `new static()` on line 3
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `new static()` to the enclosing class"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "class SessionManager is on line 1"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_new_parent_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Manager {\n",
        "    public function boot(): void {}\n",
        "}\n",
        "\n",
        "class SessionManager extends Manager {\n",
        "    protected function callCustomCreator(): void {\n",
        "        new parent();\n",
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

    // Click on "parent" in `new parent()` on line 7
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `new parent()` to the parent class Manager"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 1, "class Manager is on line 1");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_parent_imported_from_another_namespace() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Lib {\n",
        "    class Manager {}\n",
        "}\n",
        "namespace App {\n",
        "    use Lib\\Manager;\n",
        "    class SessionManager extends Manager {\n",
        "        protected function callCustomCreator(): void {\n",
        "            new parent();\n",
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

    // Click on "parent" in `new parent()` on line 8
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
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "class Manager is on line 2");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_new_parent_cross_file_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Manager.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "\n",
                "class Manager {\n",
                "    public function boot(): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class SessionManager extends Manager {\n",
        "    protected function callCustomCreator(): void {\n",
        "        new parent();\n",
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

    // Click on "parent" in `new parent()` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `new parent()` to cross-file Manager class via PSR-4"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/Manager.php"),
                "Should point to Manager.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 3,
                "class Manager is on line 3 in Manager.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_self_outside_class_returns_none() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    // `self` used outside any class — should not resolve
    let text = concat!("<?php\n", "$x = new self();\n",);

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Click on "self" on line 1
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 1,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_none(),
        "`new self()` outside a class should not resolve"
    );
}

#[tokio::test]
async fn test_goto_definition_new_self_in_nested_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Factory {\n",
        "    public function build(): void {}\n",
        "\n",
        "    public static function create(): self {\n",
        "        $instance = new self();\n",
        "        return $instance;\n",
        "    }\n",
        "\n",
        "    public static function createStatic(): static {\n",
        "        $instance = new static();\n",
        "        return $instance;\n",
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

    // Click on "self" in `new self()` on line 7
    let params_self = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params_self).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `new self()` inside namespaced class"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 3, "class Factory is on line 3");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // Click on "static" in `new static()` on line 12
    let params_static = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params_static).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `new static()` inside namespaced class"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 3, "class Factory is on line 3");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Goto Definition: new ClassName()->method() ─────────────────────────────

#[tokio::test]
async fn test_goto_definition_new_classname_arrow_method_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class SessionManager {\n",
        "    public function callCustomCreator(): void {}\n",
        "    public function boot(): void {\n",
        "        new SessionManager()->callCustomCreator();\n",
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

    // Click on "callCustomCreator" in `new SessionManager()->callCustomCreator()` on line 4
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `new SessionManager()->callCustomCreator()` to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "callCustomCreator is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_parenthesized_new_arrow_method_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class SessionManager {\n",
        "    public function callCustomCreator(): void {}\n",
        "    public function boot(): void {\n",
        "        (new SessionManager())->callCustomCreator();\n",
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

    // Click on "callCustomCreator" in `(new SessionManager())->callCustomCreator()` on line 4
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 33,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `(new SessionManager())->callCustomCreator()` to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "callCustomCreator is declared on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_new_classname_arrow_method_cross_file() {
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
        "        new SessionManager()->callCustomCreator();\n",
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

    // Click on "callCustomCreator" on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `new SessionManager()->callCustomCreator()` cross-file"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/SessionManager.php"),
                "Should point to SessionManager.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 4,
                "callCustomCreator is on line 4 in SessionManager.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_parenthesized_new_arrow_method_cross_file() {
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
        "        (new SessionManager())->callCustomCreator();\n",
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

    // Click on "callCustomCreator" on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 33,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `(new SessionManager())->callCustomCreator()` cross-file"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            let path = location.uri.to_file_path().unwrap();
            assert!(
                path.ends_with("src/SessionManager.php"),
                "Should point to SessionManager.php, got: {:?}",
                path
            );
            assert_eq!(
                location.range.start.line, 4,
                "callCustomCreator is on line 4 in SessionManager.php"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_new_namespaced_classname_arrow_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Widget {\n",
        "    public function render(): void {}\n",
        "}\n",
        "\n",
        "class Page {\n",
        "    public function show(): void {\n",
        "        (new Widget())->render();\n",
        "        new Widget()->render();\n",
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

    // Click on "render" in `(new Widget())->render()` on line 9
    let params1 = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params1).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `(new Widget())->render()` to render declaration"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 4, "render is on line 4");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // Click on "render" in `new Widget()->render()` on line 10
    let params2 = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params2).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve `new Widget()->render()` to render declaration"
    );
    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 4, "render is on line 4");
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}
