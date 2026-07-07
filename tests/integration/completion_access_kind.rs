use crate::common::create_test_backend;
use phpantom_lsp::{AccessKind, extract_completion_target};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Access Kind Detection ──────────────────────────────────────────────────

#[tokio::test]
async fn test_detect_access_kind_arrow() {
    let target = extract_completion_target(
        "$this->",
        Position {
            line: 0,
            character: 7,
        },
    )
    .expect("should detect arrow");
    assert_eq!(target.access_kind, AccessKind::Arrow);
    assert_eq!(target.subject, "$this");
}

#[tokio::test]
async fn test_detect_access_kind_arrow_with_partial_identifier() {
    let target = extract_completion_target(
        "$this->get",
        Position {
            line: 0,
            character: 10,
        },
    )
    .expect("should detect arrow");
    assert_eq!(target.access_kind, AccessKind::Arrow);
    assert_eq!(target.subject, "$this");
}

#[tokio::test]
async fn test_detect_access_kind_double_colon() {
    let target = extract_completion_target(
        "self::",
        Position {
            line: 0,
            character: 6,
        },
    )
    .expect("should detect double colon");
    assert_eq!(target.access_kind, AccessKind::DoubleColon);
    assert_eq!(target.subject, "self");
}

#[tokio::test]
async fn test_detect_access_kind_double_colon_with_partial_identifier() {
    let target = extract_completion_target(
        "Foo::cr",
        Position {
            line: 0,
            character: 7,
        },
    )
    .expect("should detect double colon");
    assert_eq!(target.access_kind, AccessKind::DoubleColon);
    assert_eq!(target.subject, "Foo");
}

#[tokio::test]
async fn test_detect_access_kind_other() {
    let result = extract_completion_target(
        "    $x = 1;",
        Position {
            line: 0,
            character: 4,
        },
    );
    assert!(result.is_none(), "no access operator expected");
}

#[tokio::test]
async fn test_detect_access_kind_multiline() {
    let content = "<?php\n$obj->meth";
    let target = extract_completion_target(
        content,
        Position {
            line: 1,
            character: 10,
        },
    )
    .expect("should detect arrow");
    assert_eq!(target.access_kind, AccessKind::Arrow);
    assert_eq!(target.subject, "$obj");
}

// ─── Arrow vs Double-Colon Filtering ────────────────────────────────────────

#[tokio::test]
async fn test_completion_arrow_shows_only_non_static() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///arrow.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Svc {\n",
        "    public static function create(): self {}\n",
        "    public function run(): void {}\n",
        "    public static string $instance = '';\n",
        "    public int $count = 0;\n",
        "    const MAX = 10;\n",
        "    function helper() {\n",
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

    // Cursor right after `$this->` on line 8
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
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let property_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.label.as_str())
                .collect();
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.label.as_str())
                .collect();

            // Should include non-static method `run` and `helper`
            assert!(
                method_names.contains(&"run"),
                "Arrow should include non-static method 'run'"
            );
            assert!(
                method_names.contains(&"helper"),
                "Arrow should include non-static method 'helper'"
            );
            // PHP allows calling static methods through an instance,
            // so `->` access should include static methods too.
            assert!(
                method_names.contains(&"create"),
                "Arrow should include static method 'create' (PHP allows static calls via instance)"
            );

            // Should include non-static property `count`
            assert!(
                property_names.contains(&"count"),
                "Arrow should include non-static property 'count'"
            );
            // Should NOT include static property `instance`
            assert!(
                !property_names.contains(&"instance"),
                "Arrow should exclude static property 'instance'"
            );

            // Should NOT include constants
            assert!(
                constant_names.is_empty(),
                "Arrow should not include constants, got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_self_double_colon_shows_all_members() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///dcolon.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Svc {\n",
        "    public static function create(): self {}\n",
        "    public function run(): void {}\n",
        "    public static string $instance = '';\n",
        "    public int $count = 0;\n",
        "    const MAX = 10;\n",
        "    function helper() {\n",
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

    // Cursor right after `self::` on line 8
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
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let property_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.label.as_str())
                .collect();
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.label.as_str())
                .collect();

            // Should include static method `create`
            assert!(
                method_names.contains(&"create"),
                "self:: should include static method 'create'"
            );
            // self:: shows both static and non-static methods (PHP allows
            // calling instance methods via `self::` from within the class).
            assert!(
                method_names.contains(&"run"),
                "self:: should include non-static method 'run'"
            );
            assert!(
                method_names.contains(&"helper"),
                "self:: should include non-static method 'helper'"
            );

            // Should include static property `$instance` (with $ prefix for :: access)
            assert!(
                property_names.contains(&"$instance"),
                "self:: should include static property '$instance'"
            );
            // Non-static properties are still excluded — `self::$count`
            // is not valid PHP for instance properties.
            assert!(
                !property_names.contains(&"count") && !property_names.contains(&"$count"),
                "self:: should exclude non-static property 'count'"
            );

            // Should include constant `MAX`
            assert!(
                constant_names.contains(&"MAX"),
                "self:: should include constant 'MAX'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_arrow_with_partial_typed_identifier() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///partial.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Obj {\n",
        "    public static function staticMethod(): void {}\n",
        "    public function instanceMethod(): void {}\n",
        "    function test() {\n",
        "        $this->inst\n",
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

    // Cursor after `$this->inst` on line 5 — partial identifier typed after ->
    let completion_params = CompletionParams {
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
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Should include non-static method
            assert!(
                method_names.contains(&"instanceMethod"),
                "Should include instanceMethod"
            );
            assert!(method_names.contains(&"test"), "Should include test");
            // PHP allows calling static methods through an instance,
            // so `->` access should include static methods too.
            assert!(
                method_names.contains(&"staticMethod"),
                "Should include staticMethod when using -> (PHP allows static calls via instance)"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── __construct visibility via :: access ───────────────────────────────────

#[tokio::test]
async fn test_construct_shown_via_self_inside_same_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///construct_self.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function __construct() {}\n",
        "    public function test() {\n",
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

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 4,
                character: 14,
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
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"__construct"),
                "self:: inside same class should show __construct, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_construct_shown_via_classname_inside_same_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///construct_classname.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function __construct() {}\n",
        "    public function test() {\n",
        "        A::\n",
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
                character: 11,
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
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"__construct"),
                "A:: inside class A should show __construct, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_construct_shown_via_static_inside_same_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///construct_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function __construct() {}\n",
        "    public function test() {\n",
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
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"__construct"),
                "static:: inside same class should show __construct, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_construct_shown_via_parent_in_subclass() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///construct_parent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function __construct() {}\n",
        "}\n",
        "class B extends A {\n",
        "    public function __construct() {\n",
        "        parent::\n",
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
                character: 16,
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
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"__construct"),
                "parent:: in subclass should show __construct, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_construct_shown_via_self_in_subclass() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///construct_self_sub.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function __construct() {}\n",
        "}\n",
        "class B extends A {\n",
        "    public function __construct() {\n",
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
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"__construct"),
                "self:: in subclass should show __construct, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_construct_shown_via_parent_classname_in_subclass() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///construct_parent_name.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function __construct() {}\n",
        "}\n",
        "class B extends A {\n",
        "    public function __construct() {\n",
        "        A::\n",
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
                character: 11,
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
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"__construct"),
                "A:: inside subclass B should show __construct, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_construct_hidden_via_classname_outside_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///construct_outside.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function __construct() {}\n",
        "}\n",
        "A::\n",
    );

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
                character: 3,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();

    if let Some(CompletionResponse::Array(items)) = result {
        let method_names: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|i| i.filter_text.as_deref().unwrap())
            .collect();
        assert!(
            !method_names.contains(&"__construct"),
            "A:: outside any class should NOT show __construct, got: {:?}",
            method_names
        );
    }
    // No results at all is also acceptable — no magic methods to show
}

#[tokio::test]
async fn test_construct_hidden_via_classname_in_unrelated_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///construct_unrelated.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class A {\n",
        "    public function __construct() {}\n",
        "    public static function make(): self {}\n",
        "}\n",
        "class C {\n",
        "    public function test() {\n",
        "        A::\n",
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
        "Completion should return results (static method 'make')"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                method_names.contains(&"make"),
                "A:: in unrelated class C should still show static method 'make', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"__construct"),
                "A:: in unrelated class C should NOT show __construct, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── ::class keyword completion ─────────────────────────────────────────────

#[tokio::test]
async fn test_double_colon_shows_class_keyword() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///class_keyword.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    const BAR = 1;\n",
        "    public static function baz(): void {}\n",
        "    function test() {\n",
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
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let keyword_labels: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
                .map(|i| i.label.as_str())
                .collect();
            assert!(
                keyword_labels.contains(&"class"),
                "self:: should offer ::class keyword, got: {:?}",
                keyword_labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── self:: and static:: show the same symbols as parent:: ──────────────────

/// `self::` and `static::` should show both static **and** non-static methods,
/// just like `parent::` does, because PHP allows calling instance methods via
/// `self::method()` and `static::method()` from within the class hierarchy.
#[tokio::test]
async fn test_self_and_static_show_non_static_methods_like_parent() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///self_static_parity.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function breathe(): void {}\n",
        "    public static function kingdom(): string { return 'Animalia'; }\n",
        "    public const LEGS = 4;\n",
        "    public static string $species = '';\n",
        "    public int $age = 0;\n",
        "}\n",
        "class Dog extends Animal {\n",
        "    public function bark(): void {}\n",
        "    function testSelf() {\n",
        "        self::\n",
        "    }\n",
        "    function testStatic() {\n",
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

    // ── self:: (line 11, character 14) ──
    let self_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let self_result = backend.completion(self_params).await.unwrap();
    assert!(self_result.is_some(), "self:: should return completions");

    let self_items = match self_result.unwrap() {
        CompletionResponse::Array(items) => items,
        _ => panic!("Expected CompletionResponse::Array"),
    };

    let self_method_names: Vec<&str> = self_items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap())
        .collect();
    let self_property_names: Vec<&str> = self_items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
        .map(|i| i.label.as_str())
        .collect();
    let self_constant_names: Vec<&str> = self_items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
        .map(|i| i.label.as_str())
        .collect();

    // self:: should show both static and non-static methods
    assert!(
        self_method_names.contains(&"breathe"),
        "self:: should include inherited non-static 'breathe', got: {:?}",
        self_method_names
    );
    assert!(
        self_method_names.contains(&"kingdom"),
        "self:: should include inherited static 'kingdom', got: {:?}",
        self_method_names
    );
    assert!(
        self_method_names.contains(&"bark"),
        "self:: should include own non-static 'bark', got: {:?}",
        self_method_names
    );
    // Static properties shown, instance properties excluded
    assert!(
        self_property_names.contains(&"$species"),
        "self:: should include static property '$species', got: {:?}",
        self_property_names
    );
    assert!(
        !self_property_names.contains(&"age") && !self_property_names.contains(&"$age"),
        "self:: should exclude non-static property 'age', got: {:?}",
        self_property_names
    );
    // Constants shown
    assert!(
        self_constant_names.contains(&"LEGS"),
        "self:: should include constant 'LEGS', got: {:?}",
        self_constant_names
    );

    // ── static:: (line 14, character 16) ──
    let static_params = CompletionParams {
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

    let static_result = backend.completion(static_params).await.unwrap();
    assert!(
        static_result.is_some(),
        "static:: should return completions"
    );

    let static_items = match static_result.unwrap() {
        CompletionResponse::Array(items) => items,
        _ => panic!("Expected CompletionResponse::Array"),
    };

    let static_method_names: Vec<&str> = static_items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap())
        .collect();
    let static_constant_names: Vec<&str> = static_items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
        .map(|i| i.label.as_str())
        .collect();

    // static:: should also show both static and non-static methods
    assert!(
        static_method_names.contains(&"breathe"),
        "static:: should include inherited non-static 'breathe', got: {:?}",
        static_method_names
    );
    assert!(
        static_method_names.contains(&"kingdom"),
        "static:: should include inherited static 'kingdom', got: {:?}",
        static_method_names
    );
    assert!(
        static_method_names.contains(&"bark"),
        "static:: should include own non-static 'bark', got: {:?}",
        static_method_names
    );
    assert!(
        static_constant_names.contains(&"LEGS"),
        "static:: should include constant 'LEGS', got: {:?}",
        static_constant_names
    );
}

// ─── Static property subject resolves to property type ──────────────────────

/// When `self::$prop->method()` is used, the subject `self::$prop` should
/// resolve to the *type* of the static property, not to the class that
/// declares it.
#[tokio::test]
async fn test_static_property_access_resolves_to_property_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b17_static_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Connection {\n",
        "    public function setConfig(string $key): void {}\n",
        "    public function ping(): bool { return true; }\n",
        "}\n",
        "class ConnectionManager {\n",
        "    private static Connection $instance;\n",
        "\n",
        "    public static function test(): void {\n",
        "        self::$instance->\n",
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

    // Cursor right after `self::$instance->` on line 9
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 26,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    let method_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        method_names.iter().any(|n| n.starts_with("setConfig")),
        "self::$instance-> should resolve to Connection and show 'setConfig', got: {:?}",
        method_names
    );
    assert!(
        method_names.iter().any(|n| n.starts_with("ping")),
        "self::$instance-> should resolve to Connection and show 'ping', got: {:?}",
        method_names
    );
}

/// Static property access via an explicit class name should also resolve
/// to the property type.
#[tokio::test]
async fn test_classname_static_property_access_resolves_to_property_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b17_classname_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "}\n",
        "class Registry {\n",
        "    public static Logger $logger;\n",
        "\n",
        "    public static function test(): void {\n",
        "        Registry::$logger->\n",
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

    // Cursor right after `Registry::$logger->` on line 8
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 28,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    let method_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        method_names.iter().any(|n| n.starts_with("info")),
        "Registry::$logger-> should resolve to Logger and show 'info', got: {:?}",
        method_names
    );
}

/// `parent::$prop->` should resolve through the parent class's static
/// property type.
#[tokio::test]
async fn test_parent_static_property_access_resolves_to_property_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b17_parent_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Formatter {\n",
        "    public function format(string $s): string { return $s; }\n",
        "}\n",
        "class Base {\n",
        "    protected static Formatter $fmt;\n",
        "}\n",
        "class Child extends Base {\n",
        "    public function test(): void {\n",
        "        parent::$fmt->\n",
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

    // Cursor right after `parent::$fmt->` on line 9
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    let method_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        method_names.iter().any(|n| n.starts_with("format")),
        "parent::$fmt-> should resolve to Formatter and show 'format', got: {:?}",
        method_names
    );
}
