use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── parent:: completion tests ──────────────────────────────────────────────

#[tokio::test]
async fn test_completion_parent_double_colon_shows_static_and_instance() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///parent_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function breathe(): void {}\n",
        "    public static function kingdom(): string { return 'Animalia'; }\n",
        "}\n",
        "class Dog extends Animal {\n",
        "    function test() {\n",
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
        "parent:: should return completion results"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // parent:: shows BOTH static and non-static methods
            assert!(
                method_names.contains(&"breathe"),
                "parent:: should include non-static 'breathe', got {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"kingdom"),
                "parent:: should include static 'kingdom', got {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_parent_double_colon_excludes_private() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///parent_vis.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function pubMethod(): void {}\n",
        "    protected function protMethod(): void {}\n",
        "    private function privMethod(): void {}\n",
        "    public string $pubProp;\n",
        "    protected string $protProp;\n",
        "    private string $privProp;\n",
        "    public static string $pubStaticProp = '';\n",
        "    protected static string $protStaticProp = '';\n",
        "    private static string $privStaticProp = '';\n",
        "    public const PUB_CONST = 1;\n",
        "    protected const PROT_CONST = 2;\n",
        "    private const PRIV_CONST = 3;\n",
        "}\n",
        "class Child extends Base {\n",
        "    function test() {\n",
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
                line: 17,
                character: 16,
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
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.label.as_str())
                .collect();
            let const_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.label.as_str())
                .collect();

            // Methods: public and protected included, private excluded
            assert!(
                method_names.contains(&"pubMethod"),
                "Should include public method"
            );
            assert!(
                method_names.contains(&"protMethod"),
                "Should include protected method"
            );
            assert!(
                !method_names.contains(&"privMethod"),
                "Should NOT include private method"
            );

            // Properties: only static properties shown (parent:: uses :: syntax),
            // public and protected included, private excluded
            assert!(
                prop_names.contains(&"$pubStaticProp"),
                "Should include public static property"
            );
            assert!(
                prop_names.contains(&"$protStaticProp"),
                "Should include protected static property"
            );
            assert!(
                !prop_names.contains(&"$privStaticProp"),
                "Should NOT include private static property"
            );
            // Non-static properties should not appear via ::
            assert!(
                !prop_names.contains(&"pubProp"),
                "Should NOT include non-static property via ::"
            );
            assert!(
                !prop_names.contains(&"$pubProp"),
                "Should NOT include non-static property via ::"
            );

            // Constants: public and protected included, private excluded
            assert!(
                const_names.contains(&"PUB_CONST"),
                "Should include public constant"
            );
            assert!(
                const_names.contains(&"PROT_CONST"),
                "Should include protected constant"
            );
            assert!(
                !const_names.contains(&"PRIV_CONST"),
                "Should NOT include private constant"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_parent_double_colon_includes_constants() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///parent_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    const VERSION = '1.0';\n",
        "    const APP_NAME = 'MyApp';\n",
        "}\n",
        "class AppConfig extends Config {\n",
        "    function test() {\n",
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
                line: 7,
                character: 16,
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
            let const_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.label.as_str())
                .collect();

            assert!(
                const_names.contains(&"VERSION"),
                "Should include constant 'VERSION'"
            );
            assert!(
                const_names.contains(&"APP_NAME"),
                "Should include constant 'APP_NAME'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_parent_double_colon_cross_file_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/BaseService.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "class BaseService {\n",
                "    public function init(): void {}\n",
                "    public static function create(): self { return new self(); }\n",
                "    protected function configure(): void {}\n",
                "    private function internalSetup(): void {}\n",
                "    const SERVICE_VERSION = '2.0';\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\BaseService;\n",
        "class MyService extends BaseService {\n",
        "    function test() {\n",
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
        "parent:: should resolve cross-file via PSR-4"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let const_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.label.as_str())
                .collect();

            // Both static and instance methods
            assert!(
                method_names.contains(&"init"),
                "Should include non-static 'init'"
            );
            assert!(
                method_names.contains(&"create"),
                "Should include static 'create'"
            );
            assert!(
                method_names.contains(&"configure"),
                "Should include protected 'configure'"
            );
            assert!(
                !method_names.contains(&"internalSetup"),
                "Should NOT include private 'internalSetup'"
            );

            // Constants
            assert!(
                const_names.contains(&"SERVICE_VERSION"),
                "Should include constant"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_parent_double_colon_with_grandparent() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///parent_grandparent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Grandparent {\n",
        "    public function ancestorMethod(): void {}\n",
        "    protected function ancestorProtected(): void {}\n",
        "    private function ancestorPrivate(): void {}\n",
        "}\n",
        "class ParentClass extends Grandparent {\n",
        "    public function parentMethod(): void {}\n",
        "}\n",
        "class ChildClass extends ParentClass {\n",
        "    function test() {\n",
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
                line: 11,
                character: 16,
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

            // ParentClass's own method
            assert!(
                method_names.contains(&"parentMethod"),
                "Should include parent's own 'parentMethod'"
            );

            // Grandparent's public and protected methods (inherited into ParentClass)
            assert!(
                method_names.contains(&"ancestorMethod"),
                "Should include grandparent's 'ancestorMethod'"
            );
            assert!(
                method_names.contains(&"ancestorProtected"),
                "Should include grandparent's protected 'ancestorProtected'"
            );

            // Grandparent's private should NOT appear
            assert!(
                !method_names.contains(&"ancestorPrivate"),
                "Should NOT include grandparent's private 'ancestorPrivate'"
            );

            // Child's own methods should NOT appear (parent:: is the parent, not self)
            assert!(
                !method_names.contains(&"test"),
                "Should NOT include child's own 'test'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_parent_double_colon_no_parent_falls_back() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///parent_none.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Standalone {\n",
        "    function test() {\n",
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
                line: 3,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // Should return None since there's no parent class
    assert!(
        result.is_none(),
        "parent:: with no parent class should return None"
    );
}

#[tokio::test]
async fn test_completion_parent_double_colon_construct_and_magic_included() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///parent_magic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function __construct() {}\n",
        "    public function __toString(): string { return ''; }\n",
        "    public function realMethod(): void {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    function test() {\n",
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
                line: 8,
                character: 16,
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

            assert!(
                method_names.contains(&"__construct"),
                "__construct should be available via parent:: (commonly used to call parent constructor), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"__toString"),
                "Implemented magic methods like __toString are offered via parent::, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"realMethod"),
                "Non-magic method should appear via parent::"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `static::` inside a `final` class should produce no suggestions,
/// nudging the developer to use `self::` instead.
#[tokio::test]
async fn test_completion_static_double_colon_suppressed_on_final_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///static_final.php").unwrap();
    let text = concat!(
        "<?php\n",
        "final class Singleton {\n",
        "    private static ?self $instance = null;\n",
        "    public static function getInstance(): static {\n",
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
    // Should return None since the class is final — static:: is suppressed
    assert!(
        result.is_none(),
        "static:: in a final class should return None"
    );
}

/// `static::` inside a non-final class should work as normal.
#[tokio::test]
async fn test_completion_static_double_colon_works_on_non_final_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///static_non_final.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public static function create(): static {\n",
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
                line: 3,
                character: 16,
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

            assert!(
                method_names.contains(&"create"),
                "static:: on a non-final class should show methods, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `static::` inside an enum should be suppressed (enums are implicitly final).
#[tokio::test]
async fn test_completion_static_double_colon_suppressed_on_enum() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///static_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Color {\n",
        "    case Red;\n",
        "    case Blue;\n",
        "    public function label(): string {\n",
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
        result.is_none(),
        "static:: in an enum should return None (enums are final)"
    );
}

/// `self::` should still work on final classes (only `static::` is suppressed).
#[tokio::test]
async fn test_completion_self_double_colon_works_on_final_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///self_final.php").unwrap();
    let text = concat!(
        "<?php\n",
        "final class Config {\n",
        "    public const VERSION = '1.0';\n",
        "    public static function getVersion(): string {\n",
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let has_version = items.iter().any(|i| {
                i.kind == Some(CompletionItemKind::CONSTANT)
                    && i.filter_text.as_deref() == Some("VERSION")
            });
            let has_get_version = items.iter().any(|i| {
                i.kind == Some(CompletionItemKind::METHOD)
                    && i.filter_text.as_deref() == Some("getVersion")
            });
            assert!(
                has_version,
                "self:: on a final class should still show constants, got: {:?}",
                items.iter().map(|i| &i.label).collect::<Vec<_>>()
            );
            assert!(
                has_get_version,
                "self:: on a final class should still show static methods, got: {:?}",
                items.iter().map(|i| &i.label).collect::<Vec<_>>()
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `self::` should include `__construct` along with other implemented magic methods.
#[tokio::test]
async fn test_completion_self_double_colon_includes_construct() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///self_construct.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class MyService {\n",
        "    public function __construct(private string $name) {}\n",
        "    public function __toString(): string { return ''; }\n",
        "    public static function create(): static {\n",
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"__construct"),
                "self:: should include __construct, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"__toString"),
                "self:: offers implemented magic methods like __toString"
            );
            assert!(
                method_names.contains(&"create"),
                "self:: should include static methods"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `static::` should include `__construct` along with other implemented magic methods.
#[tokio::test]
async fn test_completion_static_double_colon_includes_construct() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///static_construct.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function __construct(private string $name) {}\n",
        "    public function __clone(): void {}\n",
        "    public static function make(): static {\n",
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
                line: 5,
                character: 16,
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

            assert!(
                method_names.contains(&"__construct"),
                "static:: should include __construct, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"__clone"),
                "static:: offers implemented magic methods like __clone"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Using the class name instead of `self::` (sloppy code) should also
/// show `__construct` in completion.
#[tokio::test]
async fn test_completion_classname_double_colon_includes_construct() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///classname_construct.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function __construct(private string $label) {}\n",
        "    public function __debugInfo(): array { return []; }\n",
        "    public static function create(): static {\n",
        "        Widget::\n",
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
                character: 16,
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

            assert!(
                method_names.contains(&"__construct"),
                "ClassName:: (sloppy self) should include __construct, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"__debugInfo"),
                "ClassName:: should still exclude other magic methods like __debugInfo"
            );
            assert!(
                method_names.contains(&"create"),
                "ClassName:: should include static methods"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Using another class name via `::` from outside should also show
/// `__construct` — PHP allows this for calling parent constructors
/// by explicit class name, e.g. `BaseClass::__construct(...)`.
#[tokio::test]
async fn test_completion_other_classname_double_colon_includes_construct() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///other_class_construct.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function __construct(private string $name) {}\n",
        "    public function __sleep(): array { return []; }\n",
        "    public static function create(): static { return new static(''); }\n",
        "}\n",
        "class Child extends Base {\n",
        "    public function __construct(string $name, private int $age) {\n",
        "        Base::\n",
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"__construct"),
                "Base:: from child class should include __construct, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"__sleep"),
                "Base:: should still exclude other magic methods like __sleep"
            );
            assert!(
                method_names.contains(&"create"),
                "Base:: should include static methods"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `__construct` should NOT appear via `->` (arrow) access — only via `::`.
#[tokio::test]
async fn test_completion_arrow_access_still_excludes_construct() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///arrow_no_construct.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Thing {\n",
        "    public function __construct() {}\n",
        "    public function doWork(): void {}\n",
        "}\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        $t = new Thing();\n",
        "        $t->\n",
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                !method_names.contains(&"__construct"),
                "__construct should NOT appear via -> access, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"doWork"),
                "Regular methods should still appear via -> access"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}
