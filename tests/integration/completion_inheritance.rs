use crate::common::{create_psr4_workspace, create_test_backend};
use phpantom_lsp::Backend;
use std::collections::HashMap;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

static EXCEPTION_CHAIN_STUB: &str = r#"<?php
class Exception
{
    public function getMessage(): string {}
    public function getCode(): int {}
    public function getTrace(): array {}
}

class RuntimeException extends Exception {}
class PDOException extends RuntimeException {}
"#;

fn create_exception_chain_backend() -> Backend {
    let mut class_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    class_stubs.insert("Exception", EXCEPTION_CHAIN_STUB);
    class_stubs.insert("RuntimeException", EXCEPTION_CHAIN_STUB);
    class_stubs.insert("PDOException", EXCEPTION_CHAIN_STUB);
    Backend::new_test_with_all_stubs(class_stubs, HashMap::new(), HashMap::new())
}

// ─── Inheritance tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_completion_inherits_public_and_protected_methods_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inherit.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function breathe(): void {}\n",
        "    protected function sleep(): void {}\n",
        "    private function digest(): void {}\n",
        "}\n",
        "class Dog extends Animal {\n",
        "    public function bark(): void {}\n",
        "    function test() {\n",
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
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Own methods
            assert!(
                method_names.contains(&"bark"),
                "Should include own method 'bark'"
            );
            assert!(
                method_names.contains(&"test"),
                "Should include own method 'test'"
            );

            // Inherited public
            assert!(
                method_names.contains(&"breathe"),
                "Should include inherited public 'breathe'"
            );

            // Inherited protected
            assert!(
                method_names.contains(&"sleep"),
                "Should include inherited protected 'sleep'"
            );

            // Private should NOT be inherited
            assert!(
                !method_names.contains(&"digest"),
                "Should NOT include inherited private 'digest'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_inherits_properties_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inherit_props.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public string $publicProp;\n",
        "    protected int $protectedProp;\n",
        "    private float $privateProp;\n",
        "}\n",
        "class Child extends Base {\n",
        "    public string $ownProp;\n",
        "    function test() {\n",
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.label.as_str())
                .collect();

            assert!(
                prop_names.contains(&"ownProp"),
                "Should include own property"
            );
            assert!(
                prop_names.contains(&"publicProp"),
                "Should include inherited public property"
            );
            assert!(
                prop_names.contains(&"protectedProp"),
                "Should include inherited protected property"
            );
            assert!(
                !prop_names.contains(&"privateProp"),
                "Should NOT include inherited private property"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_inherits_constants_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inherit_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public const PUB_CONST = 1;\n",
        "    protected const PROT_CONST = 2;\n",
        "    private const PRIV_CONST = 3;\n",
        "}\n",
        "class Child extends Base {\n",
        "    const OWN_CONST = 4;\n",
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
                line: 9,
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
            let const_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.label.as_str())
                .collect();

            assert!(
                const_names.contains(&"OWN_CONST"),
                "Should include own constant"
            );
            assert!(
                const_names.contains(&"PUB_CONST"),
                "Should include inherited public constant"
            );
            assert!(
                const_names.contains(&"PROT_CONST"),
                "Should include inherited protected constant"
            );
            assert!(
                !const_names.contains(&"PRIV_CONST"),
                "Should NOT include inherited private constant"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_child_method_overrides_parent() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function greet(string $name): string { return ''; }\n",
        "    public function hello(): void {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    public function greet(string $name, string $greeting): string { return ''; }\n",
        "    function test() {\n",
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .collect();

            // 'greet' should appear exactly once (child's version)
            let greet_items: Vec<&&CompletionItem> = method_items
                .iter()
                .filter(|i| i.filter_text.as_deref() == Some("greet"))
                .collect();
            assert_eq!(
                greet_items.len(),
                1,
                "Overridden method 'greet' should appear exactly once"
            );

            // The label should show the child's signature (2 params)
            let greet_label = &greet_items[0].label;
            assert!(
                greet_label.contains("$greeting"),
                "Should use child's signature with $greeting, got: {}",
                greet_label
            );

            // 'hello' should be inherited from parent
            let hello_items: Vec<&&CompletionItem> = method_items
                .iter()
                .filter(|i| i.filter_text.as_deref() == Some("hello"))
                .collect();
            assert_eq!(hello_items.len(), 1, "Inherited 'hello' should appear once");
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_multi_level_inheritance_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///multi_level.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Grandparent {\n",
        "    public function ancestorMethod(): void {}\n",
        "    private function gpPrivate(): void {}\n",
        "}\n",
        "class ParentClass extends Grandparent {\n",
        "    public function parentMethod(): void {}\n",
        "    protected function parentProtected(): void {}\n",
        "}\n",
        "class ChildClass extends ParentClass {\n",
        "    public function childMethod(): void {}\n",
        "    function test() {\n",
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

    let completion_params = CompletionParams {
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

            // Own method
            assert!(
                method_names.contains(&"childMethod"),
                "Should include own 'childMethod'"
            );
            assert!(method_names.contains(&"test"), "Should include own 'test'");

            // From parent
            assert!(
                method_names.contains(&"parentMethod"),
                "Should include parent's 'parentMethod'"
            );
            assert!(
                method_names.contains(&"parentProtected"),
                "Should include parent's protected 'parentProtected'"
            );

            // From grandparent
            assert!(
                method_names.contains(&"ancestorMethod"),
                "Should include grandparent's 'ancestorMethod'"
            );

            // Private from grandparent should NOT appear
            assert!(
                !method_names.contains(&"gpPrivate"),
                "Should NOT include grandparent's private 'gpPrivate'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_inherits_static_members() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inherit_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public static function staticBase(): void {}\n",
        "    protected static string $staticProp = '';\n",
        "    private static function privateStatic(): void {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    public static function staticChild(): void {}\n",
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
                line: 9,
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
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.label.as_str())
                .collect();

            assert!(
                method_names.contains(&"staticChild"),
                "Should include own static method"
            );
            assert!(
                method_names.contains(&"staticBase"),
                "Should include inherited public static method"
            );
            assert!(
                !method_names.contains(&"privateStatic"),
                "Should NOT include inherited private static method"
            );
            assert!(
                prop_names.contains(&"$staticProp"),
                "Should include inherited protected static property"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_cross_file_inheritance_psr4() {
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
                "class BaseModel {\n",
                "    public function save(): bool { return true; }\n",
                "    protected function validate(): bool { return true; }\n",
                "    private function internalLog(): void {}\n",
                "    public string $id;\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\BaseModel;\n",
        "class User extends BaseModel {\n",
        "    public string $name;\n",
        "    public function getName(): string { return $this->name; }\n",
        "    function test() {\n",
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

    let completion_params = CompletionParams {
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
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for cross-file inheritance"
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

            // Own members
            assert!(
                method_names.contains(&"getName"),
                "Should include own 'getName'"
            );
            assert!(
                prop_names.contains(&"name"),
                "Should include own property 'name'"
            );

            // Inherited public from BaseModel
            assert!(
                method_names.contains(&"save"),
                "Should include inherited 'save' from BaseModel"
            );
            assert!(
                prop_names.contains(&"id"),
                "Should include inherited property 'id' from BaseModel"
            );

            // Inherited protected from BaseModel
            assert!(
                method_names.contains(&"validate"),
                "Should include inherited protected 'validate'"
            );

            // Private should NOT be inherited
            assert!(
                !method_names.contains(&"internalLog"),
                "Should NOT include inherited private 'internalLog'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_cross_file_multi_level_inheritance_psr4() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Base.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Base {\n",
                    "    public function baseMethod(): void {}\n",
                    "    protected function baseProtected(): void {}\n",
                    "    private function basePrivate(): void {}\n",
                    "}\n",
                ),
            ),
            (
                "src/Middle.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Middle extends Base {\n",
                    "    public function middleMethod(): void {}\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Middle;\n",
        "class Leaf extends Middle {\n",
        "    public function leafMethod(): void {}\n",
        "    function test() {\n",
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
    assert!(
        result.is_some(),
        "Completion should return results for multi-level cross-file inheritance"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"leafMethod"),
                "Should include own 'leafMethod'"
            );
            assert!(
                method_names.contains(&"middleMethod"),
                "Should include parent's 'middleMethod'"
            );
            assert!(
                method_names.contains(&"baseMethod"),
                "Should include grandparent's 'baseMethod'"
            );
            assert!(
                method_names.contains(&"baseProtected"),
                "Should include grandparent's protected 'baseProtected'"
            );
            assert!(
                !method_names.contains(&"basePrivate"),
                "Should NOT include grandparent's private 'basePrivate'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_override_across_files() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/ParentClass.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "class ParentClass {\n",
                "    public function render(): string { return ''; }\n",
                "    public function prepare(): void {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\ParentClass;\n",
        "class ChildView extends ParentClass {\n",
        "    public function render(string $template): string { return ''; }\n",
        "    function test() {\n",
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .collect();

            // 'render' should appear exactly once (child's version with $template param)
            let render_items: Vec<&&CompletionItem> = method_items
                .iter()
                .filter(|i| i.filter_text.as_deref() == Some("render"))
                .collect();
            assert_eq!(render_items.len(), 1, "'render' should appear exactly once");
            assert!(
                render_items[0].label.contains("$template"),
                "Should use child's signature with $template, got: {}",
                render_items[0].label
            );

            // 'prepare' should be inherited from parent
            let prepare_items: Vec<&&CompletionItem> = method_items
                .iter()
                .filter(|i| i.filter_text.as_deref() == Some("prepare"))
                .collect();
            assert_eq!(
                prepare_items.len(),
                1,
                "'prepare' should be inherited from parent"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_no_parent_class_unchanged_behavior() {
    // Verify that classes without extends still work exactly as before
    let backend = create_test_backend();

    let uri = Url::parse("file:///standalone.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Standalone {\n",
        "    public function doStuff(): void {}\n",
        "    private function internal(): void {}\n",
        "    function test() {\n",
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"doStuff"),
                "Should include 'doStuff'"
            );
            assert!(
                method_names.contains(&"internal"),
                "Own private methods should still appear"
            );
            assert!(method_names.contains(&"test"), "Should include 'test'");
            assert_eq!(method_names.len(), 3, "Should have exactly 3 methods");
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_variable_of_child_type_includes_inherited() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_inherit.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Vehicle {\n",
        "    public function start(): void {}\n",
        "    protected function fuelCheck(): bool { return true; }\n",
        "    private function internalDiag(): void {}\n",
        "}\n",
        "class Car extends Vehicle {\n",
        "    public function openTrunk(): void {}\n",
        "}\n",
        "class Garage {\n",
        "    function test() {\n",
        "        $car = new Car();\n",
        "        $car->\n",
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
                line: 12,
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
                method_names.contains(&"openTrunk"),
                "Should include Car's own 'openTrunk'"
            );
            assert!(
                method_names.contains(&"start"),
                "Should include inherited 'start' from Vehicle"
            );
            assert!(
                !method_names.contains(&"fuelCheck"),
                "Should NOT include inherited protected 'fuelCheck' from unrelated class"
            );
            assert!(
                !method_names.contains(&"internalDiag"),
                "Should NOT include inherited private 'internalDiag'"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_magic_methods_sorted_after_regular() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///magic_inherit.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function __construct() {}\n",
        "    public function __toString(): string { return ''; }\n",
        "    public function realMethod(): void {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    function test() {\n",
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
    assert!(result.is_some());

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // `__construct` is never offered via `->` (arrow) access.
            assert!(
                !method_names.contains(&"__construct"),
                "__construct should not appear via -> access"
            );
            // Other implemented magic methods are offered, but sorted after
            // the regular methods so they never appear at the top.
            assert!(
                method_names.contains(&"__toString"),
                "Implemented __toString should be offered, got: {method_names:?}"
            );
            assert!(
                method_names.contains(&"realMethod"),
                "Should include inherited 'realMethod'"
            );
            let regular_idx = items
                .iter()
                .position(|i| i.filter_text.as_deref() == Some("realMethod"))
                .unwrap();
            let magic_idx = items
                .iter()
                .position(|i| i.filter_text.as_deref() == Some("__toString"))
                .unwrap();
            assert!(
                items[regular_idx].sort_text < items[magic_idx].sort_text,
                "Regular method should sort before the magic method"
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a parent method declares `@return static`, calling it on a variable
/// typed as a subclass should resolve to the *subclass*, not the declaring
/// (parent) class.  This is PHP's late-static-binding semantics.
#[tokio::test]
async fn test_static_return_type_resolved_to_caller_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///static_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Builder {\n",
        "    /** @return static */\n",
        "    public function configure(): static { return $this; }\n",
        "    public function build(): void {}\n",
        "}\n",
        "class AppBuilder extends Builder {\n",
        "    public function setDebug(): void {}\n",
        "}\n",
        "class TestClass {\n",
        "    public function test() {\n",
        "        $builder = new AppBuilder();\n",
        "        $builder->configure()->\n",
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
                line: 12,
                character: 31,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    let method_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap())
        .collect();

    assert!(
        method_names.contains(&"setDebug"),
        "static return should resolve to AppBuilder and include setDebug. Got: {:?}",
        method_names
    );
    assert!(
        method_names.contains(&"configure"),
        "Should include inherited configure. Got: {:?}",
        method_names
    );
    assert!(
        method_names.contains(&"build"),
        "Should include inherited build. Got: {:?}",
        method_names
    );
}

/// Cross-file variant: parent with `@return static` lives in a separate
/// PSR-4 file. Completion on a subclass variable after calling the parent
/// method should still resolve to the subclass.
#[tokio::test]
async fn test_static_return_type_cross_file_psr4() {
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
                "src/Builder.php",
                concat!(
                    "<?php\n",
                    "namespace Acme;\n",
                    "class Builder {\n",
                    "    /** @return static */\n",
                    "    public function configure(): static { return $this; }\n",
                    "    public function build(): void {}\n",
                    "}\n",
                ),
            ),
            (
                "src/AppBuilder.php",
                concat!(
                    "<?php\n",
                    "namespace Acme;\n",
                    "class AppBuilder extends Builder {\n",
                    "    public function setDebug(): void {}\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use Acme\\AppBuilder;\n",
        "class Controller {\n",
        "    public function test() {\n",
        "        $builder = new AppBuilder();\n",
        "        $builder->configure()->\n",
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
                character: 31,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    let method_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap())
        .collect();

    assert!(
        method_names.contains(&"setDebug"),
        "Cross-file static return should resolve to AppBuilder. Got: {:?}",
        method_names
    );
    assert!(
        method_names.contains(&"configure"),
        "Should include inherited configure cross-file. Got: {:?}",
        method_names
    );
    assert!(
        method_names.contains(&"build"),
        "Should include inherited build cross-file. Got: {:?}",
        method_names
    );
}

/// Chained `static` return types: calling two fluent methods that each
/// return `static` should still resolve to the subclass, not the parent.
#[tokio::test]
async fn test_static_return_type_chained_calls() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///static_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Query {\n",
        "    /** @return static */\n",
        "    public function where(): static { return $this; }\n",
        "    /** @return static */\n",
        "    public function orderBy(): static { return $this; }\n",
        "    public function get(): array { return []; }\n",
        "}\n",
        "class UserQuery extends Query {\n",
        "    public function active(): static { return $this; }\n",
        "}\n",
        "class TestClass {\n",
        "    public function test() {\n",
        "        $q = new UserQuery();\n",
        "        $q->where()->orderBy()->\n",
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
                line: 14,
                character: 32,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    let method_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap())
        .collect();

    assert!(
        method_names.contains(&"active"),
        "Chained static returns should resolve to UserQuery. Got: {:?}",
        method_names
    );
    assert!(
        method_names.contains(&"where"),
        "Should include inherited where. Got: {:?}",
        method_names
    );
    assert!(
        method_names.contains(&"get"),
        "Should include inherited get. Got: {:?}",
        method_names
    );
}

/// Cross-file FQN return type resolution through an inheritance chain.
///
/// `LeadProvider extends Model`, and `Model::query()` declares
/// `@return \Illuminate\Database\Eloquent\Builder`. Completion on
/// `LeadProvider::query()->` must resolve the FQN return type across
/// PSR-4 file boundaries and offer Builder's methods.
#[tokio::test]
async fn test_namespaced_static_method_return_type_chain() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\Models\\": "src/Models/",
                    "Illuminate\\Database\\Eloquent\\": "src/Eloquent/"
                }
            }
        }"#,
        &[
            (
                "src/Eloquent/Builder.php",
                concat!(
                    "<?php\n",
                    "namespace Illuminate\\Database\\Eloquent;\n",
                    "class Builder {\n",
                    "    public function where(): static { return $this; }\n",
                    "    public function first(): mixed { return null; }\n",
                    "}\n",
                ),
            ),
            (
                "src/Models/Model.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Models;\n",
                    "abstract class Model {\n",
                    "    /** @return \\Illuminate\\Database\\Eloquent\\Builder */\n",
                    "    public static function query() {}\n",
                    "    public function save(): bool { return true; }\n",
                    "}\n",
                ),
            ),
            (
                "src/Models/LeadProvider.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Models;\n",
                    "final class LeadProvider extends Model {}\n",
                ),
            ),
        ],
    );

    // LeadProvider:: should include inherited `query` from Model
    let uri1 = Url::parse("file:///step1.php").unwrap();
    let text1 = concat!(
        "<?php\n",
        "use App\\Models\\LeadProvider;\n",
        "LeadProvider::\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri1.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text1.to_string(),
            },
        })
        .await;
    let result1 = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri1 },
                position: Position {
                    line: 2,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    let items1 = match result1 {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };
    let labels1: Vec<&str> = items1
        .iter()
        .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
        .collect();
    assert!(
        labels1.contains(&"query"),
        "LeadProvider:: should include inherited 'query'. Got: {:?}",
        labels1
    );

    // Model::query()-> should resolve the FQN return type to Builder
    let uri2 = Url::parse("file:///step2.php").unwrap();
    let text2 = concat!(
        "<?php\n",
        "use App\\Models\\Model;\n",
        "class Step2 {\n",
        "    function t() {\n",
        "        Model::query()->\n",
        "    }\n",
        "}\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri2.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text2.to_string(),
            },
        })
        .await;
    let result2 = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri2 },
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
    let items2 = match result2 {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };
    let labels2: Vec<&str> = items2
        .iter()
        .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
        .collect();
    assert!(
        labels2.contains(&"where"),
        "Model::query()-> should include Builder::where(). Got: {:?}",
        labels2
    );

    // The full chain: LeadProvider::query()-> through the parent's FQN return type
    let uri3 = Url::parse("file:///step3.php").unwrap();
    let text3 = concat!(
        "<?php\n",
        "use App\\Models\\LeadProvider;\n",
        "class Step3 {\n",
        "    function t() {\n",
        "        LeadProvider::query()->\n",
        "    }\n",
        "}\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri3.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text3.to_string(),
            },
        })
        .await;
    let result3 = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri3 },
                position: Position {
                    line: 4,
                    character: 31,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    let items3 = match result3 {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };
    let labels3: Vec<&str> = items3
        .iter()
        .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
        .collect();
    assert!(
        labels3.contains(&"where"),
        "LeadProvider::query()-> should include Builder::where(). Got: {:?}",
        labels3
    );
    assert!(
        labels3.contains(&"first"),
        "LeadProvider::query()-> should include Builder::first(). Got: {:?}",
        labels3
    );
}

// ─── Interface virtual member tests ─────────────────────────────────────────

#[tokio::test]
async fn test_interface_method_and_property_tags_visible_on_implementor() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///iface_virtual.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @property-read string $iguana\n",
        " * @method string jaguar()\n",
        " */\n",
        "interface Contract {}\n",
        "\n",
        "/**\n",
        " * @property string $gorilla\n",
        " * @method bool hyena(string $x)\n",
        " */\n",
        "class Zoo implements Contract {\n",
        "    public function demo(): void {\n",
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

    let completion_params = CompletionParams {
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
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();

            // @method and @property on own class docblock
            assert!(
                method_names.contains(&"hyena"),
                "Should include @method 'hyena' from own class docblock, got: {:?}",
                method_names
            );
            assert!(
                prop_names.contains(&"gorilla"),
                "Should include @property 'gorilla' from own class docblock, got: {:?}",
                prop_names
            );

            // @method and @property-read on implemented interface
            assert!(
                method_names.contains(&"jaguar"),
                "Should include @method 'jaguar' from implemented interface, got: {:?}",
                method_names
            );
            assert!(
                prop_names.contains(&"iguana"),
                "Should include @property-read 'iguana' from implemented interface, got: {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_interface_virtual_members_visible_on_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///iface_virtual_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @property-read string $iguana\n",
        " * @method string jaguar()\n",
        " */\n",
        "interface Contract {}\n",
        "\n",
        "/**\n",
        " * @property string $gorilla\n",
        " * @method bool hyena(string $x)\n",
        " */\n",
        "class Zoo implements Contract {\n",
        "    public string $baboon = '';\n",
        "    public function aardvark(): void {}\n",
        "}\n",
        "\n",
        "function demo(): void {\n",
        "    $zoo = new Zoo();\n",
        "    $zoo->\n",
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
                line: 18,
                character: 10,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();

            // Own real members
            assert!(
                method_names.contains(&"aardvark"),
                "Should include own method 'aardvark', got: {:?}",
                method_names
            );
            assert!(
                prop_names.contains(&"baboon"),
                "Should include own property 'baboon', got: {:?}",
                prop_names
            );

            // @method and @property on own class docblock
            assert!(
                method_names.contains(&"hyena"),
                "Should include @method 'hyena' from own class, got: {:?}",
                method_names
            );
            assert!(
                prop_names.contains(&"gorilla"),
                "Should include @property 'gorilla' from own class, got: {:?}",
                prop_names
            );

            // @method and @property-read on implemented interface
            assert!(
                method_names.contains(&"jaguar"),
                "Should include @method 'jaguar' from interface, got: {:?}",
                method_names
            );
            assert!(
                prop_names.contains(&"iguana"),
                "Should include @property-read 'iguana' from interface, got: {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_interface_virtual_members_visible_through_parent_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///iface_parent_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @property-read string $sensor\n",
        " * @method string scan()\n",
        " */\n",
        "interface Scannable {}\n",
        "\n",
        "class BaseDevice implements Scannable {\n",
        "    public function power(): void {}\n",
        "}\n",
        "\n",
        "class Scanner extends BaseDevice {\n",
        "    public function demo(): void {\n",
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

    let completion_params = CompletionParams {
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
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(i.label.as_str()))
                .collect();

            // Inherited real method from parent
            assert!(
                method_names.contains(&"power"),
                "Should include inherited 'power' from BaseDevice, got: {:?}",
                method_names
            );

            // @method from interface implemented by parent
            assert!(
                method_names.contains(&"scan"),
                "Should include @method 'scan' from interface on parent class, got: {:?}",
                method_names
            );

            // @property-read from interface implemented by parent
            assert!(
                prop_names.contains(&"sensor"),
                "Should include @property-read 'sensor' from interface on parent class, got: {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Deep inheritance chain through stubs ───────────────────────────────────

/// Methods inherited from `Exception` (like `getCode()`, `getMessage()`) should
/// be found on a class that extends through a multi-level chain where
/// intermediate classes live in stubs (e.g. PDOException → RuntimeException →
/// Exception).
#[tokio::test]
async fn test_completion_deep_inheritance_through_stubs() {
    let backend = create_exception_chain_backend();

    let uri = Url::parse("file:///deep_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class QueryException extends \\PDOException {\n",
        "    public function getSql(): string { return ''; }\n",
        "}\n",
        "class DeepChainTest {\n",
        "    public function handle(QueryException $e): void {\n",
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
        "Completion should return results for QueryException"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Own method
            assert!(
                method_names.contains(&"getSql"),
                "Should include own method 'getSql', got: {:?}",
                method_names
            );

            // Methods from Exception (3 levels up: QueryException → PDOException → RuntimeException → Exception)
            assert!(
                method_names.contains(&"getMessage"),
                "Should include 'getMessage' inherited from Exception through deep chain, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getCode"),
                "Should include 'getCode' inherited from Exception through deep chain, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getTrace"),
                "Should include 'getTrace' inherited from Exception through deep chain, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Static return type on static method called from subclass ───────────────

/// When a parent class declares `public static function first(): ?static`,
/// calling `ChildClass::first()` should resolve `static` to `ChildClass`,
/// not to the parent where `first()` is defined.
#[tokio::test]
async fn test_static_return_type_on_static_method_called_from_subclass() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///static_method_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Model {\n",
        "    /** @return ?static */\n",
        "    public static function first(): ?static { return null; }\n",
        "    public function save(): bool { return true; }\n",
        "}\n",
        "class AdminUser extends Model {\n",
        "    public function assignRole(string $role): void {}\n",
        "}\n",
        "class TestClass {\n",
        "    public function test() {\n",
        "        $admin = AdminUser::first();\n",
        "        $admin->\n",
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
                line: 12,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    let method_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap())
        .collect();

    assert!(
        !method_names.is_empty(),
        "Should resolve type of $admin from AdminUser::first() returning ?static. Got no methods."
    );
    assert!(
        method_names.contains(&"assignRole"),
        "static return should resolve to AdminUser, including own method assignRole. Got: {:?}",
        method_names
    );
    assert!(
        method_names.contains(&"save"),
        "Should include inherited save from Model. Got: {:?}",
        method_names
    );
}

/// Cross-file variant: `ChildClass::staticMethod()` where `staticMethod()`
/// returns `static` and is defined on a parent in a separate PSR-4 file.
#[tokio::test]
async fn test_static_return_type_on_static_method_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Model.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Model {\n",
                    "    /** @return ?static */\n",
                    "    public static function first(): ?static { return null; }\n",
                    "    public function save(): bool { return true; }\n",
                    "}\n",
                ),
            ),
            (
                "src/AdminUser.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class AdminUser extends Model {\n",
                    "    public function assignRole(string $role): void {}\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\AdminUser;\n",
        "class Seeder {\n",
        "    public function run() {\n",
        "        $admin = AdminUser::first();\n",
        "        $admin->\n",
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
    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    let method_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap())
        .collect();

    assert!(
        method_names.contains(&"assignRole"),
        "Cross-file static method return should resolve to AdminUser. Got: {:?}",
        method_names
    );
    assert!(
        method_names.contains(&"save"),
        "Should include inherited save cross-file. Got: {:?}",
        method_names
    );
}

/// Chained call after static method: `AdminUser::first()->save()` — the
/// intermediate `first()` should resolve `static` to `AdminUser`.
#[tokio::test]
async fn test_static_return_type_static_method_chained() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///static_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Model {\n",
        "    /** @return static */\n",
        "    public static function query(): static { return new static(); }\n",
        "    /** @return static */\n",
        "    public function where(string $col): static { return $this; }\n",
        "    public function get(): array { return []; }\n",
        "}\n",
        "class Product extends Model {\n",
        "    public function applyDiscount(): void {}\n",
        "}\n",
        "Product::query()->where('active')->\n",
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
                character: 37,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    let method_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap())
        .collect();

    assert!(
        method_names.contains(&"applyDiscount"),
        "Static chain should preserve Product type through query()->where(). Got: {:?}",
        method_names
    );
    assert!(
        method_names.contains(&"get"),
        "Should include inherited get. Got: {:?}",
        method_names
    );
}

// ─── Inherited docblock type propagation ────────────────────────────────────

#[tokio::test]
async fn test_hover_shows_inherited_interface_return_type() {
    // Hover on `getPens` in `$holder->getPens()` should show `list<Pen>`
    // even though the implementor only declares `: array`.
    let backend = create_test_backend();

    let uri = Url::parse("file:///hover_iface_ret.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Pen { public function write(): void {} }\n",      // 1
        "interface PenHolder {\n",                               // 2
        "    /** @return list<Pen> */\n",                        // 3
        "    public function getPens(): array;\n",               // 4
        "}\n",                                                   // 5
        "class Drawer implements PenHolder {\n",                 // 6
        "    public function getPens(): array { return []; }\n", // 7
        "}\n",                                                   // 8
        "class Consumer {\n",                                    // 9
        "    function demo(): void {\n",                         // 10
        "        $holder = new Drawer();\n",                     // 11
        "        $holder->getPens();\n",                         // 12
        "    }\n",                                               // 13
        "}\n",                                                   // 14
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

    // Hover on "getPens" at line 12: "        $holder->getPens();"
    //                                 0123456789012345678
    //                                          ^getPens starts at 18
    let hover_params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    let result = backend.hover(hover_params).await.unwrap();
    assert!(result.is_some(), "Hover should return a result");

    let hover = result.unwrap();
    if let HoverContents::Markup(markup) = hover.contents {
        let value = &markup.value;
        assert!(
            value.contains("list<Pen>"),
            "Hover on getPens() should show inherited return type 'list<Pen>'. Got:\n{}",
            value
        );
    } else {
        panic!("Expected HoverContents::Markup");
    }
}

#[tokio::test]
async fn test_hover_shows_inherited_parent_return_type() {
    // Hover on `getPens` in `$child->getPens()` should show `list<Pen>`
    // inherited from the parent class.
    let backend = create_test_backend();

    let uri = Url::parse("file:///hover_parent_ret.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Pen { public function write(): void {} }\n",      // 1
        "class BasePenHolder {\n",                               // 2
        "    /** @return list<Pen> */\n",                        // 3
        "    public function getPens(): array { return []; }\n", // 4
        "}\n",                                                   // 5
        "class ChildHolder extends BasePenHolder {\n",           // 6
        "    public function getPens(): array { return []; }\n", // 7
        "}\n",                                                   // 8
        "class Consumer {\n",                                    // 9
        "    function demo(): void {\n",                         // 10
        "        $child = new ChildHolder();\n",                 // 11
        "        $child->getPens();\n",                          // 12
        "    }\n",                                               // 13
        "}\n",                                                   // 14
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

    // Hover on "getPens" at line 12: "        $child->getPens();"
    //                                 0123456789012345678
    //                                         ^getPens starts at 17
    let hover_params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    let result = backend.hover(hover_params).await.unwrap();
    assert!(result.is_some(), "Hover should return a result");

    let hover = result.unwrap();
    if let HoverContents::Markup(markup) = hover.contents {
        let value = &markup.value;
        assert!(
            value.contains("list<Pen>"),
            "Hover on getPens() should show inherited return type 'list<Pen>'. Got:\n{}",
            value
        );
    } else {
        panic!("Expected HoverContents::Markup");
    }
}

#[tokio::test]
async fn test_hover_shows_inherited_param_type() {
    // Hover on `accept` in `$box->accept(...)` should show `list<Pen>` for
    // the parameter, inherited from the interface (with renamed param).
    let backend = create_test_backend();

    let uri = Url::parse("file:///hover_iface_param.php").unwrap();
    let text = concat!(
        "<?php\n",                                             // 0
        "class Pen { public function write(): void {} }\n",    // 1
        "interface PenAcceptor {\n",                           // 2
        "    /** @param list<Pen> $pens */\n",                 // 3
        "    public function accept(array $pens): void;\n",    // 4
        "}\n",                                                 // 5
        "class PenBox implements PenAcceptor {\n",             // 6
        "    public function accept(array $items): void {}\n", // 7
        "}\n",                                                 // 8
        "class Consumer {\n",                                  // 9
        "    function demo(): void {\n",                       // 10
        "        $box = new PenBox();\n",                      // 11
        "        $box->accept([]);\n",                         // 12
        "    }\n",                                             // 13
        "}\n",                                                 // 14
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

    // Hover on "accept" at line 12: "        $box->accept([]);"
    //                                 012345678901234
    //                                       ^accept starts at 15
    let hover_params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    let result = backend.hover(hover_params).await.unwrap();
    assert!(result.is_some(), "Hover should return a result");

    let hover = result.unwrap();
    if let HoverContents::Markup(markup) = hover.contents {
        let value = &markup.value;
        assert!(
            value.contains("list<Pen>"),
            "Hover on accept() should show inherited param type 'list<Pen>'. Got:\n{}",
            value
        );
    } else {
        panic!("Expected HoverContents::Markup");
    }
}

#[tokio::test]
async fn test_interface_docblock_return_type_propagates_to_implementor() {
    // Interface declares `@return list<Pen>`, implementor just has `: array`.
    // The enriched return type should show `list<Pen>` on the implementor's method.
    let backend = create_test_backend();

    let uri = Url::parse("file:///iface_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen { public function write(): void {} }\n",
        "interface PenHolder {\n",
        "    /** @return list<Pen> */\n",
        "    public function getPens(): array;\n",
        "}\n",
        "class Drawer implements PenHolder {\n",
        "    public function getPens(): array { return []; }\n",
        "    function test() {\n",
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
            let get_pens = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("getPens"))
                .expect("Should find getPens in completion");

            let detail = get_pens.detail.as_deref().unwrap_or("");
            assert!(
                detail.contains("list<Pen>"),
                "Interface @return list<Pen> should propagate to implementor's \
                 method detail, not just 'array'. Got: {:?}",
                detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_parent_docblock_return_type_propagates_to_child() {
    // Parent declares `@return list<Pen>`, child overrides with just `: array`.
    // The enriched return type should show `list<Pen>` on the child's method.
    let backend = create_test_backend();

    let uri = Url::parse("file:///parent_return.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen { public function write(): void {} }\n",
        "class BasePenHolder {\n",
        "    /** @return list<Pen> */\n",
        "    public function getPens(): array { return []; }\n",
        "}\n",
        "class ConcretePenHolder extends BasePenHolder {\n",
        "    public function getPens(): array { return [new Pen()]; }\n",
        "    function test() {\n",
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
            let get_pens = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("getPens"))
                .expect("Should find getPens in completion");

            let detail = get_pens.detail.as_deref().unwrap_or("");
            assert!(
                detail.contains("list<Pen>"),
                "Parent @return list<Pen> should propagate to child's \
                 method detail. Got: {:?}",
                detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_child_own_docblock_return_type_wins_over_parent() {
    // Both parent and child have docblock return types.
    // The child's own docblock should win.
    let backend = create_test_backend();

    let uri = Url::parse("file:///child_wins.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog { public function bark(): void {} }\n",
        "class Cat { public function meow(): void {} }\n",
        "class AnimalStore {\n",
        "    /** @return list<Dog> */\n",
        "    public function getAnimals(): array { return []; }\n",
        "}\n",
        "class CatStore extends AnimalStore {\n",
        "    /** @return list<Cat> */\n",
        "    public function getAnimals(): array { return []; }\n",
        "    function test() {\n",
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

    let completion_params = CompletionParams {
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
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let get_animals = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("getAnimals"))
                .expect("Should find getAnimals in completion");

            let detail = get_animals.detail.as_deref().unwrap_or("");
            assert!(
                detail.contains("list<Cat>"),
                "Child's own @return list<Cat> should win over parent's list<Dog>. Got: {:?}",
                detail
            );
            assert!(
                !detail.contains("Dog"),
                "Parent's Dog type should NOT leak through. Got: {:?}",
                detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_parent_docblock_param_type_propagates_to_child() {
    // Parent has `@param list<Pen> $pens`, child just has `: array $pens`.
    let backend = create_test_backend();

    let uri = Url::parse("file:///parent_param.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Pen { public function write(): void {} }\n",      // 1
        "class BasePenAcceptor {\n",                             // 2
        "    /** @param list<Pen> $pens */\n",                   // 3
        "    public function accept(array $pens): void {}\n",    // 4
        "}\n",                                                   // 5
        "class ConcretePenAcceptor extends BasePenAcceptor {\n", // 6
        "    public function accept(array $pens): void {\n",     // 7
        "        $pens[0]->\n",                                  // 8
        "    }\n",                                               // 9
        "}\n",                                                   // 10
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

    // "        $pens[0]->" — cursor at char 18
    // 012345678901234567
    //         $pens[0]->
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 18,
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
                method_names.contains(&"write"),
                "Parent @param list<Pen> should propagate to child, \
                 enabling Pen member completion. Got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_interface_docblock_return_type_with_generics() {
    // Interface with generics: `@implements Holder<FancyPen>` should
    // substitute the template param before propagating.
    let backend = create_test_backend();

    let uri = Url::parse("file:///iface_generic.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "class Pen { public function write(): void {} }\n",       // 1
        "class FancyPen extends Pen {\n",                         // 2
        "    public function engrave(): void {}\n",               // 3
        "}\n",                                                    // 4
        "/**\n",                                                  // 5
        " * @template T\n",                                       // 6
        " */\n",                                                  // 7
        "interface Holder {\n",                                   // 8
        "    /** @return list<T> */\n",                           // 9
        "    public function getItems(): array;\n",               // 10
        "}\n",                                                    // 11
        "/** @implements Holder<FancyPen> */\n",                  // 12
        "class FancyDrawer implements Holder {\n",                // 13
        "    public function getItems(): array { return []; }\n", // 14
        "    function test() {\n",                                // 15
        "        $this->\n",                                      // 16
        "    }\n",                                                // 17
        "}\n",                                                    // 18
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
                line: 16,
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
            let get_items = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("getItems"))
                .expect("Should find getItems in completion");

            let detail = get_items.detail.as_deref().unwrap_or("");
            assert!(
                detail.contains("list<FancyPen>"),
                "Generic @implements substitution should propagate through \
                 interface return type enrichment. Got: {:?}",
                detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_parent_docblock_return_type_propagates_cross_file() {
    // Cross-file: parent in a PSR-4 file has `@return list<Pen>`,
    // child in the open file just has `: array`.
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Pen.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Pen {\n",
                    "    public function write(): void {}\n",
                    "}\n",
                ),
            ),
            (
                "src/BasePenHolder.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class BasePenHolder {\n",
                    "    /** @return list<Pen> */\n",
                    "    public function getPens(): array { return []; }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "use App\\BasePenHolder;\n",                             // 1
        "class Drawer extends BasePenHolder {\n",                // 2
        "    public function getPens(): array { return []; }\n", // 3
        "    function test() {\n",                               // 4
        "        $this->\n",                                     // 5
        "    }\n",                                               // 6
        "}\n",                                                   // 7
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
            let get_pens = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("getPens"))
                .expect("Should find getPens in completion");

            let detail = get_pens.detail.as_deref().unwrap_or("");
            assert!(
                detail.contains("list<Pen>"),
                "Cross-file parent @return list<Pen> should propagate to child. Got: {:?}",
                detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_grandparent_docblock_return_type_propagates_through_chain() {
    // Grandparent has `@return list<Pen>`, parent has no docblock, child has no docblock.
    // The type should flow through the entire chain.
    let backend = create_test_backend();

    let uri = Url::parse("file:///grandparent_return.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Pen { public function write(): void {} }\n",      // 1
        "class GrandParent_ {\n",                                // 2
        "    /** @return list<Pen> */\n",                        // 3
        "    public function getPens(): array { return []; }\n", // 4
        "}\n",                                                   // 5
        "class Parent_ extends GrandParent_ {\n",                // 6
        "    public function getPens(): array { return []; }\n", // 7
        "}\n",                                                   // 8
        "class Child_ extends Parent_ {\n",                      // 9
        "    public function getPens(): array { return []; }\n", // 10
        "    function test() {\n",                               // 11
        "        $this->\n",                                     // 12
        "    }\n",                                               // 13
        "}\n",                                                   // 14
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
                line: 12,
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
            let get_pens = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("getPens"))
                .expect("Should find getPens in completion");

            let detail = get_pens.detail.as_deref().unwrap_or("");
            assert!(
                detail.contains("list<Pen>"),
                "Grandparent @return list<Pen> should propagate through \
                 the entire chain to the child. Got: {:?}",
                detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_interface_description_propagates_to_implementor() {
    // Interface has description, implementor has none.
    // Verify that the completion detail shows the enriched return type
    // (description propagation works at the MethodInfo level, but hover
    // may re-resolve from the local parse tree).
    let backend = create_test_backend();

    let uri = Url::parse("file:///iface_desc.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Pen { public function write(): void {} }\n",      // 1
        "interface Describable {\n",                             // 2
        "    /**\n",                                             // 3
        "     * Get the pens.\n",                                // 4
        "     * @return list<Pen> The pens in the holder\n",     // 5
        "     */\n",                                             // 6
        "    public function getPens(): array;\n",               // 7
        "}\n",                                                   // 8
        "class Person implements Describable {\n",               // 9
        "    public function getPens(): array { return []; }\n", // 10
        "    function test() {\n",                               // 11
        "        $this->\n",                                     // 12
        "    }\n",                                               // 13
        "}\n",                                                   // 14
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
                line: 12,
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
            let get_pens = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("getPens"))
                .expect("Should find getPens in completion");

            let detail = get_pens.detail.as_deref().unwrap_or("");
            assert!(
                detail.contains("list<Pen>"),
                "Interface return type and description should propagate. Got: {:?}",
                detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_parent_description_propagates_to_child() {
    // Parent has a richer return type, child has none.
    // Verify the enriched type flows through.
    let backend = create_test_backend();

    let uri = Url::parse("file:///parent_desc.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "class Pen { public function write(): void {} }\n",       // 1
        "class BaseRepo {\n",                                     // 2
        "    /**\n",                                              // 3
        "     * Find all pens.\n",                                // 4
        "     * @return list<Pen> The found pens\n",              // 5
        "     */\n",                                              // 6
        "    public function findPens(): array { return []; }\n", // 7
        "}\n",                                                    // 8
        "class UserRepo extends BaseRepo {\n",                    // 9
        "    public function findPens(): array { return []; }\n", // 10
        "    function test() {\n",                                // 11
        "        $this->\n",                                      // 12
        "    }\n",                                                // 13
        "}\n",                                                    // 14
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
                line: 12,
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
            let find_pens = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("findPens"))
                .expect("Should find findPens in completion");

            let detail = find_pens.detail.as_deref().unwrap_or("");
            assert!(
                detail.contains("list<Pen>"),
                "Parent @return list<Pen> should propagate to child. Got: {:?}",
                detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_no_return_type_at_all_inherits_from_interface() {
    // Child method has no return type hint at all (neither native nor docblock).
    // Interface has `@return list<Pen>`. Should still propagate.
    let backend = create_test_backend();

    let uri = Url::parse("file:///no_return.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Pen { public function write(): void {} }\n", // 1
        "interface PenHolder {\n",                          // 2
        "    /** @return list<Pen> */\n",                   // 3
        "    public function getPens(): array;\n",          // 4
        "}\n",                                              // 5
        "class Drawer implements PenHolder {\n",            // 6
        "    public function getPens() { return []; }\n",   // 7
        "    function test() {\n",                          // 8
        "        $this->\n",                                // 9
        "    }\n",                                          // 10
        "}\n",                                              // 11
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
            let get_pens = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("getPens"))
                .expect("Should find getPens in completion");

            let detail = get_pens.detail.as_deref().unwrap_or("");
            assert!(
                detail.contains("list<Pen>"),
                "When child has no return type at all, interface @return \
                 should still propagate. Got: {:?}",
                detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_return_type_detail_shows_enriched_type() {
    // Verify that the completion detail for the method shows the enriched
    // return type from the interface, not just `array`.
    let backend = create_test_backend();

    let uri = Url::parse("file:///detail_enriched.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen { public function write(): void {} }\n",
        "interface PenHolder {\n",
        "    /** @return list<Pen> */\n",
        "    public function getPens(): array;\n",
        "}\n",
        "class Drawer implements PenHolder {\n",
        "    public function getPens(): array { return []; }\n",
        "    function test() {\n",
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
            let get_pens = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("getPens"))
                .expect("Should find getPens in completion");

            let detail = get_pens.detail.as_deref().unwrap_or("");
            assert!(
                detail.contains("list<Pen>"),
                "Completion detail should show enriched return type 'list<Pen>', \
                 not just 'array'. Got: {:?}",
                detail
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_child_own_docblock_param_type_wins_over_interface() {
    // Both interface and implementor have @param types.
    // The child's own docblock should win.
    let backend = create_test_backend();

    let uri = Url::parse("file:///child_param_wins.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class Dog { public function bark(): void {} }\n",      // 1
        "class Cat { public function meow(): void {} }\n",      // 2
        "interface AnimalAcceptor {\n",                         // 3
        "    /** @param list<Dog> $animals */\n",               // 4
        "    public function accept(array $animals): void;\n",  // 5
        "}\n",                                                  // 6
        "class CatAcceptor implements AnimalAcceptor {\n",      // 7
        "    /** @param list<Cat> $animals */\n",               // 8
        "    public function accept(array $animals): void {\n", // 9
        "        $animals[0]->\n",                              // 10
        "    }\n",                                              // 11
        "}\n",                                                  // 12
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

    // "        $animals[0]->" — cursor at char 22
    // 0123456789012345678901
    //         $animals[0]->
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 22,
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
                method_names.contains(&"meow"),
                "Child's own @param list<Cat> should win over interface's list<Dog>. Got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"bark"),
                "Interface's Dog type should NOT leak through. Got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_inherited_interface_return_type_enables_array_element_completion() {
    // Interface declares `@return list<Pen>`, implementor has only `: array`.
    // `$holder->getPens()[0]->` should complete Pen members.
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_iface.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Pen { public function write(): void {} }\n",      // 1
        "interface PenHolder {\n",                               // 2
        "    /** @return list<Pen> */\n",                        // 3
        "    public function getPens(): array;\n",               // 4
        "}\n",                                                   // 5
        "class Drawer implements PenHolder {\n",                 // 6
        "    public function getPens(): array { return []; }\n", // 7
        "}\n",                                                   // 8
        "class Consumer {\n",                                    // 9
        "    function demo(): void {\n",                         // 10
        "        $d = new Drawer();\n",                          // 11
        "        $d->getPens()[0]->\n",                          // 12
        "    }\n",                                               // 13
        "}\n",                                                   // 14
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

    // "        $d->getPens()[0]->" cursor after final ->
    // 0         1         2
    // 0123456789012345678901234567
    //         $d->getPens()[0]->
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 26,
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
                method_names.contains(&"write"),
                "Interface @return list<Pen> should propagate to implementor, \
                 enabling Pen member completion on [0]->. Got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_inherited_parent_return_type_enables_array_element_completion() {
    // Parent declares `@return list<Pen>`, child overrides with just `: array`.
    // `$child->getPens()[0]->` should complete Pen members.
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_parent.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Pen { public function write(): void {} }\n",      // 1
        "class BasePenHolder {\n",                               // 2
        "    /** @return list<Pen> */\n",                        // 3
        "    public function getPens(): array { return []; }\n", // 4
        "}\n",                                                   // 5
        "class ChildHolder extends BasePenHolder {\n",           // 6
        "    public function getPens(): array { return []; }\n", // 7
        "}\n",                                                   // 8
        "class Consumer {\n",                                    // 9
        "    function demo(): void {\n",                         // 10
        "        $c = new ChildHolder();\n",                     // 11
        "        $c->getPens()[0]->\n",                          // 12
        "    }\n",                                               // 13
        "}\n",                                                   // 14
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

    // "        $c->getPens()[0]->" cursor after final ->
    // 0         1         2
    // 01234567890123456789012345
    //         $c->getPens()[0]->
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 26,
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
                method_names.contains(&"write"),
                "Parent @return list<Pen> should propagate to child, \
                 enabling Pen member completion on [0]->. Got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_inherited_grandparent_return_type_enables_array_element_completion() {
    // Grandparent has `@return list<Pen>`, middle and child have only `: array`.
    // `$deep->getPens()[0]->` should complete Pen members.
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_grandparent.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Pen { public function write(): void {} }\n",      // 1
        "class GrandBase {\n",                                   // 2
        "    /** @return list<Pen> */\n",                        // 3
        "    public function getPens(): array { return []; }\n", // 4
        "}\n",                                                   // 5
        "class Mid extends GrandBase {\n",                       // 6
        "    public function getPens(): array { return []; }\n", // 7
        "}\n",                                                   // 8
        "class Deep extends Mid {\n",                            // 9
        "    public function getPens(): array { return []; }\n", // 10
        "}\n",                                                   // 11
        "class Consumer {\n",                                    // 12
        "    function demo(): void {\n",                         // 13
        "        $d = new Deep();\n",                            // 14
        "        $d->getPens()[0]->\n",                          // 15
        "    }\n",                                               // 16
        "}\n",                                                   // 17
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

    // "        $d->getPens()[0]->" cursor after final ->
    // 0         1         2
    // 0123456789012345678901234567
    //         $d->getPens()[0]->
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 15,
                character: 26,
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
                method_names.contains(&"write"),
                "Grandparent @return list<Pen> should propagate through \
                 the entire chain, enabling Pen completion on [0]->. Got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}
