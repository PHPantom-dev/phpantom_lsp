use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Basic trait usage (same file) ──────────────────────────────────────────

#[tokio::test]
async fn test_completion_trait_methods_available_on_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Greetable {\n",
        "    public function greet(): string { return 'hi'; }\n",
        "    protected function farewell(): string { return 'bye'; }\n",
        "}\n",
        "class Person {\n",
        "    use Greetable;\n",
        "    public function name(): string { return 'Alice'; }\n",
        "    function test() {\n",
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

    assert!(result.is_some(), "Completion should return results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"greet"),
                "Should include trait method 'greet', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"farewell"),
                "Should include trait protected method 'farewell', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"name"),
                "Should include own method 'name', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait properties ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_completion_trait_properties_available_on_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_props.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait HasTimestamps {\n",
        "    public string $created_at;\n",
        "    protected string $updated_at;\n",
        "    private string $internal_ts;\n",
        "}\n",
        "class Post {\n",
        "    use HasTimestamps;\n",
        "    public string $title;\n",
        "    function test() {\n",
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

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                prop_names.contains(&"title"),
                "Should include own property 'title', got: {:?}",
                prop_names
            );
            assert!(
                prop_names.contains(&"created_at"),
                "Should include trait property 'created_at', got: {:?}",
                prop_names
            );
            assert!(
                prop_names.contains(&"updated_at"),
                "Should include trait protected property 'updated_at', got: {:?}",
                prop_names
            );
            // Private trait members ARE included (trait is copy-paste semantics)
            assert!(
                prop_names.contains(&"internal_ts"),
                "Should include trait private property 'internal_ts', got: {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait constants ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_completion_trait_constants_available_via_double_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait HasVersion {\n",
        "    public const VERSION = '1.0';\n",
        "}\n",
        "class App {\n",
        "    use HasVersion;\n",
        "    public const NAME = 'MyApp';\n",
        "    function test() {\n",
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
                    line: 8,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let const_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                const_names.contains(&"NAME"),
                "Should include own constant 'NAME', got: {:?}",
                const_names
            );
            assert!(
                const_names.contains(&"VERSION"),
                "Should include trait constant 'VERSION', got: {:?}",
                const_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Multiple traits ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_completion_multiple_traits_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///multi_traits.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Loggable {\n",
        "    public function log(): void {}\n",
        "}\n",
        "trait Cacheable {\n",
        "    public function cache(): void {}\n",
        "}\n",
        "class Service {\n",
        "    use Loggable, Cacheable;\n",
        "    public function run(): void {}\n",
        "    function test() {\n",
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
                    line: 11,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"log"),
                "Should include Loggable::log, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"cache"),
                "Should include Cacheable::cache, got: {:?}",
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

// ─── Trait composed from other traits ───────────────────────────────────────

#[tokio::test]
async fn test_completion_nested_trait_composition() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nested_traits.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Hello {\n",
        "    public function sayHello(): string { return 'Hello'; }\n",
        "}\n",
        "trait World {\n",
        "    public function sayWorld(): string { return 'World'; }\n",
        "}\n",
        "trait HelloWorld {\n",
        "    use Hello, World;\n",
        "}\n",
        "class Greeter {\n",
        "    use HelloWorld;\n",
        "    function test() {\n",
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
                    line: 13,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"sayHello"),
                "Should include Hello::sayHello via nested trait, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"sayWorld"),
                "Should include World::sayWorld via nested trait, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Class method overrides trait method ────────────────────────────────────

#[tokio::test]
async fn test_completion_class_method_overrides_trait_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Renderable {\n",
        "    public function render(): string { return 'trait'; }\n",
        "    public function format(): string { return 'format'; }\n",
        "}\n",
        "class View {\n",
        "    use Renderable;\n",
        "    public function render(): string { return 'class'; }\n",
        "    function test() {\n",
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

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .collect();

            let render_items: Vec<&&CompletionItem> = method_items
                .iter()
                .filter(|i| i.filter_text.as_deref() == Some("render"))
                .collect();

            // There should be exactly one 'render' — the class's version wins
            assert_eq!(
                render_items.len(),
                1,
                "Should have exactly one 'render' method (class override), got: {}",
                render_items.len()
            );

            // Trait-only method should still be present
            let format_items: Vec<&&CompletionItem> = method_items
                .iter()
                .filter(|i| i.filter_text.as_deref() == Some("format"))
                .collect();
            assert_eq!(
                format_items.len(),
                1,
                "Should include trait-only method 'format'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait + parent class inheritance (PHP precedence) ───────────────────────

#[tokio::test]
async fn test_completion_trait_overrides_parent_class_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_vs_parent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function hello(): string { return 'base'; }\n",
        "    public function baseOnly(): void {}\n",
        "}\n",
        "trait SayWorld {\n",
        "    public function hello(): string { return 'trait'; }\n",
        "    public function traitOnly(): void {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    use SayWorld;\n",
        "    function test() {\n",
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
                    line: 12,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Trait method should be present (overrides parent)
            assert!(
                method_names.contains(&"hello"),
                "Should include 'hello' (from trait), got: {:?}",
                method_names
            );
            // Trait-only method
            assert!(
                method_names.contains(&"traitOnly"),
                "Should include trait-only method, got: {:?}",
                method_names
            );
            // Parent-only method
            assert!(
                method_names.contains(&"baseOnly"),
                "Should include parent-only method, got: {:?}",
                method_names
            );
            // Own method
            assert!(
                method_names.contains(&"test"),
                "Should include own method 'test', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Cross-file trait usage with PSR-4 ──────────────────────────────────────

#[tokio::test]
async fn test_completion_trait_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let trait_php = concat!(
        "<?php\n",
        "namespace App\\Traits;\n",
        "trait Auditable {\n",
        "    public function getAuditLog(): array { return []; }\n",
        "    public function setAuditor(string $name): void {}\n",
        "}\n",
    );

    let class_php = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "use App\\Traits\\Auditable;\n",
        "class User {\n",
        "    use Auditable;\n",
        "    public string $name;\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("src/Traits/Auditable.php", trait_php),
            ("src/Models/User.php", class_php),
        ],
    );

    let uri = Url::parse("file:///test_cross_trait.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: class_php.to_string(),
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

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getAuditLog"),
                "Should include cross-file trait method 'getAuditLog', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"setAuditor"),
                "Should include cross-file trait method 'setAuditor', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Cross-file nested trait composition with PSR-4 ─────────────────────────

#[tokio::test]
async fn test_completion_nested_trait_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let trait_a = concat!(
        "<?php\n",
        "namespace App\\Traits;\n",
        "trait Timestamps {\n",
        "    public function getCreatedAt(): string { return ''; }\n",
        "}\n",
    );

    let trait_b = concat!(
        "<?php\n",
        "namespace App\\Traits;\n",
        "trait SoftDeletes {\n",
        "    public function trashed(): bool { return false; }\n",
        "}\n",
    );

    let composed_trait = concat!(
        "<?php\n",
        "namespace App\\Traits;\n",
        "use App\\Traits\\Timestamps;\n",
        "use App\\Traits\\SoftDeletes;\n",
        "trait ModelBehavior {\n",
        "    use Timestamps, SoftDeletes;\n",
        "    public function save(): bool { return true; }\n",
        "}\n",
    );

    let model_php = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "use App\\Traits\\ModelBehavior;\n",
        "class Post {\n",
        "    use ModelBehavior;\n",
        "    public string $title;\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("src/Traits/Timestamps.php", trait_a),
            ("src/Traits/SoftDeletes.php", trait_b),
            ("src/Traits/ModelBehavior.php", composed_trait),
            ("src/Models/Post.php", model_php),
        ],
    );

    let uri = Url::parse("file:///test_nested_trait.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: model_php.to_string(),
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

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"save"),
                "Should include ModelBehavior::save, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getCreatedAt"),
                "Should include nested Timestamps::getCreatedAt, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"trashed"),
                "Should include nested SoftDeletes::trashed, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait static members ───────────────────────────────────────────────────

#[tokio::test]
async fn test_completion_trait_static_methods_via_double_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait HasFactory {\n",
        "    public static function factory(): self { return new static(); }\n",
        "}\n",
        "class User {\n",
        "    use HasFactory;\n",
        "    public static function query(): string { return ''; }\n",
        "    function test() {\n",
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
                    line: 8,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"factory"),
                "Should include trait static method 'factory', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"query"),
                "Should include own static method 'query', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Variable typed as class that uses trait ────────────────────────────────

#[tokio::test]
async fn test_completion_variable_of_class_with_trait() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Printable {\n",
        "    public function print(): void {}\n",
        "}\n",
        "class Document {\n",
        "    use Printable;\n",
        "    public function getTitle(): string { return ''; }\n",
        "}\n",
        "class Consumer {\n",
        "    public function test() {\n",
        "        $doc = new Document();\n",
        "        $doc->\n",
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
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"print"),
                "Should include trait method 'print' on variable, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getTitle"),
                "Should include own method 'getTitle' on variable, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait in parent class is inherited by child ────────────────────────────

#[tokio::test]
async fn test_completion_child_inherits_parent_trait_members() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///parent_trait.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Serializable {\n",
        "    public function serialize(): string { return ''; }\n",
        "}\n",
        "class BaseModel {\n",
        "    use Serializable;\n",
        "    public function getId(): int { return 0; }\n",
        "}\n",
        "class User extends BaseModel {\n",
        "    public function getEmail(): string { return ''; }\n",
        "    function test() {\n",
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
                    line: 11,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Own method
            assert!(
                method_names.contains(&"getEmail"),
                "Should include own method 'getEmail', got: {:?}",
                method_names
            );
            // Parent method
            assert!(
                method_names.contains(&"getId"),
                "Should include parent method 'getId', got: {:?}",
                method_names
            );
            // Trait method from parent's trait
            assert!(
                method_names.contains(&"serialize"),
                "Should include trait method 'serialize' from parent, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait used in both class and parent (no duplicates) ────────────────────

#[tokio::test]
async fn test_completion_no_duplicate_members_from_trait() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_no_dup.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Loggable {\n",
        "    public function log(): void {}\n",
        "}\n",
        "class Base {\n",
        "    use Loggable;\n",
        "}\n",
        "class Child extends Base {\n",
        "    use Loggable;\n",
        "    function test() {\n",
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

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let log_count = items
                .iter()
                .filter(|i| {
                    i.kind == Some(CompletionItemKind::METHOD)
                        && i.filter_text.as_deref() == Some("log")
                })
                .count();

            assert_eq!(
                log_count, 1,
                "Should have exactly one 'log' method (no duplicates), got: {}",
                log_count
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait with method that returns typed object (chaining) ─────────────────

#[tokio::test]
async fn test_completion_trait_method_return_type_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function build(): string { return ''; }\n",
        "}\n",
        "trait HasBuilder {\n",
        "    public function getBuilder(): Builder { return new Builder(); }\n",
        "}\n",
        "class Service {\n",
        "    use HasBuilder;\n",
        "    function test() {\n",
        "        $this->getBuilder()->\n",
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
                    character: 30,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"build"),
                "Should resolve trait method return type and chain to Builder::build, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait parsed correctly (parser test) ───────────────────────────────────

#[tokio::test]
async fn test_parser_extracts_trait_info() {
    let backend = create_test_backend();

    let text = concat!(
        "<?php\n",
        "trait MyTrait {\n",
        "    public function traitMethod(): void {}\n",
        "    public string $traitProp;\n",
        "}\n",
        "class MyClass {\n",
        "    use MyTrait;\n",
        "    public function classMethod(): void {}\n",
        "}\n",
    );

    let classes = backend.parse_php(text);

    // Should have both the trait and the class
    assert_eq!(classes.len(), 2, "Should have trait + class");

    let trait_info = classes.iter().find(|c| c.name == "MyTrait").unwrap();
    assert_eq!(trait_info.methods.len(), 1);
    assert_eq!(trait_info.methods[0].name, "traitMethod");
    assert_eq!(trait_info.properties.len(), 1);
    assert_eq!(trait_info.properties[0].name, "traitProp");
    assert!(trait_info.parent_class.is_none());

    let class_info = classes.iter().find(|c| c.name == "MyClass").unwrap();
    assert_eq!(class_info.methods.len(), 1);
    assert_eq!(class_info.methods[0].name, "classMethod");
    assert_eq!(class_info.used_traits.len(), 1);
    assert_eq!(class_info.used_traits[0], "MyTrait");
}

// ─── Trait with namespace parsed correctly ──────────────────────────────────

#[tokio::test]
async fn test_parser_resolves_trait_names_with_use_statements() {
    let backend = create_test_backend();

    let uri = "file:///parse_ns_trait.php";
    let text = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "use App\\Traits\\Auditable;\n",
        "class User {\n",
        "    use Auditable;\n",
        "}\n",
    );

    // update_ast resolves trait names via use-map and namespace
    backend.update_ast(uri, text);

    let classes = backend
        .get_classes_for_uri(uri)
        .expect("Should have AST entries");

    let user = classes.iter().find(|c| c.name == "User").unwrap();
    assert_eq!(user.used_traits.len(), 1);
    // The trait name should be resolved to FQN via the `use` statement
    assert_eq!(
        user.used_traits[0], "App\\Traits\\Auditable",
        "Trait name should be resolved to FQN, got: {}",
        user.used_traits[0]
    );
}

// ─── Go-to-definition for trait method ──────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_trait_method_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_goto.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "trait Greetable {\n",                     // 1
        "    public function greet(): string {\n", // 2
        "        return 'hello';\n",               // 3
        "    }\n",                                 // 4
        "}\n",                                     // 5
        "class Person {\n",                        // 6
        "    use Greetable;\n",                    // 7
        "    function test() {\n",                 // 8
        "        $this->greet();\n",               // 9
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
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 9,
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
        "Should resolve definition for trait method"
    );
    if let Some(GotoDefinitionResponse::Scalar(location)) = result {
        assert_eq!(location.uri, uri);
        // Should point to the `greet` method definition in the trait (line 2)
        assert_eq!(
            location.range.start.line, 2,
            "Should jump to trait method definition line"
        );
    } else {
        panic!("Expected GotoDefinitionResponse::Scalar");
    }
}

// ─── Go-to-definition for trait method cross-file ───────────────────────────

#[tokio::test]
async fn test_goto_definition_trait_method_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let trait_php = concat!(
        "<?php\n",
        "namespace App\\Traits;\n",
        "trait Loggable {\n",
        "    public function logMessage(): void {}\n",
        "}\n",
    );

    let class_php = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "use App\\Traits\\Loggable;\n",
        "class Worker {\n",
        "    use Loggable;\n",
        "    function run() {\n",
        "        $this->logMessage();\n",
        "    }\n",
        "}\n",
    );

    let (backend, dir) = create_psr4_workspace(
        composer_json,
        &[
            ("src/Traits/Loggable.php", trait_php),
            ("src/Services/Worker.php", class_php),
        ],
    );

    let uri = Url::parse("file:///test_trait_goto.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: class_php.to_string(),
            },
        })
        .await;

    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 6,
                    character: 20,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should resolve definition for cross-file trait method"
    );
    if let Some(GotoDefinitionResponse::Scalar(location)) = result {
        let trait_path = dir.path().join("src/Traits/Loggable.php");
        let expected_uri = Url::from_file_path(&trait_path).unwrap();
        assert_eq!(location.uri, expected_uri, "Should point to the trait file");
        // Method definition should be on line 3
        assert_eq!(
            location.range.start.line, 3,
            "Should jump to trait method definition line"
        );
    } else {
        panic!("Expected GotoDefinitionResponse::Scalar");
    }
}

// ─── Multiple use statements (separate lines) ──────────────────────────────

#[tokio::test]
async fn test_completion_separate_use_statements_for_traits() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///separate_use.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait A {\n",
        "    public function fromA(): void {}\n",
        "}\n",
        "trait B {\n",
        "    public function fromB(): void {}\n",
        "}\n",
        "class MyClass {\n",
        "    use A;\n",
        "    use B;\n",
        "    function test() {\n",
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
                    line: 11,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"fromA"),
                "Should include A::fromA, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"fromB"),
                "Should include B::fromB, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait + interface + parent class combined ──────────────────────────────

#[tokio::test]
async fn test_completion_trait_with_interface_and_parent() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///combined.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Printable {\n",
        "    public function print(): void;\n",
        "}\n",
        "trait Loggable {\n",
        "    public function log(): void {}\n",
        "}\n",
        "class Base {\n",
        "    public function baseMethod(): void {}\n",
        "}\n",
        "class Document extends Base implements Printable {\n",
        "    use Loggable;\n",
        "    public function print(): void {}\n",
        "    function test() {\n",
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
                    line: 14,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"print"),
                "Should include own method 'print', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"log"),
                "Should include trait method 'log', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"baseMethod"),
                "Should include parent method 'baseMethod', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Param type hint with trait-using class ─────────────────────────────────

#[tokio::test]
async fn test_completion_param_type_hint_class_with_trait() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///param_trait.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Taggable {\n",
        "    public function addTag(string $tag): void {}\n",
        "    public function getTags(): array { return []; }\n",
        "}\n",
        "class Article {\n",
        "    use Taggable;\n",
        "    public function getTitle(): string { return ''; }\n",
        "}\n",
        "class Processor {\n",
        "    public function process(Article $article) {\n",
        "        $article->\n",
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
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"addTag"),
                "Should include trait method 'addTag' on param variable, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getTags"),
                "Should include trait method 'getTags' on param variable, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getTitle"),
                "Should include class method 'getTitle' on param variable, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Class with trait and child class ───────────────────────────────────────

#[tokio::test]
async fn test_completion_grandchild_inherits_trait_from_grandparent() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///grandchild_trait.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Identifiable {\n",
        "    public function getId(): int { return 0; }\n",
        "}\n",
        "class BaseModel {\n",
        "    use Identifiable;\n",
        "}\n",
        "class User extends BaseModel {\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Admin extends User {\n",
        "    public function getRole(): string { return ''; }\n",
        "    function test() {\n",
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
                    line: 13,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getRole"),
                "Should include own method 'getRole', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getName"),
                "Should include parent method 'getName', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getId"),
                "Should include grandparent trait method 'getId', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait definition itself is indexable ────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_trait_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///goto_trait_name.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "trait Fooable {\n",                    // 1
        "    public function foo(): void {}\n", // 2
        "}\n",                                  // 3
        "class Bar {\n",                        // 4
        "    use Fooable;\n",                   // 5
        "}\n",                                  // 6
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

    // Click on "Fooable" in the use statement (line 5)
    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 5,
                    character: 10,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should resolve goto-definition for trait name"
    );
    if let Some(GotoDefinitionResponse::Scalar(location)) = result {
        assert_eq!(location.uri, uri);
        // Should point to the trait declaration on line 1
        assert_eq!(
            location.range.start.line, 1,
            "Should jump to trait declaration line"
        );
    } else {
        panic!("Expected GotoDefinitionResponse::Scalar");
    }
}

// ─── Trait with docblock return types ───────────────────────────────────────

#[tokio::test]
async fn test_completion_trait_method_with_docblock_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_docblock.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Result {\n",
        "    public function isOk(): bool { return true; }\n",
        "}\n",
        "trait HasResult {\n",
        "    /** @return Result */\n",
        "    public function getResult() { return new Result(); }\n",
        "}\n",
        "class Handler {\n",
        "    use HasResult;\n",
        "    function test() {\n",
        "        $this->getResult()->\n",
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
                    character: 28,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"isOk"),
                "Should chain through trait method docblock return type to Result::isOk, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait `insteadof` conflict resolution ──────────────────────────────────

#[tokio::test]
async fn test_completion_trait_insteadof_basic() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_insteadof.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait TraitA {\n",
        "    public function hello(): string { return 'A'; }\n",
        "    public function shared(): string { return 'A'; }\n",
        "}\n",
        "trait TraitB {\n",
        "    public function world(): string { return 'B'; }\n",
        "    public function shared(): string { return 'B'; }\n",
        "}\n",
        "class MyClass {\n",
        "    use TraitA, TraitB {\n",
        "        TraitA::shared insteadof TraitB;\n",
        "    }\n",
        "    function test() {\n",
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
                    line: 14,
                    character: 15,
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

            assert!(
                method_names.contains(&"hello"),
                "Should include TraitA::hello, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"world"),
                "Should include TraitB::world, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"shared"),
                "Should include 'shared' (from TraitA via insteadof), got: {:?}",
                method_names
            );
            // `shared` should appear exactly once (TraitB's version excluded)
            let shared_count = method_names.iter().filter(|&&n| n == "shared").count();
            assert_eq!(
                shared_count, 1,
                "Should have exactly one 'shared' method, got: {}",
                shared_count
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_trait_insteadof_multiple_excluded() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_insteadof_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait TraitA {\n",
        "    public function doWork(): string { return 'A'; }\n",
        "}\n",
        "trait TraitB {\n",
        "    public function doWork(): string { return 'B'; }\n",
        "}\n",
        "trait TraitC {\n",
        "    public function doWork(): string { return 'C'; }\n",
        "}\n",
        "class Worker {\n",
        "    use TraitA, TraitB, TraitC {\n",
        "        TraitA::doWork insteadof TraitB, TraitC;\n",
        "    }\n",
        "    function test() {\n",
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
                    line: 15,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"doWork"),
                "Should include 'doWork' from TraitA, got: {:?}",
                method_names
            );
            let count = method_names.iter().filter(|&&n| n == "doWork").count();
            assert_eq!(
                count, 1,
                "Should have exactly one 'doWork' (B and C excluded), got: {}",
                count
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait `as` alias ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_completion_trait_as_alias_basic() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_as_alias.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait TraitA {\n",
        "    public function hello(): string { return 'A'; }\n",
        "    public function shared(): string { return 'A'; }\n",
        "}\n",
        "trait TraitB {\n",
        "    public function world(): string { return 'B'; }\n",
        "    public function shared(): string { return 'B'; }\n",
        "}\n",
        "class MyClass {\n",
        "    use TraitA, TraitB {\n",
        "        TraitA::shared insteadof TraitB;\n",
        "        TraitB::shared as sharedFromB;\n",
        "    }\n",
        "    function test() {\n",
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
                    line: 15,
                    character: 15,
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

            assert!(
                method_names.contains(&"hello"),
                "Should include TraitA::hello, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"world"),
                "Should include TraitB::world, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"shared"),
                "Should include 'shared' (from TraitA), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"sharedFromB"),
                "Should include alias 'sharedFromB' (from TraitB), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_trait_as_visibility_only() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_as_visibility.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Greeter {\n",
        "    public function greet(): string { return 'hi'; }\n",
        "}\n",
        "class MyClass {\n",
        "    use Greeter {\n",
        "        greet as protected;\n",
        "    }\n",
        "    function test() {\n",
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

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Method should still be present (visible from within the class)
            assert!(
                method_names.contains(&"greet"),
                "Should include 'greet' with changed visibility, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_trait_as_alias_with_visibility() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_as_vis_alias.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Logger {\n",
        "    public function log(): void {}\n",
        "}\n",
        "class Service {\n",
        "    use Logger {\n",
        "        Logger::log as private privateLog;\n",
        "    }\n",
        "    function test() {\n",
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

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"log"),
                "Should include original 'log' method, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"privateLog"),
                "Should include alias 'privateLog', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Combined insteadof + as ────────────────────────────────────────────────

#[tokio::test]
async fn test_completion_trait_insteadof_with_as_alias() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_combined.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Talker {\n",
        "    public function smallTalk(): string { return 'talker'; }\n",
        "    public function talk(): string { return 'talker'; }\n",
        "}\n",
        "trait Greeter {\n",
        "    public function greet(): string { return 'greeter'; }\n",
        "    public function talk(): string { return 'greeter'; }\n",
        "}\n",
        "class Person {\n",
        "    use Talker, Greeter {\n",
        "        Talker::talk insteadof Greeter;\n",
        "        Greeter::talk as greeterTalk;\n",
        "    }\n",
        "    function test() {\n",
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
                    line: 15,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"smallTalk"),
                "Should include Talker::smallTalk, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"greet"),
                "Should include Greeter::greet, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"talk"),
                "Should include 'talk' (from Talker via insteadof), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"greeterTalk"),
                "Should include alias 'greeterTalk' (from Greeter), got: {:?}",
                method_names
            );
            let talk_count = method_names.iter().filter(|&&n| n == "talk").count();
            assert_eq!(
                talk_count, 1,
                "Should have exactly one 'talk', got: {}",
                talk_count
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Cross-file insteadof / as ─────────────────────────────────────────────

#[tokio::test]
async fn test_completion_trait_insteadof_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let trait_a_php = concat!(
        "<?php\n",
        "namespace App\\Traits;\n",
        "trait TraitA {\n",
        "    public function onlyA(): string { return 'A'; }\n",
        "    public function conflict(): string { return 'A'; }\n",
        "}\n",
    );

    let trait_b_php = concat!(
        "<?php\n",
        "namespace App\\Traits;\n",
        "trait TraitB {\n",
        "    public function onlyB(): string { return 'B'; }\n",
        "    public function conflict(): string { return 'B'; }\n",
        "}\n",
    );

    let class_php = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "use App\\Traits\\TraitA;\n",
        "use App\\Traits\\TraitB;\n",
        "class Widget {\n",
        "    use TraitA, TraitB {\n",
        "        TraitA::conflict insteadof TraitB;\n",
        "        TraitB::conflict as conflictFromB;\n",
        "    }\n",
        "    function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("src/Traits/TraitA.php", trait_a_php),
            ("src/Traits/TraitB.php", trait_b_php),
            ("src/Models/Widget.php", class_php),
        ],
    );

    let uri = Url::parse("file:///test_cross_insteadof.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: class_php.to_string(),
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

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"onlyA"),
                "Should include TraitA::onlyA, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"onlyB"),
                "Should include TraitB::onlyB, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"conflict"),
                "Should include 'conflict' (from TraitA), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"conflictFromB"),
                "Should include alias 'conflictFromB' (from TraitB), got: {:?}",
                method_names
            );
            let conflict_count = method_names.iter().filter(|&&n| n == "conflict").count();
            assert_eq!(
                conflict_count, 1,
                "Should have exactly one 'conflict', got: {}",
                conflict_count
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Class own method overrides trait insteadof ─────────────────────────────

#[tokio::test]
async fn test_completion_class_own_method_wins_over_insteadof() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_own_wins.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait TraitA {\n",
        "    public function doIt(): string { return 'A'; }\n",
        "}\n",
        "trait TraitB {\n",
        "    public function doIt(): string { return 'B'; }\n",
        "}\n",
        "class MyClass {\n",
        "    use TraitA, TraitB {\n",
        "        TraitA::doIt insteadof TraitB;\n",
        "    }\n",
        "    public function doIt(): int { return 42; }\n",
        "    function test() {\n",
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
                    line: 13,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"doIt"),
                "Should include 'doIt', got: {:?}",
                method_names
            );
            let count = method_names.iter().filter(|&&n| n == "doIt").count();
            assert_eq!(
                count, 1,
                "Class own method should win, exactly one 'doIt', got: {}",
                count
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Alias preserves return type ────────────────────────────────────────────

#[tokio::test]
async fn test_completion_trait_alias_preserves_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_alias_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "trait Finder {\n",
        "    public function find(): User { return new User(); }\n",
        "}\n",
        "trait Loader {\n",
        "    public function find(): User { return new User(); }\n",
        "}\n",
        "class Repository {\n",
        "    use Finder, Loader {\n",
        "        Finder::find insteadof Loader;\n",
        "        Loader::find as loadFind;\n",
        "    }\n",
        "    function test() {\n",
        "        $this->loadFind()->\n",
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
                    line: 16,
                    character: 28,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Alias return type should chain — 'loadFind()' returns User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Variable typed as class with trait adaptations ─────────────────────────

#[tokio::test]
async fn test_completion_variable_of_class_with_trait_insteadof() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_var_insteadof.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Encoder {\n",
        "    public function encode(): string { return ''; }\n",
        "    public function process(): string { return 'encoder'; }\n",
        "}\n",
        "trait Decoder {\n",
        "    public function decode(): string { return ''; }\n",
        "    public function process(): string { return 'decoder'; }\n",
        "}\n",
        "class Codec {\n",
        "    use Encoder, Decoder {\n",
        "        Encoder::process insteadof Decoder;\n",
        "        Decoder::process as decoderProcess;\n",
        "    }\n",
        "}\n",
        "function test() {\n",
        "    $codec = new Codec();\n",
        "    $codec->\n",
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
                    line: 17,
                    character: 12,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"encode"),
                "Should include 'encode', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"decode"),
                "Should include 'decode', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"process"),
                "Should include 'process' (from Encoder), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"decoderProcess"),
                "Should include alias 'decoderProcess', got: {:?}",
                method_names
            );
            let process_count = method_names.iter().filter(|&&n| n == "process").count();
            assert_eq!(
                process_count, 1,
                "Should have exactly one 'process', got: {}",
                process_count
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Parser extraction ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_parser_extracts_trait_adaptations() {
    let backend = create_test_backend();

    let text = concat!(
        "<?php\n",
        "trait A {\n",
        "    public function foo(): void {}\n",
        "}\n",
        "trait B {\n",
        "    public function foo(): void {}\n",
        "}\n",
        "class C {\n",
        "    use A, B {\n",
        "        A::foo insteadof B;\n",
        "        B::foo as bFoo;\n",
        "        A::foo as protected;\n",
        "    }\n",
        "}\n",
    );

    let classes = backend.parse_php(text);
    let class_c = classes
        .iter()
        .find(|c| c.name == "C")
        .expect("Should find class C");

    // Check precedences
    assert_eq!(
        class_c.trait_precedences.len(),
        1,
        "Should have 1 precedence"
    );
    assert_eq!(class_c.trait_precedences[0].trait_name, "A");
    assert_eq!(class_c.trait_precedences[0].method_name, "foo");
    assert_eq!(class_c.trait_precedences[0].insteadof, vec!["B"]);

    // Check aliases
    assert_eq!(class_c.trait_aliases.len(), 2, "Should have 2 aliases");

    // B::foo as bFoo
    let alias_b = class_c
        .trait_aliases
        .iter()
        .find(|a| a.alias.as_deref() == Some("bFoo"))
        .expect("Should have bFoo alias");
    assert_eq!(alias_b.trait_name.as_deref(), Some("B"));
    assert_eq!(alias_b.method_name, "foo");
    assert!(alias_b.visibility.is_none());

    // A::foo as protected (visibility-only)
    let alias_a = class_c
        .trait_aliases
        .iter()
        .find(|a| a.alias.is_none())
        .expect("Should have visibility-only alias");
    assert_eq!(alias_a.trait_name.as_deref(), Some("A"));
    assert_eq!(alias_a.method_name, "foo");
    assert_eq!(
        alias_a.visibility,
        Some(phpantom_lsp::types::Visibility::Protected)
    );
}

// ─── Unqualified `as` reference (no trait name prefix) ──────────────────────

#[tokio::test]
async fn test_completion_trait_as_unqualified_reference() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_unqualified_as.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Greeter {\n",
        "    public function greet(): string { return 'hi'; }\n",
        "}\n",
        "class MyClass {\n",
        "    use Greeter {\n",
        "        greet as helloWorld;\n",
        "    }\n",
        "    function test() {\n",
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

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"greet"),
                "Should still include original 'greet', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"helloWorld"),
                "Should include alias 'helloWorld', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait property @var docblock type resolution ───────────────────────────

/// When a trait defines a property with a `@var` docblock type, and a class
/// uses that trait (possibly through inheritance), chaining on `$this->prop`
/// should resolve to the docblock type and offer its members.
#[tokio::test]
async fn test_completion_trait_property_docblock_type_chain_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_prop_docblock.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class OutputStyle {\n",
        "    public function createProgressBar(): ProgressBar { return new ProgressBar(); }\n",
        "}\n",
        "class ProgressBar {\n",
        "    public function advance(): void {}\n",
        "}\n",
        "trait InteractsWithIO {\n",
        "    /** @var OutputStyle */\n",
        "    protected $output;\n",
        "}\n",
        "class Command {\n",
        "    use InteractsWithIO;\n",
        "}\n",
        "final class ReindexSelectedCommand extends Command {\n",
        "    public function handle(): int {\n",
        "        $this->output->\n",
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

    // Completion on `$this->output->` inside ReindexSelectedCommand
    let result = backend
        .completion(CompletionParams {
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
                method_names.contains(&"createProgressBar"),
                "Should resolve trait property @var type and show OutputStyle::createProgressBar, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file PSR-4 variant: the trait, the parent class, and the child
/// class live in separate files.
#[tokio::test]
async fn test_completion_trait_property_docblock_type_chain_cross_file() {
    let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
    let trait_file = concat!(
        "<?php\n",
        "namespace App\\Concerns;\n",
        "use App\\OutputStyle;\n",
        "trait InteractsWithIO {\n",
        "    /** @var OutputStyle */\n",
        "    protected $output;\n",
        "}\n",
    );
    let output_style_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "class OutputStyle {\n",
        "    public function createProgressBar(): ProgressBar { return new ProgressBar(); }\n",
        "}\n",
    );
    let progress_bar_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "class ProgressBar {\n",
        "    public function advance(): void {}\n",
        "}\n",
    );
    let command_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Concerns\\InteractsWithIO;\n",
        "class Command {\n",
        "    use InteractsWithIO;\n",
        "}\n",
    );
    let child_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "final class ReindexSelectedCommand extends Command {\n",
        "    public function handle(): int {\n",
        "        $this->output->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Concerns/InteractsWithIO.php", trait_file),
            ("src/OutputStyle.php", output_style_file),
            ("src/ProgressBar.php", progress_bar_file),
            ("src/Command.php", command_file),
            ("src/ReindexSelectedCommand.php", child_file),
        ],
    );

    let uri = Url::from_file_path(_dir.path().join("src/ReindexSelectedCommand.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: child_file.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 4,
                    character: 24,
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
                method_names.contains(&"createProgressBar"),
                "Should resolve cross-file trait property @var type, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Edge case: the trait lives in a completely different namespace from the
/// consuming class, and the `@var` type is imported via `use` in the trait
/// file but NOT in the consuming class file. The type should still resolve
/// because `resolve_parent_class_names` resolves property type hints to FQN
/// at parse time using the trait file's own use_map.
#[tokio::test]
async fn test_completion_trait_property_docblock_type_different_namespace() {
    let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
    let trait_file = concat!(
        "<?php\n",
        "namespace App\\Concerns;\n",
        "use App\\Support\\OutputStyle;\n",
        "trait InteractsWithIO {\n",
        "    /** @var OutputStyle */\n",
        "    protected $output;\n",
        "}\n",
    );
    let output_style_file = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class OutputStyle {\n",
        "    public function createProgressBar(): ProgressBar { return new ProgressBar(); }\n",
        "}\n",
    );
    let progress_bar_file = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class ProgressBar {\n",
        "    public function advance(): void {}\n",
        "}\n",
    );
    let command_file = concat!(
        "<?php\n",
        "namespace App\\Console;\n",
        "use App\\Concerns\\InteractsWithIO;\n",
        "class Command {\n",
        "    use InteractsWithIO;\n",
        "}\n",
    );
    // Child is in App\Console\Commands — a completely different namespace
    // from App\Support where OutputStyle lives. OutputStyle is NOT imported
    // in this file.
    let child_file = concat!(
        "<?php\n",
        "namespace App\\Console\\Commands;\n",
        "use App\\Console\\Command;\n",
        "final class ReindexSelectedCommand extends Command {\n",
        "    public function handle(): int {\n",
        "        $this->output->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Concerns/InteractsWithIO.php", trait_file),
            ("src/Support/OutputStyle.php", output_style_file),
            ("src/Support/ProgressBar.php", progress_bar_file),
            ("src/Console/Command.php", command_file),
            (
                "src/Console/Commands/ReindexSelectedCommand.php",
                child_file,
            ),
        ],
    );

    let uri = Url::from_file_path(
        _dir.path()
            .join("src/Console/Commands/ReindexSelectedCommand.php"),
    )
    .unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: child_file.to_string(),
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

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"createProgressBar"),
                "Should resolve trait property @var type across different namespaces, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// FQN variant with leading backslash: `@var \App\Support\OutputStyle`.
/// The leading `\` should be preserved by `resolve_name` and handled
/// correctly by downstream resolution.
#[tokio::test]
async fn test_completion_trait_property_fqn_leading_backslash() {
    let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
    let trait_file = concat!(
        "<?php\n",
        "namespace App\\Concerns;\n",
        "trait InteractsWithIO {\n",
        "    /** @var \\App\\Support\\OutputStyle */\n",
        "    protected $output;\n",
        "}\n",
    );
    let output_style_file = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class OutputStyle {\n",
        "    public function createProgressBar(): void {}\n",
        "}\n",
    );
    let command_file = concat!(
        "<?php\n",
        "namespace App\\Console;\n",
        "use App\\Concerns\\InteractsWithIO;\n",
        "class Command {\n",
        "    use InteractsWithIO;\n",
        "}\n",
    );
    let child_file = concat!(
        "<?php\n",
        "namespace App\\Console\\Commands;\n",
        "use App\\Console\\Command;\n",
        "final class ReindexSelectedCommand extends Command {\n",
        "    public function handle(): int {\n",
        "        $this->output->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Concerns/InteractsWithIO.php", trait_file),
            ("src/Support/OutputStyle.php", output_style_file),
            ("src/Console/Command.php", command_file),
            (
                "src/Console/Commands/ReindexSelectedCommand.php",
                child_file,
            ),
        ],
    );

    let uri = Url::from_file_path(
        _dir.path()
            .join("src/Console/Commands/ReindexSelectedCommand.php"),
    )
    .unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: child_file.to_string(),
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

    assert!(result.is_some(), "Should return completion results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"createProgressBar"),
                "Should resolve FQN @var type with leading backslash across namespaces, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Go-to-definition on a method called on a trait property's `@var` type.
/// The child class inherits `$output` from a trait via a parent class, and
/// go-to-definition on `createProgressBar()` should jump to `OutputStyle`.
#[tokio::test]
async fn test_goto_definition_method_on_trait_property_docblock_type() {
    let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
    let trait_file = concat!(
        "<?php\n",
        "namespace App\\Concerns;\n",
        "use App\\OutputStyle;\n",
        "trait InteractsWithIO {\n",
        "    /** @var OutputStyle */\n",
        "    protected $output;\n",
        "}\n",
    );
    let output_style_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "class OutputStyle {\n",
        "    public function createProgressBar(): ProgressBar { return new ProgressBar(); }\n",
        "}\n",
    );
    let progress_bar_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "class ProgressBar {\n",
        "    public function advance(): void {}\n",
        "}\n",
    );
    let command_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Concerns\\InteractsWithIO;\n",
        "class Command {\n",
        "    use InteractsWithIO;\n",
        "}\n",
    );
    let child_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "final class ReindexSelectedCommand extends Command {\n",
        "    public function handle(): int {\n",
        "        $bar = $this->output->createProgressBar();\n",
        "    }\n",
        "}\n",
    );

    let (backend, dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Concerns/InteractsWithIO.php", trait_file),
            ("src/OutputStyle.php", output_style_file),
            ("src/ProgressBar.php", progress_bar_file),
            ("src/Command.php", command_file),
            ("src/ReindexSelectedCommand.php", child_file),
        ],
    );

    let uri = Url::from_file_path(dir.path().join("src/ReindexSelectedCommand.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: child_file.to_string(),
            },
        })
        .await;

    // Go-to-definition on `createProgressBar` (line 4, inside the method name)
    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 4,
                    character: 35,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    assert!(
        result.is_some(),
        "Should resolve go-to-definition for method on trait property's @var type"
    );
    if let Some(GotoDefinitionResponse::Scalar(location)) = result {
        let expected_uri = Url::from_file_path(dir.path().join("src/OutputStyle.php")).unwrap();
        assert_eq!(
            location.uri, expected_uri,
            "Should jump to OutputStyle file"
        );
        assert_eq!(
            location.range.start.line, 3,
            "Should jump to createProgressBar definition line"
        );
    } else {
        panic!("Expected GotoDefinitionResponse::Scalar");
    }
}

/// FQN variant: when the `@var` type uses a fully-qualified name (leading `\`),
/// the type should still resolve correctly.
#[tokio::test]
async fn test_completion_trait_property_fqn_var_type() {
    let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
    let trait_file = concat!(
        "<?php\n",
        "namespace App\\Concerns;\n",
        "trait InteractsWithIO {\n",
        "    /** @var \\App\\OutputStyle */\n",
        "    protected $output;\n",
        "}\n",
    );
    let output_style_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "class OutputStyle {\n",
        "    public function createProgressBar(): void {}\n",
        "}\n",
    );
    let command_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Concerns\\InteractsWithIO;\n",
        "class Command {\n",
        "    use InteractsWithIO;\n",
        "}\n",
    );
    let child_file = concat!(
        "<?php\n",
        "namespace App;\n",
        "final class ReindexSelectedCommand extends Command {\n",
        "    public function handle(): int {\n",
        "        $this->output->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Concerns/InteractsWithIO.php", trait_file),
            ("src/OutputStyle.php", output_style_file),
            ("src/Command.php", command_file),
            ("src/ReindexSelectedCommand.php", child_file),
        ],
    );

    let uri = Url::from_file_path(_dir.path().join("src/ReindexSelectedCommand.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: child_file.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 4,
                    character: 24,
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
                method_names.contains(&"createProgressBar"),
                "Should resolve FQN @var type on trait property, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── $this return type from trait methods ────────────────────────────────────

#[tokio::test]
async fn test_this_return_from_trait_method_resolves_to_using_class() {
    // When a trait method returns `$this`, the resolver should treat it as
    // the concrete class that uses the trait, not the trait itself.
    // This allows chaining into methods defined on the class or its parents.
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_this_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait MakesHttpRequests {\n",
        "    /** @return $this */\n",
        "    public function withHeaders(array $headers): static { return $this; }\n",
        "}\n",
        "class TestCase {\n",
        "    public function post(string $uri): string { return ''; }\n",
        "}\n",
        "class FeatureTest extends TestCase {\n",
        "    use MakesHttpRequests;\n",
        "    public function testSomething() {\n",
        "        $this->withHeaders([])->\n",
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

    // "$this->withHeaders([])->" at line 11, character 33
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 11,
                    character: 33,
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
                method_names.contains(&"post"),
                "withHeaders() returns $this which should resolve to FeatureTest, offering post() from parent TestCase, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"withHeaders"),
                "Should also offer withHeaders() from the trait for further chaining, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_this_return_from_trait_method_cross_file() {
    // Cross-file variant: the trait, parent class, and child class live in
    // separate files loaded via PSR-4.  When the trait method returns `$this`,
    // it should resolve to the concrete class so that parent methods are
    // available for chaining.
    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/",
                "App\\Traits\\": "src/Traits/",
                "App\\Http\\": "src/Http/"
            }
        }
    }"#;

    let trait_php = "\
<?php
namespace App\\Traits;
trait MakesHttpRequests {
    /** @return $this */
    public function withHeaders(array $headers): static { return $this; }
    /** @return $this */
    public function withCookies(array $cookies): static { return $this; }
}
";

    let base_php = "\
<?php
namespace App\\Http;
class TestCase {
    public function post(string $uri): string { return ''; }
    public function get(string $uri): string { return ''; }
}
";

    let feature_php = "\
<?php
namespace App\\Http;
use App\\Traits\\MakesHttpRequests;
class FeatureTest extends TestCase {
    use MakesHttpRequests;
    public function testSomething() {
        $this->withHeaders([])->
    }
}
";

    let (backend, dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Traits/MakesHttpRequests.php", trait_php),
            ("src/Http/TestCase.php", base_php),
            ("src/Http/FeatureTest.php", feature_php),
        ],
    );

    let uri = Url::from_file_path(dir.path().join("src/Http/FeatureTest.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: feature_php.to_string(),
            },
        })
        .await;

    // "$this->withHeaders([])->" at line 6, character 33
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 6,
                    character: 33,
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
                method_names.contains(&"post"),
                "withHeaders() returns $this, should resolve to FeatureTest offering post() from parent TestCase, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"get"),
                "Should also offer get() from parent TestCase, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"withCookies"),
                "Should offer withCookies() from the trait for further chaining, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Variable type resolution inside trait method bodies ────────────────────

/// Variables assigned inside a trait method body should resolve their type.
/// Previously, `resolve_variable_in_statements` did not handle
/// `Statement::Trait`, so `$var = new ClassName()` inside a trait method
/// produced no completions on `$var->`.
#[tokio::test]
async fn test_variable_resolution_inside_trait_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "trait IsAuditableTrait {\n",
        "    public function transformAudit(): array {\n",
        "        $user = new Customer();\n",
        "        $user->\n",
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
                // "$user->" at line 8, character 15
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

    match result {
        Some(CompletionResponse::Array(items))
        | Some(CompletionResponse::List(CompletionList { items, .. })) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Variable inside trait method should resolve to Customer with getName(), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Variable inside trait method should resolve to Customer with getEmail(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected completion results for $user-> inside trait method"),
    }
}

/// Variables assigned from method calls inside trait bodies should also resolve.
#[tokio::test]
async fn test_variable_from_method_call_inside_trait() {
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Order.php",
                "<?php\nnamespace App;\nclass Order {\n    public function getTotal(): float { return 0.0; }\n}\n",
            ),
            (
                "src/OrderService.php",
                "<?php\nnamespace App;\nclass OrderService {\n    public function findOrder(): Order { return new Order(); }\n}\n",
            ),
            (
                "src/AuditTrait.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "trait AuditTrait {\n",
                    "    public function audit(): void {\n",
                    "        $svc = new OrderService();\n",
                    "        $order = $svc->findOrder();\n",
                    "        $order->\n",
                    "    }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let trait_content = concat!(
        "<?php\n",
        "namespace App;\n",
        "trait AuditTrait {\n",
        "    public function audit(): void {\n",
        "        $svc = new OrderService();\n",
        "        $order = $svc->findOrder();\n",
        "        $order->\n",
        "    }\n",
        "}\n",
    );

    let uri = Url::from_file_path(dir.path().join("src/AuditTrait.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: trait_content.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                // "$order->" at line 6, character 16
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

    match result {
        Some(CompletionResponse::Array(items))
        | Some(CompletionResponse::List(CompletionList { items, .. })) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getTotal"),
                "Chained variable inside trait should resolve to Order with getTotal(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected completion results for $order-> inside trait method"),
    }
}

/// Parameter type hints inside trait methods should resolve for completion.
#[tokio::test]
async fn test_parameter_type_hint_inside_trait_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Request {\n",
        "    public function getMethod(): string { return ''; }\n",
        "    public function getPath(): string { return ''; }\n",
        "}\n",
        "trait HandlesRequests {\n",
        "    public function handle(Request $req): void {\n",
        "        $req->\n",
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
                // "$req->" at line 7, character 14
                position: Position {
                    line: 7,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    match result {
        Some(CompletionResponse::Array(items))
        | Some(CompletionResponse::List(CompletionList { items, .. })) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getMethod"),
                "Parameter type hint inside trait method should resolve, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getPath"),
                "Parameter type hint inside trait method should resolve, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected completion results for $req-> inside trait method"),
    }
}

// ─── Cross-file variable resolution inside trait method bodies ──────────────

/// When a trait is in its own file with a `use` import for a class defined
/// in another file, `$var = new ClassName(); $var->` should resolve.
/// This reproduces the real-world bug where variable resolution inside
/// traits failed in cross-file PSR-4 workspaces even though it worked
/// for classes.
#[tokio::test]
async fn test_cross_file_variable_resolution_inside_trait_method() {
    let customer_php = "\
<?php
namespace App\\Models;
class Customer {
    public function getName(): string { return ''; }
    public function getEmail(): string { return ''; }
}
";
    let trait_php = "\
<?php
namespace App\\Traits;
use App\\Models\\Customer;
trait IsAuditableTrait {
    public function transformAudit(): array {
        $user = new Customer();
        $user->
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\Models\\": "src/Models/", "App\\Traits\\": "src/Traits/" } } }"#,
        &[
            ("src/Models/Customer.php", customer_php),
            ("src/Traits/IsAuditableTrait.php", trait_php),
        ],
    );

    let uri = Url::from_file_path(dir.path().join("src/Traits/IsAuditableTrait.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: trait_php.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                // "$user->" at line 6, character 15
                position: Position {
                    line: 6,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    match result {
        Some(CompletionResponse::Array(items))
        | Some(CompletionResponse::List(CompletionList { items, .. })) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Cross-file variable inside trait should resolve Customer with getName(), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Cross-file variable inside trait should resolve Customer with getEmail(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected completion results for $user-> inside cross-file trait method"),
    }
}

// ─── Phase 4B: Trait resolution (ported from Mago) ──────────────────────────

#[tokio::test]
async fn test_trait_constant_access_via_static_and_this() {
    // Ported from Mago trait_constant_access.php
    // Trait constants accessed via static:: and $this:: inside the using class
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_const_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait HasSettings {\n",
        "    public const TIMEOUT = 30;\n",
        "    public const LABEL = 'default';\n",
        "}\n",
        "class Service {\n",
        "    use HasSettings;\n",
        "    public const NAME = 'svc';\n",
        "    function testStatic() {\n",
        "        static::\n",
        "    }\n",
        "    function testThis() {\n",
        "        $this::\n",
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

    // static:: at line 9, character 16
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 9,
                    character: 16,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let const_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                const_names.contains(&"TIMEOUT"),
                "static:: should include trait constant TIMEOUT, got: {:?}",
                const_names
            );
            assert!(
                const_names.contains(&"LABEL"),
                "static:: should include trait constant LABEL, got: {:?}",
                const_names
            );
            assert!(
                const_names.contains(&"NAME"),
                "static:: should include own constant NAME, got: {:?}",
                const_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }

    // $this:: at line 12, character 15
    let result = backend
        .completion(CompletionParams {
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
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let const_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                const_names.contains(&"TIMEOUT"),
                "$this:: should include trait constant TIMEOUT, got: {:?}",
                const_names
            );
            assert!(
                const_names.contains(&"LABEL"),
                "$this:: should include trait constant LABEL, got: {:?}",
                const_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_trait_alias_with_self_return_resolves_to_using_class() {
    // Ported from Mago trait_alias_vis_self.php
    // Trait method returning `self` aliased with visibility change should
    // resolve self to the using class, enabling chaining.
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_alias_self.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait SomeTrait {\n",
        "    protected function someTraitMethod(): self {\n",
        "        return $this;\n",
        "    }\n",
        "}\n",
        "class SomeClass {\n",
        "    use SomeTrait {\n",
        "        SomeTrait::someTraitMethod as public someClassMethod;\n",
        "    }\n",
        "    public function ownMethod(): string { return ''; }\n",
        "}\n",
        "$cls = new SomeClass();\n",
        "$cls->someClassMethod()->\n",
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

    // $cls->someClassMethod()-> at line 13, character 27
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 13,
                    character: 27,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"ownMethod"),
                "Aliased trait method returning self should resolve to SomeClass, offering ownMethod(), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"someClassMethod"),
                "Should also offer the aliased method for further chaining, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_self_in_closure_param_within_trait_resolves_to_using_class() {
    // Ported from Mago trait_method_closure_self.php
    // `self` used as a closure parameter type hint inside a trait method
    // should resolve to the class that uses the trait.
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_closure_self.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait FooTrait {\n",
        "    /** @param array<string, \\Closure(self):void> $steps */\n",
        "    protected function applySteps(array $steps): void {}\n",
        "}\n",
        "class Foo {\n",
        "    use FooTrait;\n",
        "    public function getName(): string { return ''; }\n",
        "    public function test(): void {\n",
        "        $this->applySteps([\n",
        "            'step1' => function (self $ctx): void {\n",
        "                $ctx->\n",
        "            },\n",
        "        ]);\n",
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

    // $ctx-> at line 11, character 22
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 11,
                    character: 22,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "self in closure param within trait should resolve to Foo, offering getName(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_require_extends_self_return_resolves_to_using_class() {
    // Ported from Mago trait_require_extends_self_return.php
    // Trait with @phpstan-require-extends returning self should resolve
    // to the concrete class that uses the trait.
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_require_extends.php").unwrap();
    let text = concat!(
        "<?php\n",
        "abstract class BaseElement {\n",
        "    abstract public function getName(): string;\n",
        "}\n",
        "/** @phpstan-require-extends BaseElement */\n",
        "trait NamedElement {\n",
        "    public function getThis(): self { return $this; }\n",
        "    public function getName(): string { return 'named'; }\n",
        "}\n",
        "class ConcreteElement extends BaseElement {\n",
        "    use NamedElement;\n",
        "    public function getTag(): string { return ''; }\n",
        "}\n",
        "$el = new ConcreteElement();\n",
        "$el->getThis()->\n",
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

    // $el->getThis()-> at line 14, character 18
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 14,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getTag"),
                "self return from require-extends trait should resolve to ConcreteElement, offering getTag(), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getName"),
                "Should also offer getName() from the trait/base, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_require_implements_self_return_resolves_to_using_class() {
    // Ported from Mago trait_require_implements_self_return.php
    // Trait with @phpstan-require-implements returning self should resolve
    // to the concrete class that uses the trait.
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_require_implements.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Element {\n",
        "    /** @param Element[] $children */\n",
        "    public function setChildren(array $children): self;\n",
        "}\n",
        "/** @phpstan-require-implements Element */\n",
        "trait IsElement {\n",
        "    /** @param Element[] $children */\n",
        "    public function setChildren(array $children): self { return $this; }\n",
        "}\n",
        "final class CollectionElement implements Element {\n",
        "    use IsElement;\n",
        "    public function getSize(): int { return 0; }\n",
        "}\n",
        "$el = new CollectionElement();\n",
        "$el->setChildren([])->\n",
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

    // $el->setChildren([])-> at line 15, character 24
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 15,
                    character: 24,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getSize"),
                "self return from require-implements trait should resolve to CollectionElement, offering getSize(), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"setChildren"),
                "Should also offer setChildren() for further chaining, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_require_extends_and_implements_combined_self_return() {
    // Ported from Mago trait_require_combined.php
    // Trait with both @phpstan-require-extends and @phpstan-require-implements
    // returning self should resolve to the concrete class.
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_require_combined.php").unwrap();
    let text = concat!(
        "<?php\n",
        "abstract class Base {\n",
        "    abstract public function getBase(): string;\n",
        "}\n",
        "interface Handler {\n",
        "    public function handle(): void;\n",
        "}\n",
        "/**\n",
        " * @phpstan-require-extends Base\n",
        " * @phpstan-require-implements Handler\n",
        " */\n",
        "trait CombinedTrait {\n",
        "    public function getSelf(): self { return $this; }\n",
        "    public function getBase(): string { return 'base'; }\n",
        "    public function handle(): void {}\n",
        "}\n",
        "class MyClass extends Base implements Handler {\n",
        "    use CombinedTrait;\n",
        "    public function extra(): int { return 1; }\n",
        "}\n",
        "$obj = new MyClass();\n",
        "$obj->getSelf()->\n",
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

    // $obj->getSelf()-> at line 21, character 19
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 21,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some());
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"extra"),
                "self return from combined require trait should resolve to MyClass, offering extra(), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getBase"),
                "Should offer getBase() from the trait/base, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"handle"),
                "Should offer handle() from the trait/interface, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── @phpstan-require-extends surfaces base members on $this ─────────────────

#[tokio::test]
async fn test_completion_require_extends_this_members_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///require_extends_same.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class TestBase {\n",
        "    public function mock(string $class): object { return new \\stdClass(); }\n",
        "}\n",
        "/** @phpstan-require-extends TestBase */\n",
        "trait MocksStorageService {\n",
        "    protected function helper(): void {\n",
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

    assert!(result.is_some(), "Completion should return results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"mock"),
                "Should include required base class method 'mock', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"helper"),
                "Should still include the trait's own method 'helper', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_require_extends_this_members_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "Tests\\": "tests/"
            }
        }
    }"#;

    let base_php = concat!(
        "<?php\n",
        "namespace Tests;\n",
        "class TestCase {\n",
        "    public function mock(string $class): object { return new \\stdClass(); }\n",
        "}\n",
    );

    let trait_php = concat!(
        "<?php\n",
        "namespace Tests\\Support;\n",
        "use Tests\\TestCase;\n",
        "/** @phpstan-require-extends \\Tests\\TestCase */\n",
        "trait MocksStorageService {\n",
        "    protected function helper(): void {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("tests/TestCase.php", base_php),
            ("tests/Support/MocksStorageService.php", trait_php),
        ],
    );

    // Open the trait file itself: it is analyzed standalone, exactly as
    // PHPStan never does (PHPStan only sees traits through using classes).
    let uri = Url::parse("file:///MocksStorageService.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: trait_php.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 6,
                    character: 15,
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

            assert!(
                method_names.contains(&"mock"),
                "Should include cross-file required base method 'mock', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Body-inferred `return $this` in trait methods ──────────────────────────

/// A trait method whose body is `return $this;` (no declared return type)
/// must resolve to the *using* class, so a chained call continues with the
/// using class's members, not the trait's.  Same-file variant.
#[tokio::test]
async fn test_return_this_trait_method_resolves_to_using_class_same_file() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///trait_return_this_sf.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait MakesAssertions {\n",
        "    function assertSee(string $v)\n",
        "    {\n",
        "        $ok = true;\n",
        "        return $this;\n",
        "    }\n",
        "}\n",
        "class Testable {\n",
        "    use MakesAssertions;\n",
        "    public int $count = 1;\n",
        "    function run() {\n",
        "        $t = new Testable();\n",
        "        $t->assertSee('a')->\n",
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
                    character: 28,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    match result.expect("Completion should return results") {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(""))
                .collect();
            assert!(
                names.contains(&"count"),
                "Chain through a trait's `return $this` should expose the using \
                 class's property `count`, got: {names:?}"
            );
            assert!(
                names.contains(&"assertSee"),
                "The trait method should still be available on the chain, got: {names:?}"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file variant: the trait lives in one PSR-4 file, the using class in
/// another, and the chained call in a third.  The body-inferred `return $this`
/// must still resolve to the using class (previously the foreign-file byte
/// offset made inference fail entirely, so the chain resolved to nothing).
#[tokio::test]
async fn test_return_this_trait_method_resolves_to_using_class_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/MakesAssertions.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "trait MakesAssertions {\n",
                    "    function assertSee(string $v)\n",
                    "    {\n",
                    "        $ok = true;\n",
                    "        return $this;\n",
                    "    }\n",
                    "}\n",
                ),
            ),
            (
                "src/Testable.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Testable {\n",
                    "    use MakesAssertions;\n",
                    "    public int $count = 1;\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///consumer.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Testable;\n",
        "function run(Testable $t) {\n",
        "    $t->assertSee('a')->\n",
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
                    character: 24,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    match result.expect("Completion should return results") {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(""))
                .collect();
            assert!(
                names.contains(&"count"),
                "Cross-file chain through a trait's `return $this` should expose \
                 the using class's property `count`, got: {names:?}"
            );
            assert!(
                names.contains(&"assertSee"),
                "The trait method should still be available on the chain, got: {names:?}"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}
