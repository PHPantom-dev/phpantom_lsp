use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_completion_includes_properties() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///props.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public int $age;\n",
        "    function login() {}\n",
        "    function logout() {}\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    )
    .to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$this->` on line 7
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            // Should have 3 methods (login, logout, test) + 2 properties = 5 items
            let method_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .collect();
            let property_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .collect();

            assert_eq!(method_items.len(), 3, "Should have 3 methods");
            assert_eq!(property_items.len(), 2, "Should have 2 properties");

            let prop_labels: Vec<&str> = property_items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                prop_labels.contains(&"name"),
                "Should contain property 'name'"
            );
            assert!(
                prop_labels.contains(&"age"),
                "Should contain property 'age'"
            );

            // Check property insert_text is the property name (no $)
            let name_item = property_items.iter().find(|i| i.label == "name").unwrap();
            assert_eq!(
                name_item.insert_text.as_deref(),
                Some("name"),
                "Property insert_text should be 'name' without $"
            );

            // Check property detail includes type hint
            let name_detail = name_item.detail.as_deref().unwrap();
            assert!(
                name_detail.contains("string"),
                "Property detail '{}' should include type hint 'string'",
                name_detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_property_without_type_hint() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///untyped.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Bag {\n",
        "    public $stuff;\n",
        "    function get() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    )
    .to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$this->` on line 4
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 4,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let property_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .collect();

            assert_eq!(property_items.len(), 1);
            assert_eq!(property_items[0].label, "stuff");

            assert!(
                property_items[0].detail.is_none(),
                "Untyped property should have no detail, got: {:?}",
                property_items[0].detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_class_with_only_properties() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///onlyprops.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Data {\n",
        "    public string $name;\n",
        "    public int $value;\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    )
    .to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$this->` on line 5
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
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let property_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .collect();
            // Class has 2 properties + test method, but we check properties
            assert_eq!(
                property_items.len(),
                2,
                "Should return 2 property completions"
            );
            let labels: Vec<&str> = property_items.iter().map(|i| i.label.as_str()).collect();
            assert!(labels.contains(&"name"));
            assert!(labels.contains(&"value"));
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_constant_detail_with_type_hint() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///consttype.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Cfg {\n",
        "    const string LABEL = 'hello';\n",
        "    const COUNT = 5;\n",
        "    function f() {\n",
        "        self::\n",
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

    // Cursor after `self::` on line 5
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let constants: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .collect();
            assert_eq!(constants.len(), 2, "Should have 2 constants");

            let label_const = constants.iter().find(|c| c.label == "LABEL").unwrap();
            assert_eq!(
                label_const.detail.as_deref(),
                Some("'hello'"),
                "LABEL detail should show the value, got: {:?}",
                label_const.detail
            );

            let count_const = constants.iter().find(|c| c.label == "COUNT").unwrap();
            assert_eq!(
                count_const.detail.as_deref(),
                Some("5"),
                "COUNT detail should show the value, got: {:?}",
                count_const.detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_promoted_properties_appear_in_this() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///promoted.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ShoppingCartService {\n",
        "    private IShoppingCart $regular;\n",
        "\n",
        "    public function __construct(\n",
        "        private IShoppingCart $promoted,\n",
        "    ) {}\n",
        "\n",
        "    public function doWork(): void {\n",
        "        $this->\n",
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

    // Cursor right after `$this->` on line 9
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
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                names.contains(&"regular"),
                "Should contain regular property 'regular', got: {:?}",
                names
            );
            assert!(
                names.contains(&"promoted"),
                "Should contain promoted property 'promoted', got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Visibility filtering ───────────────────────────────────────────────────

/// Private properties should NOT appear when accessing a variable from
/// outside the class (e.g. top-level code).
#[tokio::test]
async fn test_completion_private_property_hidden_outside_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///vis_private_hidden.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Container {\n",
        "    private array $bindings = [];\n",
        "    public function bind(string $k, object $v): void {}\n",
        "    public function getStatus(): int { return 0; }\n",
        "}\n",
        "$c = new Container();\n",
        "$c->\n",
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
                    character: 4,
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
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                names.contains(&"bind"),
                "Should include public method 'bind', got: {:?}",
                names
            );
            assert!(
                names.contains(&"getStatus"),
                "Should include public method 'getStatus', got: {:?}",
                names
            );
            assert!(
                !names.contains(&"bindings"),
                "Should NOT include private property 'bindings', got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Private methods should NOT appear when accessing from outside the class.
#[tokio::test]
async fn test_completion_private_method_hidden_outside_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///vis_private_method_hidden.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    private function internalHelper(): void {}\n",
        "    protected function onSetup(): void {}\n",
        "    public function run(): void {}\n",
        "}\n",
        "$svc = new Service();\n",
        "$svc->\n",
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
                method_names.contains(&"run"),
                "Should include public method 'run', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"internalHelper"),
                "Should NOT include private method 'internalHelper', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"onSetup"),
                "Should NOT include protected method 'onSetup' from top-level, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_instance_access_includes_public_static_methods() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///vis_instance_static_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    public static function make(): self { return new self(); }\n",
        "    public function run(): void {}\n",
        "}\n",
        "$svc = new Service();\n",
        "$svc->\n",
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
                method_names.contains(&"run"),
                "Should include instance method 'run', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"make"),
                "Should include static method 'make' on instance access, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$this->` inside the same class should show private and protected members.
#[tokio::test]
async fn test_completion_private_and_protected_visible_inside_own_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///vis_private_visible.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Vault {\n",
        "    private string $secret = 'x';\n",
        "    protected int $level = 1;\n",
        "    public string $name = 'vault';\n",
        "    private function decrypt(): string { return ''; }\n",
        "    protected function validate(): bool { return true; }\n",
        "    public function open(): void {\n",
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

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                names.contains(&"secret"),
                "Should include private property 'secret' via $this->, got: {:?}",
                names
            );
            assert!(
                names.contains(&"level"),
                "Should include protected property 'level' via $this->, got: {:?}",
                names
            );
            assert!(
                names.contains(&"name"),
                "Should include public property 'name' via $this->, got: {:?}",
                names
            );
            assert!(
                names.contains(&"decrypt"),
                "Should include private method 'decrypt' via $this->, got: {:?}",
                names
            );
            assert!(
                names.contains(&"validate"),
                "Should include protected method 'validate' via $this->, got: {:?}",
                names
            );
            assert!(
                names.contains(&"open"),
                "Should include public method 'open' via $this->, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// A variable of the same class type inside the class should also show
/// private/protected members (PHP allows same-class access).
#[tokio::test]
async fn test_completion_private_visible_on_same_class_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///vis_same_class_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Node {\n",
        "    private int $id;\n",
        "    public string $label;\n",
        "    public function merge(Node $other): void {\n",
        "        $other->\n",
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
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                names.contains(&"id"),
                "Should include private 'id' on same-class variable, got: {:?}",
                names
            );
            assert!(
                names.contains(&"label"),
                "Should include public 'label', got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Protected members should be visible when accessing from a different
/// class (the caller might be a subclass).
#[tokio::test]
async fn test_completion_protected_visible_from_different_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///vis_protected_subclass.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    private function secret(): void {}\n",
        "    protected function hook(): void {}\n",
        "    public function run(): void {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    public function doWork(): void {\n",
        "        $b = new Base();\n",
        "        $b->\n",
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
                method_names.contains(&"run"),
                "Should include public 'run', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"hook"),
                "Should include protected 'hook' from inside another class, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"secret"),
                "Should NOT include private 'secret' from different class, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Private constants should be hidden from outside the class via `::`.
#[tokio::test]
async fn test_completion_private_constant_hidden_outside_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///vis_const_hidden.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    private const SECRET_KEY = 'abc';\n",
        "    protected const INTERNAL_VER = 2;\n",
        "    public const VERSION = '1.0';\n",
        "    public static function create(): void {}\n",
        "}\n",
        "Config::\n",
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
                    character: 8,
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
            let const_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                const_names.contains(&"VERSION"),
                "Should include public constant 'VERSION', got: {:?}",
                const_names
            );
            assert!(
                !const_names.contains(&"SECRET_KEY"),
                "Should NOT include private constant 'SECRET_KEY', got: {:?}",
                const_names
            );
            assert!(
                !const_names.contains(&"INTERNAL_VER"),
                "Should NOT include protected constant 'INTERNAL_VER' from top-level, got: {:?}",
                const_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `self::` inside the class should show all constants including private.
#[tokio::test]
async fn test_completion_private_constant_visible_via_self() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///vis_const_self.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    private const SECRET_KEY = 'abc';\n",
        "    protected const INTERNAL_VER = 2;\n",
        "    public const VERSION = '1.0';\n",
        "    public function check(): void {\n",
        "        self::\n",
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
            let const_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                const_names.contains(&"VERSION"),
                "Should include public constant 'VERSION', got: {:?}",
                const_names
            );
            assert!(
                const_names.contains(&"SECRET_KEY"),
                "Should include private constant 'SECRET_KEY' via self::, got: {:?}",
                const_names
            );
            assert!(
                const_names.contains(&"INTERNAL_VER"),
                "Should include protected constant 'INTERNAL_VER' via self::, got: {:?}",
                const_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file: private property of a PSR-4 class should be hidden from
/// top-level code.
#[tokio::test]
async fn test_completion_private_hidden_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Repo.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "class Repo {\n",
                "    private array $cache = [];\n",
                "    protected string $table = '';\n",
                "    public function find(int $id): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///vis_cross_file.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Repo;\n",
        "$repo = new Repo();\n",
        "$repo->\n",
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
                    character: 7,
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
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                names.contains(&"find"),
                "Should include public method 'find', got: {:?}",
                names
            );
            assert!(
                !names.contains(&"cache"),
                "Should NOT include private property 'cache', got: {:?}",
                names
            );
            assert!(
                !names.contains(&"table"),
                "Should NOT include protected property 'table' from top-level, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_promoted_property_type_resolves_for_chaining() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///promoted_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "    public function error(string $msg): void {}\n",
        "}\n",
        "class Service {\n",
        "    public function __construct(\n",
        "        private Logger $logger,\n",
        "    ) {}\n",
        "\n",
        "    public function run(): void {\n",
        "        $this->logger->\n",
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

    // Cursor right after `$this->logger->` on line 11
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 11,
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
        "Completion should resolve promoted property type for chaining"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                names.iter().any(|n| n.starts_with("info(")),
                "Should contain Logger method 'info', got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n.starts_with("error(")),
                "Should contain Logger method 'error', got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── @param Docblock Override on Promoted Properties ────────────────────────

/// When a promoted property has native type `array` but the constructor
/// docblock has `@param list<User> $users`, the promoted property should
/// get the more specific `list<User>` type, enabling property chain
/// completion on the element type.
#[tokio::test]
async fn test_completion_promoted_property_param_override_basic() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///promoted_param_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public int $id;\n",
        "    public string $email;\n",
        "}\n",
        "class UserService {\n",
        "    /**\n",
        "     * @param list<User> $users\n",
        "     */\n",
        "    public function __construct(\n",
        "        public array $users,\n",
        "    ) {}\n",
        "\n",
        "    public function demo() {\n",
        "        $this->users\n",
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

    // Verify that the promoted property exists — complete on `$this->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 14,
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
        "Completion should return results for $this->users"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("users")),
                "Should include promoted property 'users'. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

/// When a promoted property has `@param Collection<int, Order> $orders`
/// overriding native `object`, completion on `$this->orders->` should
/// show Collection members (once the type is resolved).
#[tokio::test]
async fn test_completion_promoted_property_param_override_generic_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///promoted_generic_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "}\n",
        "class Collection {\n",
        "    public function first(): mixed {}\n",
        "    public function count(): int {}\n",
        "}\n",
        "class OrderService {\n",
        "    /**\n",
        "     * @param Collection<int, Order> $orders\n",
        "     */\n",
        "    public function __construct(\n",
        "        public object $orders,\n",
        "    ) {}\n",
        "\n",
        "    public function demo() {\n",
        "        $this->orders->\n",
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
        "Completion should return results for @param overridden $this->orders->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("first")),
                "Should include 'first' from Collection via @param override. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("count")),
                "Should include 'count' from Collection via @param override. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

/// When native type is `int` (concrete scalar) and @param says `UserId`,
/// the native scalar should win — no override.
#[tokio::test]
async fn test_completion_promoted_property_param_scalar_not_overridden() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///promoted_scalar_no_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class UserId {\n",
        "    public function getValue(): int {}\n",
        "}\n",
        "class Service {\n",
        "    /**\n",
        "     * @param UserId $id\n",
        "     */\n",
        "    public function __construct(\n",
        "        public int $id,\n",
        "    ) {}\n",
        "\n",
        "    public function demo() {\n",
        "        $this->id->\n",
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
    // Scalar `int` should not be overridden — no class members to complete.
    assert!(
        result.is_none(),
        "Scalar promoted property should not resolve to class, got: {:?}",
        result
    );
}

/// Property chain on non-$this variable with promoted property @param override.
/// Exercises the combination of both features: $var->prop-> where prop type
/// comes from a @param docblock override.
#[tokio::test]
async fn test_completion_promoted_property_param_override_with_var_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///promoted_var_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "    public function error(string $msg): void {}\n",
        "}\n",
        "class App {\n",
        "    /**\n",
        "     * @param Logger $logger\n",
        "     */\n",
        "    public function __construct(\n",
        "        public object $logger,\n",
        "    ) {}\n",
        "}\n",
        "function demo() {\n",
        "    $app = new App(new Logger());\n",
        "    $app->logger->\n",
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
        "Completion should return results for $app->logger-> via @param override"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("info")),
                "Should include 'info' from Logger via @param override + property chain. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("error")),
                "Should include 'error' from Logger via @param override + property chain. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

/// Cross-file PSR-4: promoted property with @param override where the
/// docblock type is in another file.
#[tokio::test]
async fn test_completion_promoted_property_param_override_cross_file() {
    let composer = r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#;

    let cache_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Cache {\n",
        "    public function get(string $key): mixed {}\n",
        "    public function set(string $key, mixed $val): void {}\n",
        "}\n",
    );

    let service_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Service {\n",
        "    /**\n",
        "     * @param Cache $cache\n",
        "     */\n",
        "    public function __construct(\n",
        "        public object $cache,\n",
        "    ) {}\n",
        "\n",
        "    public function demo() {\n",
        "        $this->cache->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Cache.php", cache_php),
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
                line: 12,
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
        "Completion should return results for cross-file @param promoted $this->cache->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("get")),
                "Should include 'get' from cross-file Cache via @param override. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("set")),
                "Should include 'set' from cross-file Cache via @param override. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

/// Inline `@var` docblock on a promoted constructor property should
/// override the native type hint, just like `@param` on the constructor.
///
/// Reproduces the pattern used by Spatie's laravel-data:
/// ```php
/// public function __construct(
///     /** @var array<EventModel> */
///     public array|Optional $decodedEvents,
/// ) {}
/// ```
#[tokio::test]
async fn test_completion_promoted_property_inline_var_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inline_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class EventModel {\n",
        "    public string $timestamp;\n",
        "    public function getLabel(): string { return ''; }\n",
        "}\n",
        "class DataModel {\n",
        "    public function __construct(\n",
        "        /** @var array<EventModel> */\n",
        "        public array $events,\n",
        "    ) {}\n",
        "    public function test(): void {\n",
        "        foreach ($this->events as $event) {\n",
        "            $event->\n",
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

    // "$event->" at line 12, character 20
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
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
        "Completion should resolve inline @var type on promoted property"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getLabel")),
                "Should include 'getLabel' from EventModel via inline @var. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l == &"timestamp"),
                "Should include 'timestamp' property from EventModel via inline @var. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

/// Inline `@var` on a promoted property with a union type (e.g.
/// `array|Optional`) should still be overridden by the `@var` tag.
#[tokio::test]
async fn test_completion_promoted_property_inline_var_union_native_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inline_var_union.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Optional {}\n",
        "class UserModel {\n",
        "    public string $name;\n",
        "    public function greet(): string { return ''; }\n",
        "}\n",
        "class Container {\n",
        "    public function __construct(\n",
        "        /** @var array<UserModel> */\n",
        "        public array|Optional $users,\n",
        "    ) {}\n",
        "    public function demo(): void {\n",
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

    // "$user->" at line 13, character 20
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
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
        "Completion should resolve inline @var on union-typed promoted property"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("greet")),
                "Should include 'greet' from UserModel via inline @var on union type. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l == &"name"),
                "Should include 'name' property from UserModel via inline @var on union type. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── @property tag type resolution through parameter variable chain ─────────

/// When a method parameter is typed with a class that declares `@property`
/// tags in its docblock, chaining through the virtual property should
/// resolve to the declared type.
///
/// ```php
/// /** @property Carbon $created */
/// class Supplyvaluelog extends Model {}
///
/// function index(Supplyvaluelog $supplyValueLog) {
///     $supplyValueLog->created->format('Ymd');
///     //                        ^^^^^^ should complete with Carbon methods
/// }
/// ```
#[tokio::test]
async fn test_completion_phpdoc_property_type_resolves_for_chaining_on_parameter() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///phpdoc_prop_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Carbon {\n",
        "    public function format(string $fmt): string { return ''; }\n",
        "    public function diffForHumans(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @property Carbon $created\n",
        " */\n",
        "class Supplyvaluelog {\n",
        "}\n",
        "class Controller {\n",
        "    public function index(Supplyvaluelog $supplyValueLog): void {\n",
        "        $supplyValueLog->created->\n",
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

    // Cursor right after `$supplyValueLog->created->` on line 12
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
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
        "Completion should resolve @property type for chaining on parameter variable"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                names.iter().any(|n| n.starts_with("format(")),
                "Should contain Carbon method 'format' via @property chain on parameter. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n.starts_with("diffForHumans(")),
                "Should contain Carbon method 'diffForHumans' via @property chain on parameter. Got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a class extends a parent and declares `@property` tags, chaining
/// through the virtual property on a method parameter should resolve to the
/// declared type.  This reproduces the reported issue where
/// `$supplyValueLog->created->format('Ymd')` fails to resolve when the
/// model class extends a base Model class.
#[tokio::test]
async fn test_completion_phpdoc_property_type_resolves_with_parent_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///phpdoc_prop_parent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Carbon {\n",
        "    public function format(string $fmt): string { return ''; }\n",
        "    public function diffForHumans(): string { return ''; }\n",
        "}\n",
        "class Model {\n",
        "    public static function find(int $id): static { return new static(); }\n",
        "}\n",
        "/**\n",
        " * @property Carbon $created\n",
        " */\n",
        "final class Supplyvaluelog extends Model {\n",
        "}\n",
        "class Controller {\n",
        "    public function index(Supplyvaluelog $supplyValueLog): void {\n",
        "        $supplyValueLog->created->\n",
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

    // Cursor right after `$supplyValueLog->created->` on line 15
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 15,
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
        "Completion should resolve @property type for chaining when class extends parent"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                names.iter().any(|n| n.starts_with("format(")),
                "Should contain Carbon method 'format' via @property chain with parent class. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n.starts_with("diffForHumans(")),
                "Should contain Carbon method 'diffForHumans' via @property chain with parent class. Got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file variant: the class with `@property` tags lives in a separate
/// PSR-4 file and extends a Model base class.  The `@property Carbon $created`
/// type should still resolve when the parameter is typed with the cross-file class.
#[tokio::test]
async fn test_completion_phpdoc_property_type_cross_file_parameter_chain() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\Models\\": "src/Models/",
                "App\\Http\\Controllers\\": "src/Http/Controllers/",
                "Carbon\\": "src/Carbon/",
                "Illuminate\\Database\\Eloquent\\": "src/Illuminate/"
            }
        }
    }"#;

    let carbon_php = "\
<?php
namespace Carbon;
class Carbon {
    public function format(string $fmt): string { return ''; }
    public function diffForHumans(): string { return ''; }
    public function addDays(int $n): static { return $this; }
}
";

    let base_model_php = "\
<?php
namespace Illuminate\\Database\\Eloquent;
class Model {
    public static function find(int $id): static { return new static(); }
    public function save(): bool { return true; }
}
";

    let model_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
/**
 * @property \\Carbon\\Carbon $created
 * @property string $name
 */
final class Supplyvaluelog extends Model {
}
";

    let controller_php = "\
<?php
namespace App\\Http\\Controllers;
use App\\Models\\Supplyvaluelog;
class Controller {
    public function index(Supplyvaluelog $supplyValueLog): void {
        $supplyValueLog->created->
    }
}
";

    let files: Vec<(&str, &str)> = vec![
        ("src/Carbon/Carbon.php", carbon_php),
        ("src/Illuminate/Model.php", base_model_php),
        ("src/Models/Supplyvaluelog.php", model_php),
        ("src/Http/Controllers/Controller.php", controller_php),
    ];

    let (backend, _dir) = create_psr4_workspace(composer_json, &files);

    let uri = Url::from_file_path(_dir.path().join("src/Http/Controllers/Controller.php")).unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: controller_php.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$supplyValueLog->created->` on line 5
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
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
        "Completion should resolve cross-file @property type for chaining on parameter"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                names.iter().any(|n| n.starts_with("format(")),
                "Should contain Carbon method 'format' via cross-file @property chain. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n.starts_with("diffForHumans(")),
                "Should contain Carbon method 'diffForHumans' via cross-file @property chain. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n.starts_with("addDays(")),
                "Should contain Carbon method 'addDays' via cross-file @property chain. Got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file variant where `@property` uses a SHORT class name (`Carbon`)
/// that is imported via `use Carbon\Carbon` in the model file but NOT in the
/// consuming controller file.  The type resolution for the property chain
/// must use the declaring file's imports to resolve the short name.
#[tokio::test]
async fn test_completion_phpdoc_property_short_name_cross_file() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\Models\\": "src/Models/",
                "App\\Http\\Controllers\\": "src/Http/Controllers/",
                "Carbon\\": "src/Carbon/",
                "Illuminate\\Database\\Eloquent\\": "src/Illuminate/"
            }
        }
    }"#;

    let carbon_php = "\
<?php
namespace Carbon;
class Carbon {
    public function format(string $fmt): string { return ''; }
    public function diffForHumans(): string { return ''; }
    public function addDays(int $n): static { return $this; }
}
";

    let base_model_php = "\
<?php
namespace Illuminate\\Database\\Eloquent;
class Model {
    public static function find(int $id): static { return new static(); }
    public function save(): bool { return true; }
}
";

    // The @property uses short name "Carbon" — imported via `use Carbon\Carbon`.
    let model_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Carbon\\Carbon;
/**
 * @property Carbon $created
 * @property string $name
 */
final class Supplyvaluelog extends Model {
}
";

    // The controller does NOT import Carbon — it only imports Supplyvaluelog.
    let controller_php = "\
<?php
namespace App\\Http\\Controllers;
use App\\Models\\Supplyvaluelog;
class Controller {
    public function index(Supplyvaluelog $supplyValueLog): void {
        $supplyValueLog->created->
    }
}
";

    let files: Vec<(&str, &str)> = vec![
        ("src/Carbon/Carbon.php", carbon_php),
        ("src/Illuminate/Model.php", base_model_php),
        ("src/Models/Supplyvaluelog.php", model_php),
        ("src/Http/Controllers/Controller.php", controller_php),
    ];

    let (backend, _dir) = create_psr4_workspace(composer_json, &files);

    let uri = Url::from_file_path(_dir.path().join("src/Http/Controllers/Controller.php")).unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: controller_php.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `$supplyValueLog->created->` on line 5
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
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
        "Completion should resolve cross-file @property type even when short name is used"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                names.iter().any(|n| n.starts_with("format(")),
                "Should contain Carbon method 'format' via short-name @property chain. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n.starts_with("diffForHumans(")),
                "Should contain Carbon method 'diffForHumans' via short-name @property chain. Got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n.starts_with("addDays(")),
                "Should contain Carbon method 'addDays' via short-name @property chain. Got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}
