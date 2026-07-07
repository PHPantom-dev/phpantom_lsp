use crate::common::{create_psr4_workspace, create_test_backend};
use phpantom_lsp::Backend;
use std::collections::HashMap;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

static SPL_GENERIC_STUB: &str = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
interface Traversable {}

/**
 * @template TKey
 * @template TValue
 */
interface Iterator extends Traversable
{
    /** @return TValue */
    public function current(): mixed;
    /** @return TKey */
    public function key(): mixed;
    public function next(): void;
    public function rewind(): void;
    public function valid(): bool;
}

/**
 * @template TKey
 * @template TValue
 * @implements Iterator<TKey, TValue>
 */
class ArrayIterator implements Iterator
{
    /** @param array<TKey, TValue> $array */
    public function __construct(array $array = []) {}
    /** @return TValue */
    public function current(): mixed {}
    /** @return TKey */
    public function key(): mixed {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}

/**
 * @template TKey of array-key
 * @template TValue
 */
class ArrayObject
{
    /** @param array<TKey, TValue> $array */
    public function __construct(array $array = []) {}
    /** @return ArrayIterator<TKey, TValue> */
    public function getIterator(): ArrayIterator {}
}
"#;

fn create_spl_generic_backend() -> Backend {
    let mut class_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    class_stubs.insert("Traversable", SPL_GENERIC_STUB);
    class_stubs.insert("Iterator", SPL_GENERIC_STUB);
    class_stubs.insert("ArrayIterator", SPL_GENERIC_STUB);
    class_stubs.insert("ArrayObject", SPL_GENERIC_STUB);
    Backend::new_test_with_all_stubs(class_stubs, HashMap::new(), HashMap::new())
}

// ─── Generic type resolution tests ──────────────────────────────────────────
//
// These tests verify that `@template` parameters declared on a parent class
// are correctly substituted with concrete types when a child class uses
// `@extends Parent<ConcreteType1, ConcreteType2>`.

/// Basic test: a child class extends a generic parent with concrete types.
/// Methods inherited from the parent should have their template parameter
/// return types resolved to the concrete types.
#[tokio::test]
async fn test_generic_extends_resolves_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Box {\n",
        "    /** @return T */\n",
        "    public function get() {}\n",
        "    /** @return void */\n",
        "    public function set() {}\n",
        "}\n",
        "\n",
        "class Apple {\n",
        "    public function bite(): void {}\n",
        "    public function peel(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends Box<Apple>\n",
        " */\n",
        "class AppleBox extends Box {\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $box = new AppleBox();\n",
        "    $box->get()->\n",
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
                line: 24,
                character: 19,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"bite"),
                "Should resolve T to Apple and show Apple's 'bite' method, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"peel"),
                "Should resolve T to Apple and show Apple's 'peel' method, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test with two template parameters (like Collection<TKey, TValue>).
#[tokio::test]
async fn test_generic_extends_two_params_resolves() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_two_params.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "    /** @return TValue|null */\n",
        "    public function last() {}\n",
        "}\n",
        "\n",
        "class Language {\n",
        "    public int $priority;\n",
        "    public function getCode(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends Collection<int, Language>\n",
        " */\n",
        "class LanguageCollection extends Collection {\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $col = new LanguageCollection();\n",
        "    $col->first()->\n",
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
                line: 25,
                character: 21,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getCode"),
                "Should resolve TValue to Language and show 'getCode', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// A single generic argument against a two-parameter class with a
/// key-like leading bound binds to the trailing (value) parameter.
/// `@extends Collection<Language>` against `Collection<TKey of array-key,
/// TValue>` must yield `TValue => Language`, not `TKey => Language`.
#[tokio::test]
async fn test_generic_extends_short_args_right_align() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_right_align.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "}\n",
        "\n",
        "class Language {\n",
        "    public int $priority;\n",
        "    public function getCode(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends Collection<Language>\n",
        " */\n",
        "class LanguageCollection extends Collection {\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $col = new LanguageCollection();\n",
        "    $col->first()->\n",
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
                line: 23,
                character: 19,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getCode"),
                "Single arg should bind to TValue (Language) and show 'getCode', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that @template-covariant is also parsed correctly.
#[tokio::test]
async fn test_generic_template_covariant() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_covariant.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template-covariant TValue\n",
        " */\n",
        "class TypedList {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "}\n",
        "\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends TypedList<int, User>\n",
        " */\n",
        "class UserList extends TypedList {\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $list = new UserList();\n",
        "    $list->first()->\n",
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
                line: 23,
                character: 21,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve TValue (covariant) to User, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should resolve TValue (covariant) to User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that the child's own methods are still available alongside
/// inherited generic-resolved methods.
#[tokio::test]
async fn test_generic_child_own_methods_preserved() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_own_methods.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class GenericRepo {\n",
        "    /** @return T */\n",
        "    public function find() {}\n",
        "}\n",
        "\n",
        "class Product {\n",
        "    public function getPrice(): float {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends GenericRepo<Product>\n",
        " */\n",
        "class ProductRepo extends GenericRepo {\n",
        "    public function findByCategory(string $cat): void {}\n",
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
                line: 19,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            // Own method
            assert!(
                method_names.contains(&"findByCategory"),
                "Should include own method 'findByCategory', got: {:?}",
                method_names
            );

            // Inherited method with resolved generic
            assert!(
                method_names.contains(&"find"),
                "Should include inherited 'find' method, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that inherited generic method return type resolves in a chained call.
#[tokio::test]
async fn test_generic_method_return_type_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function getTotal(): float {}\n",
        "    public function getStatus(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Repository {\n",
        "    /** @return T */\n",
        "    public function findFirst() {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends Repository<Order>\n",
        " */\n",
        "class OrderRepository extends Repository {\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function getRepo(): OrderRepository {}\n",
        "    function test() {\n",
        "        $this->getRepo()->findFirst()->\n",
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
                line: 23,
                character: 42,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getTotal"),
                "Chain should resolve T→Order and show 'getTotal', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getStatus"),
                "Chain should resolve T→Order and show 'getStatus', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test nullable generic return type: `@return ?TValue` → `?Language`.
#[tokio::test]
async fn test_generic_nullable_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_nullable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Container {\n",
        "    /** @return ?T */\n",
        "    public function maybeGet() {}\n",
        "}\n",
        "\n",
        "class Widget {\n",
        "    public function render(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends Container<Widget>\n",
        " */\n",
        "class WidgetContainer extends Container {\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $c = new WidgetContainer();\n",
        "    $c->maybeGet()->\n",
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
                line: 21,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"render"),
                "Should resolve ?T to ?Widget and show 'render', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test property type substitution: inherited properties with template
/// types should be substituted.  Uses `$this->value->` inside the child
/// class, which is the supported property-chain resolution path.
#[tokio::test]
async fn test_generic_property_type_substitution() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_property.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Wrapper {\n",
        "    /** @var T */\n",
        "    public $value;\n",
        "}\n",
        "\n",
        "class Config {\n",
        "    public function get(string $key): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends Wrapper<Config>\n",
        " */\n",
        "class ConfigWrapper extends Wrapper {\n",
        "    function test() {\n",
        "        $this->value->\n",
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
                line: 18,
                character: 23,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"get"),
                "Should resolve property type T→Config and show 'get', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test generic resolution across files via PSR-4.
#[tokio::test]
async fn test_generic_extends_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let parent_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class GenericCollection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "    /** @return TValue|null */\n",
        "    public function last() {}\n",
        "    /** @return array<TKey, TValue> */\n",
        "    public function all() {}\n",
        "}\n",
    );

    let item_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Item {\n",
        "    public function getName(): string {}\n",
        "    public function getPrice(): float {}\n",
        "}\n",
    );

    let child_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use App\\GenericCollection;\n",
        "\n",
        "/**\n",
        " * @extends GenericCollection<int, Item>\n",
        " */\n",
        "class ItemCollection extends GenericCollection {\n",
        "    public function filterExpensive(): self {}\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("src/GenericCollection.php", parent_php),
            ("src/Item.php", item_php),
            ("src/ItemCollection.php", child_php),
        ],
    );

    // Open the file that uses ItemCollection
    let usage_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use App\\ItemCollection;\n",
        "\n",
        "function test() {\n",
        "    $items = new ItemCollection();\n",
        "    $items->first()->\n",
        "}\n",
    );

    let uri = Url::parse("file:///test_usage.php").unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: usage_text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 23,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Cross-file: should resolve TValue→Item and show 'getName', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getPrice"),
                "Cross-file: should resolve TValue→Item and show 'getPrice', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that non-template return types remain unchanged after substitution.
/// E.g. a method returning `void` or `self` should not be affected.
#[tokio::test]
async fn test_generic_non_template_return_types_unchanged() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_non_template.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class BaseList {\n",
        "    /** @return T */\n",
        "    public function first() {}\n",
        "    /** @return self */\n",
        "    public function filter(): self {}\n",
        "    /** @return int */\n",
        "    public function count(): int {}\n",
        "}\n",
        "\n",
        "class Task {\n",
        "    public function run(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends BaseList<Task>\n",
        " */\n",
        "class TaskList extends BaseList {\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $list = new TaskList();\n",
        "    $list->\n",
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
                line: 25,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"first"),
                "Should include 'first', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"filter"),
                "Should include 'filter' (returns self, not a template), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"count"),
                "Should include 'count' (returns int, not a template), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test chained generics: C extends B<Foo>, B extends A<T>.
/// A's methods with template param U should resolve to Foo for C.
#[tokio::test]
async fn test_generic_chained_inheritance() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_chained.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template U\n",
        " */\n",
        "class GrandParent_ {\n",
        "    /** @return U */\n",
        "    public function getItem() {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template T\n",
        " * @extends GrandParent_<T>\n",
        " */\n",
        "class Parent_ extends GrandParent_ {\n",
        "    /** @return T */\n",
        "    public function findItem() {}\n",
        "}\n",
        "\n",
        "class Car {\n",
        "    public function drive(): void {}\n",
        "    public function park(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends Parent_<Car>\n",
        " */\n",
        "class CarStore extends Parent_ {\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $store = new CarStore();\n",
        "    $store->findItem()->\n",
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

    // Test that Parent_::findItem() resolves T → Car
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 31,
                character: 27,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"drive"),
                "Should resolve T→Car on Parent_::findItem() and show 'drive', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that grandparent methods with template params also resolve
/// through the chain.
#[tokio::test]
async fn test_generic_grandparent_method_resolves() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_grandparent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template U\n",
        " */\n",
        "class BaseRepo {\n",
        "    /** @return U */\n",
        "    public function find() {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template T\n",
        " * @extends BaseRepo<T>\n",
        " */\n",
        "class CachingRepo extends BaseRepo {\n",
        "    public function clearCache(): void {}\n",
        "}\n",
        "\n",
        "class Invoice {\n",
        "    public function getPdf(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends CachingRepo<Invoice>\n",
        " */\n",
        "class InvoiceRepo extends CachingRepo {\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $repo = new InvoiceRepo();\n",
        "    $repo->find()->\n",
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
                line: 29,
                character: 20,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPdf"),
                "Grandparent: should resolve U→T→Invoice and show 'getPdf', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test @phpstan-extends variant is also recognized.
#[tokio::test]
async fn test_generic_phpstan_extends_variant() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_phpstan.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class GenericStack {\n",
        "    /** @return T */\n",
        "    public function pop() {}\n",
        "}\n",
        "\n",
        "class Message {\n",
        "    public function send(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @phpstan-extends GenericStack<Message>\n",
        " */\n",
        "class MessageStack extends GenericStack {\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $stack = new MessageStack();\n",
        "    $stack->pop()->\n",
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
                line: 21,
                character: 20,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"send"),
                "@phpstan-extends should resolve T→Message, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that when no @extends is present, template params remain unresolved
/// (no crash, methods still inherited, just without concrete types).
#[tokio::test]
async fn test_generic_without_extends_annotation_no_crash() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_no_extends.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class GenericParent {\n",
        "    /** @return T */\n",
        "    public function get() {}\n",
        "    public function size(): int {}\n",
        "}\n",
        "\n",
        // No @extends annotation — just plain extends
        "class PlainChild extends GenericParent {\n",
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
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            // Methods should still be inherited even without @extends generics
            assert!(
                method_names.contains(&"get"),
                "Should inherit 'get' even without @extends, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"size"),
                "Should inherit 'size' even without @extends, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Docblock parsing unit tests ────────────────────────────────────────────

/// Test that `extract_template_params` correctly parses various @template variants.
#[test]
fn test_extract_template_params_basic() {
    use phpantom_lsp::docblock::extract_template_params;

    let docblock = "/**\n * @template T\n */";
    assert_eq!(extract_template_params(docblock), vec!["T"]);
}

#[test]
fn test_extract_template_params_multiple() {
    use phpantom_lsp::docblock::extract_template_params;

    let docblock = "/**\n * @template TKey\n * @template TValue\n */";
    assert_eq!(extract_template_params(docblock), vec!["TKey", "TValue"]);
}

#[test]
fn test_extract_template_params_with_constraint() {
    use phpantom_lsp::docblock::extract_template_params;

    let docblock = "/**\n * @template TKey of array-key\n * @template TValue\n */";
    assert_eq!(extract_template_params(docblock), vec!["TKey", "TValue"]);
}

#[test]
fn test_extract_template_params_covariant() {
    use phpantom_lsp::docblock::extract_template_params;

    let docblock = "/**\n * @template TKey\n * @template-covariant TValue\n */";
    assert_eq!(extract_template_params(docblock), vec!["TKey", "TValue"]);
}

#[test]
fn test_extract_template_params_contravariant() {
    use phpantom_lsp::docblock::extract_template_params;

    let docblock = "/**\n * @template-contravariant TInput\n */";
    assert_eq!(extract_template_params(docblock), vec!["TInput"]);
}

#[test]
fn test_extract_template_params_phpstan_prefix() {
    use phpantom_lsp::docblock::extract_template_params;

    let docblock = "/**\n * @phpstan-template T\n */";
    assert_eq!(extract_template_params(docblock), vec!["T"]);
}

#[test]
fn test_extract_template_params_phpstan_covariant() {
    use phpantom_lsp::docblock::extract_template_params;

    let docblock = "/**\n * @phpstan-template-covariant TValue\n */";
    assert_eq!(extract_template_params(docblock), vec!["TValue"]);
}

#[test]
fn test_extract_template_params_empty() {
    use phpantom_lsp::docblock::extract_template_params;

    let docblock = "/**\n * @return void\n */";
    assert_eq!(extract_template_params(docblock), Vec::<String>::new());
}

/// Test that `extract_generics_tag` correctly parses @extends tags.
#[test]
fn test_extract_generics_tag_extends_basic() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @extends Collection<int, Language>\n */";
    let result = extract_generics_tag(docblock, "@extends");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "Collection");
    assert_eq!(
        result[0].1,
        vec![PhpType::parse("int"), PhpType::parse("Language")]
    );
}

#[test]
fn test_extract_generics_tag_extends_single_param() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @extends Box<Apple>\n */";
    let result = extract_generics_tag(docblock, "@extends");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "Box");
    assert_eq!(result[0].1, vec![PhpType::parse("Apple")]);
}

#[test]
fn test_extract_generics_tag_phpstan_extends() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @phpstan-extends Collection<int, User>\n */";
    let result = extract_generics_tag(docblock, "@extends");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "Collection");
    assert_eq!(
        result[0].1,
        vec![PhpType::parse("int"), PhpType::parse("User")]
    );
}

#[test]
fn test_extract_generics_tag_implements() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @implements ArrayAccess<string, User>\n */";
    let result = extract_generics_tag(docblock, "@implements");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "ArrayAccess");
    assert_eq!(
        result[0].1,
        vec![PhpType::parse("string"), PhpType::parse("User")]
    );
}

#[test]
fn test_extract_generics_tag_with_fqn() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @extends \\Illuminate\\Support\\Collection<int, \\App\\Model>\n */";
    let result = extract_generics_tag(docblock, "@extends");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "Illuminate\\Support\\Collection");
    assert_eq!(
        result[0].1,
        vec![PhpType::parse("int"), PhpType::parse("\\App\\Model")]
    );
}

#[test]
fn test_extract_generics_tag_nested_generic() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @extends Base<array<int, string>, User>\n */";
    let result = extract_generics_tag(docblock, "@extends");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "Base");
    assert_eq!(
        result[0].1,
        vec![PhpType::parse("array<int, string>"), PhpType::parse("User")]
    );
}

#[test]
fn test_extract_generics_tag_no_generics() {
    use phpantom_lsp::docblock::extract_generics_tag;

    let docblock = "/**\n * @return void\n */";
    let result = extract_generics_tag(docblock, "@extends");
    assert!(result.is_empty());
}

#[test]
fn test_extract_generics_tag_extends_without_angle_brackets() {
    use phpantom_lsp::docblock::extract_generics_tag;

    // @extends without generics should be ignored by extract_generics_tag
    let docblock = "/**\n * @extends SomeClass\n */";
    let result = extract_generics_tag(docblock, "@extends");
    assert!(result.is_empty());
}

#[test]
fn test_extract_generics_tag_multiple_implements() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = concat!(
        "/**\n",
        " * @implements ArrayAccess<int, User>\n",
        " * @implements Countable\n",
        " * @implements IteratorAggregate<int, User>\n",
        " */",
    );
    let result = extract_generics_tag(docblock, "@implements");
    // Only entries with generics are returned
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].0, "ArrayAccess");
    assert_eq!(
        result[0].1,
        vec![PhpType::parse("int"), PhpType::parse("User")]
    );
    assert_eq!(result[1].0, "IteratorAggregate");
    assert_eq!(
        result[1].1,
        vec![PhpType::parse("int"), PhpType::parse("User")]
    );
}

// ─── Method-level @template tests ───────────────────────────────────────────
//
// These tests verify that `@template` parameters declared on individual
// methods (not classes) are resolved from call-site arguments.
//
// The canonical pattern:
//   @template T
//   @param class-string<T> $class
//   @return T
// Calling `find(User::class)` should resolve the return type to `User`.

/// Unit test: `synthesize_template_conditional` creates a conditional
/// from a basic `@template T` + `@param class-string<T>` + `@return T`.
#[test]
fn test_synthesize_template_conditional_basic() {
    use phpantom_lsp::docblock::synthesize_template_conditional;
    use phpantom_lsp::php_type::PhpType;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param class-string<T> $class\n",
        " * @return T\n",
        " */",
    );
    let template_params = vec!["T".to_string()];
    let parsed = PhpType::parse("T");
    let result = synthesize_template_conditional(docblock, &template_params, Some(&parsed), false);
    assert!(
        result.is_some(),
        "Should synthesize a conditional for @template T with class-string<T>"
    );
}

/// Unit test: no synthesis when return type is not a template param.
#[test]
fn test_synthesize_template_conditional_non_template_return() {
    use phpantom_lsp::docblock::synthesize_template_conditional;
    use phpantom_lsp::php_type::PhpType;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param class-string<T> $class\n",
        " * @return string\n",
        " */",
    );
    let template_params = vec!["T".to_string()];
    let parsed = PhpType::parse("string");
    let result = synthesize_template_conditional(docblock, &template_params, Some(&parsed), false);
    assert!(
        result.is_none(),
        "Should NOT synthesize when return type is not a template param"
    );
}

/// Unit test: no synthesis when there are no template params.
#[test]
fn test_synthesize_template_conditional_no_templates() {
    use phpantom_lsp::docblock::synthesize_template_conditional;
    use phpantom_lsp::php_type::PhpType;

    let docblock = concat!(
        "/**\n",
        " * @param string $class\n",
        " * @return string\n",
        " */",
    );
    let template_params: Vec<String> = vec![];
    let parsed = PhpType::parse("string");
    let result = synthesize_template_conditional(docblock, &template_params, Some(&parsed), false);
    assert!(
        result.is_none(),
        "Should NOT synthesize when there are no template params"
    );
}

/// Unit test: no synthesis when an existing conditional is present.
#[test]
fn test_synthesize_template_conditional_existing_conditional() {
    use phpantom_lsp::docblock::synthesize_template_conditional;
    use phpantom_lsp::php_type::PhpType;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param class-string<T> $class\n",
        " * @return T\n",
        " */",
    );
    let template_params = vec!["T".to_string()];
    let parsed = PhpType::parse("T");
    let result = synthesize_template_conditional(docblock, &template_params, Some(&parsed), true);
    assert!(
        result.is_none(),
        "Should NOT synthesize when has_existing_conditional is true"
    );
}

/// Unit test: handles nullable return type `?T`.
#[test]
fn test_synthesize_template_conditional_nullable_return() {
    use phpantom_lsp::docblock::synthesize_template_conditional;
    use phpantom_lsp::php_type::PhpType;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param class-string<T> $class\n",
        " * @return ?T\n",
        " */",
    );
    let template_params = vec!["T".to_string()];
    let parsed = PhpType::parse("?T");
    let result = synthesize_template_conditional(docblock, &template_params, Some(&parsed), false);
    assert!(
        result.is_some(),
        "Should synthesize for nullable return type ?T"
    );
}

/// Unit test: no synthesis when no class-string param matches the template.
#[test]
fn test_synthesize_template_conditional_no_class_string_param() {
    use phpantom_lsp::docblock::synthesize_template_conditional;
    use phpantom_lsp::php_type::PhpType;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param string $class\n",
        " * @return T\n",
        " */",
    );
    let template_params = vec!["T".to_string()];
    let parsed = PhpType::parse("T");
    let result = synthesize_template_conditional(docblock, &template_params, Some(&parsed), false);
    assert!(
        result.is_none(),
        "Should NOT synthesize when no @param has class-string<T>"
    );
}

/// Unit test: handles nullable class-string param `?class-string<T>`.
#[test]
fn test_synthesize_template_conditional_nullable_class_string() {
    use phpantom_lsp::docblock::synthesize_template_conditional;
    use phpantom_lsp::php_type::PhpType;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param ?class-string<T> $class\n",
        " * @return T\n",
        " */",
    );
    let template_params = vec!["T".to_string()];
    let parsed = PhpType::parse("T");
    let result = synthesize_template_conditional(docblock, &template_params, Some(&parsed), false);
    assert!(
        result.is_some(),
        "Should synthesize for nullable class-string param ?class-string<T>"
    );
}

/// Unit test: handles class-string param with null union `class-string<T>|null`.
#[test]
fn test_synthesize_template_conditional_class_string_null_union() {
    use phpantom_lsp::docblock::synthesize_template_conditional;
    use phpantom_lsp::php_type::PhpType;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param class-string<T>|null $class\n",
        " * @return T\n",
        " */",
    );
    let template_params = vec!["T".to_string()];
    let parsed = PhpType::parse("T");
    let result = synthesize_template_conditional(docblock, &template_params, Some(&parsed), false);
    assert!(
        result.is_some(),
        "Should synthesize for class-string<T>|null union param"
    );
}

/// Integration test: method-level @template resolves in assignment context.
/// `$user = $repo->find(User::class)` should resolve $user to User.
#[tokio::test]
async fn test_method_template_assignment_resolves_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_template_assign.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class User {\n",                                       // 1
        "    public function getName(): string {}\n",           // 2
        "    public function getEmail(): string {}\n",          // 3
        "}\n",                                                  // 4
        "\n",                                                   // 5
        "class Repository {\n",                                 // 6
        "    /**\n",                                            // 7
        "     * @template T\n",                                 // 8
        "     * @param class-string<T> $class\n",               // 9
        "     * @return T\n",                                   // 10
        "     */\n",                                            // 11
        "    public function find(string $class): object {}\n", // 12
        "}\n",                                                  // 13
        "\n",                                                   // 14
        "function test() {\n",                                  // 15
        "    $repo = new Repository();\n",                      // 16
        "    $user = $repo->find(User::class);\n",              // 17
        "    $user->\n",                                        // 18
        "}\n",                                                  // 19
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve T to User and show 'getName', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should resolve T to User and show 'getEmail', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Integration test: method-level @template resolves in inline chain context.
/// `$repo->find(User::class)->` should show User's members directly.
#[tokio::test]
async fn test_method_template_inline_chain_resolves() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_template_chain.php").unwrap();
    let text = concat!(
        "<?php\n",                                                    // 0
        "class Product {\n",                                          // 1
        "    public function getPrice(): float {}\n",                 // 2
        "    public function getTitle(): string {}\n",                // 3
        "}\n",                                                        // 4
        "\n",                                                         // 5
        "class EntityManager {\n",                                    // 6
        "    /**\n",                                                  // 7
        "     * @template T\n",                                       // 8
        "     * @param class-string<T> $entityClass\n",               // 9
        "     * @return T\n",                                         // 10
        "     */\n",                                                  // 11
        "    public function find(string $entityClass): object {}\n", // 12
        "}\n",                                                        // 13
        "\n",                                                         // 14
        "function test(EntityManager $em) {\n",                       // 15
        "    $em->find(Product::class)->\n",                          // 16
        "}\n",                                                        // 17
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
                character: 35,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "Should resolve T to Product and show 'getPrice', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getTitle"),
                "Should resolve T to Product and show 'getTitle', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Integration test: method-level @template works on static methods.
/// `Repository::find(Order::class)->` should resolve to Order.
#[tokio::test]
async fn test_method_template_static_method_resolves() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_template_static.php").unwrap();
    let text = concat!(
        "<?php\n",                                                     // 0
        "class Order {\n",                                             // 1
        "    public function getTotal(): float {}\n",                  // 2
        "    public function getStatus(): string {}\n",                // 3
        "}\n",                                                         // 4
        "\n",                                                          // 5
        "class Repository {\n",                                        // 6
        "    /**\n",                                                   // 7
        "     * @template T\n",                                        // 8
        "     * @param class-string<T> $class\n",                      // 9
        "     * @return T\n",                                          // 10
        "     */\n",                                                   // 11
        "    public static function find(string $class): object {}\n", // 12
        "}\n",                                                         // 13
        "\n",                                                          // 14
        "function test() {\n",                                         // 15
        "    Repository::find(Order::class)->\n",                      // 16
        "}\n",                                                         // 17
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
                character: 39,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getTotal"),
                "Should resolve T to Order and show 'getTotal', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getStatus"),
                "Should resolve T to Order and show 'getStatus', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Integration test: method-level @template on a standalone function.
/// `resolve(Config::class)->` should resolve to Config.
#[tokio::test]
async fn test_function_template_resolves_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///function_template.php").unwrap();
    let text = concat!(
        "<?php\n",                                                     // 0
        "class Config {\n",                                            // 1
        "    public function get(string $key): mixed {}\n",            // 2
        "    public function set(string $key, mixed $val): void {}\n", // 3
        "}\n",                                                         // 4
        "\n",                                                          // 5
        "/**\n",                                                       // 6
        " * @template T\n",                                            // 7
        " * @param class-string<T> $class\n",                          // 8
        " * @return T\n",                                              // 9
        " */\n",                                                       // 10
        "function resolve(string $class): object {}\n",                // 11
        "\n",                                                          // 12
        "function test() {\n",                                         // 13
        "    resolve(Config::class)->\n",                              // 14
        "}\n",                                                         // 15
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
                character: 33,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"get"),
                "Should resolve T to Config and show 'get', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"set"),
                "Should resolve T to Config and show 'set', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Integration test: function-level @template with assignment.
/// `$config = resolve(Config::class); $config->` should show Config's members.
#[tokio::test]
async fn test_function_template_assignment_resolves() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///function_template_assign.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class Logger {\n",                                  // 1
        "    public function info(string $msg): void {}\n",  // 2
        "    public function error(string $msg): void {}\n", // 3
        "}\n",                                               // 4
        "\n",                                                // 5
        "/**\n",                                             // 6
        " * @template T\n",                                  // 7
        " * @param class-string<T> $abstract\n",             // 8
        " * @return T\n",                                    // 9
        " */\n",                                             // 10
        "function resolve(string $abstract): object {}\n",   // 11
        "\n",                                                // 12
        "function test() {\n",                               // 13
        "    $logger = resolve(Logger::class);\n",           // 14
        "    $logger->\n",                                   // 15
        "}\n",                                               // 16
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
                character: 13,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"info"),
                "Should resolve T to Logger and show 'info', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"error"),
                "Should resolve T to Logger and show 'error', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Integration test: method-level @template used inside $this-> context.
/// `$this->find(User::class)->` from within the same class.
#[tokio::test]
async fn test_method_template_this_context_resolves() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_template_this.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // 0
        "class Address {\n",                                // 1
        "    public function getCity(): string {}\n",       // 2
        "    public function getZip(): string {}\n",        // 3
        "}\n",                                              // 4
        "\n",                                               // 5
        "class Container {\n",                              // 6
        "    /**\n",                                        // 7
        "     * @template T\n",                             // 8
        "     * @param class-string<T> $id\n",              // 9
        "     * @return T\n",                               // 10
        "     */\n",                                        // 11
        "    public function get(string $id): object {}\n", // 12
        "\n",                                               // 13
        "    public function test() {\n",                   // 14
        "        $this->get(Address::class)->\n",           // 15
        "    }\n",                                          // 16
        "}\n",                                              // 17
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
                character: 40,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getCity"),
                "Should resolve T to Address and show 'getCity', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getZip"),
                "Should resolve T to Address and show 'getZip', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Integration test: method-level @template does NOT break methods
/// that have an explicit PHPStan conditional return type.
#[tokio::test]
async fn test_method_template_does_not_override_existing_conditional() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_template_existing.php").unwrap();
    let text = concat!(
        "<?php\n",                                                             // 0
        "class Session {\n",                                                   // 1
        "    public function getId(): string {}\n",                            // 2
        "}\n",                                                                 // 3
        "\n",                                                                  // 4
        "class App {\n",                                                       // 5
        "    /**\n",                                                           // 6
        "     * @template TClass\n",                                           // 7
        "     * @param class-string<TClass>|null $abstract\n",                 // 8
        "     * @return ($abstract is class-string<TClass> ? TClass : App)\n", // 9
        "     */\n",                                                           // 10
        "    public function make(?string $abstract = null): mixed {}\n",      // 11
        "}\n",                                                                 // 12
        "\n",                                                                  // 13
        "function test(App $app) {\n",                                         // 14
        "    $app->make(Session::class)->\n",                                  // 15
        "}\n",                                                                 // 16
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
                character: 35,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            // The explicit conditional should still work — Session is resolved.
            assert!(
                method_names.contains(&"getId"),
                "Explicit conditional should still resolve Session, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Integration test: @template with @phpstan-template variant.
#[tokio::test]
async fn test_method_phpstan_template_resolves() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_phpstan_template.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "class Invoice {\n",                                      // 1
        "    public function getAmount(): float {}\n",            // 2
        "}\n",                                                    // 3
        "\n",                                                     // 4
        "class Finder {\n",                                       // 5
        "    /**\n",                                              // 6
        "     * @phpstan-template T\n",                           // 7
        "     * @param class-string<T> $type\n",                  // 8
        "     * @return T\n",                                     // 9
        "     */\n",                                              // 10
        "    public function findOne(string $type): object {}\n", // 11
        "}\n",                                                    // 12
        "\n",                                                     // 13
        "function test(Finder $f) {\n",                           // 14
        "    $f->findOne(Invoice::class)->\n",                    // 15
        "}\n",                                                    // 16
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
                character: 35,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getAmount"),
                "@phpstan-template should resolve T to Invoice, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Integration test: @template with cross-file PSR-4 resolution.
/// The target class is defined in a separate file, loaded via PSR-4.
#[tokio::test]
async fn test_method_template_cross_file_resolves() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Payment.php",
            "<?php\nnamespace App;\nclass Payment {\n    public function charge(): void {}\n    public function refund(): void {}\n}\n",
        )],
    );

    let uri = Url::parse("file:///method_template_cross.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Payment;\n",
        "\n",
        "class ServiceLocator {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param class-string<T> $id\n",
        "     * @return T\n",
        "     */\n",
        "    public function get(string $id): object {}\n",
        "}\n",
        "\n",
        "function test(ServiceLocator $sl) {\n",
        "    $sl->get(Payment::class)->\n",
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
                character: 33,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"charge"),
                "Should resolve T to Payment cross-file and show 'charge', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"refund"),
                "Should resolve T to Payment cross-file and show 'refund', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Integration test: method-level @template with a different param name.
/// Uses `$entityClass` instead of `$class`.
#[tokio::test]
async fn test_method_template_different_param_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_template_param_name.php").unwrap();
    let text = concat!(
        "<?php\n",                                                    // 0
        "class Customer {\n",                                         // 1
        "    public function getLoyaltyPoints(): int {}\n",           // 2
        "}\n",                                                        // 3
        "\n",                                                         // 4
        "class ORM {\n",                                              // 5
        "    /**\n",                                                  // 6
        "     * @template TEntity\n",                                 // 7
        "     * @param class-string<TEntity> $entityClass\n",         // 8
        "     * @return TEntity\n",                                   // 9
        "     */\n",                                                  // 10
        "    public function find(string $entityClass): object {}\n", // 11
        "}\n",                                                        // 12
        "\n",                                                         // 13
        "function test(ORM $orm) {\n",                                // 14
        "    $orm->find(Customer::class)->\n",                        // 15
        "}\n",                                                        // 16
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
                character: 35,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getLoyaltyPoints"),
                "Should resolve TEntity to Customer and show 'getLoyaltyPoints', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Generic context preservation ──────────────────────────────────────────
//
// These tests verify that when a property or method return type carries
// generic parameters (e.g. `Collection<int, User>`), the generic context
// is preserved and applied so that further chaining resolves correctly.

/// Test: a property typed as `Collection<int, User>` via `@var` docblock
/// should resolve to `Collection` with `TValue → User`, so chaining
/// `->first()->` offers `User`'s methods.
#[tokio::test]
async fn test_generic_property_type_preserves_context() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generic_property_context.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "    /** @return TValue[] */\n",
        "    public function all() {}\n",
        "}\n",
        "\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "\n",
        "class UserRepository {\n",
        "    /** @var Collection<int, User> */\n",
        "    public $users;\n",
        "\n",
        "    function test() {\n",
        "        $this->users->first()->\n",
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
                line: 22,
                character: 37,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve Collection<int, User>::first() → User and show 'getName', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should resolve Collection<int, User>::first() → User and show 'getEmail', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: a method with `@return Collection<int, User>` preserves the
/// generic context so that chaining through the return type works.
#[tokio::test]
async fn test_generic_return_type_preserves_context() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generic_return_context.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "}\n",
        "\n",
        "class Product {\n",
        "    public function getPrice(): float {}\n",
        "    public function getSku(): string {}\n",
        "}\n",
        "\n",
        "class Catalog {\n",
        "    /** @return Collection<int, Product> */\n",
        "    public function getProducts() {}\n",
        "\n",
        "    function test() {\n",
        "        $this->getProducts()->first()->\n",
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
                line: 20,
                character: 45,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "Should resolve Collection<int, Product>::first() → Product and show 'getPrice', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getSku"),
                "Should resolve Collection<int, Product>::first() → Product and show 'getSku', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: a variable assigned from a method returning a generic type
/// preserves generic context for further chaining.
#[tokio::test]
async fn test_generic_context_through_variable_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generic_var_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Box {\n",
        "    /** @return T */\n",
        "    public function unwrap() {}\n",
        "}\n",
        "\n",
        "class Gift {\n",
        "    public function open(): string {}\n",
        "}\n",
        "\n",
        "class Store {\n",
        "    /** @return Box<Gift> */\n",
        "    public function getGiftBox() {}\n",
        "\n",
        "    function test() {\n",
        "        $box = $this->getGiftBox();\n",
        "        $box->unwrap()->\n",
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
                line: 19,
                character: 28,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"open"),
                "Should resolve Box<Gift>::unwrap() → Gift and show 'open', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: generic property type via `@var` with a single template param
/// resolves correctly.
#[tokio::test]
async fn test_generic_property_single_template_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generic_prop_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Container {\n",
        "    /** @return T */\n",
        "    public function get() {}\n",
        "}\n",
        "\n",
        "class Config {\n",
        "    public function has(string $key): bool {}\n",
        "    public function all(): array {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    /** @var Container<Config> */\n",
        "    public $config;\n",
        "\n",
        "    function test() {\n",
        "        $this->config->get()->\n",
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
                line: 19,
                character: 34,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"has"),
                "Should resolve Container<Config>::get() → Config and show 'has', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"all"),
                "Should resolve Container<Config>::get() → Config and show 'all', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: generic context preserved across PSR-4 file boundaries.
#[tokio::test]
async fn test_generic_property_context_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let collection_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "    /** @return static */\n",
        "    public function filter(callable $fn) {}\n",
        "}\n",
    );

    let user_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "    public function getAge(): int {}\n",
        "}\n",
    );

    let service_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class UserService {\n",
        "    /** @var Collection<int, User> */\n",
        "    public $users;\n",
        "\n",
        "    function test() {\n",
        "        $this->users->first()->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("src/Collection.php", collection_php),
            ("src/User.php", user_php),
            ("src/UserService.php", service_php),
        ],
    );

    let uri = Url::parse("file:///src/UserService.php").unwrap();
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
                line: 8,
                character: 37,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Cross-file: should resolve Collection<int, User>::first() → User, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getAge"),
                "Cross-file: should resolve Collection<int, User>::first() → User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: generic type with nullable union in `@var` (e.g. `Collection<int, User>|null`)
/// preserves generic context after stripping `|null`.
#[tokio::test]
async fn test_generic_nullable_union_preserves_context() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generic_nullable_union.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Optional {\n",
        "    /** @return T */\n",
        "    public function get() {}\n",
        "}\n",
        "\n",
        "class Session {\n",
        "    public function getId(): string {}\n",
        "}\n",
        "\n",
        "class Handler {\n",
        "    /** @var Optional<Session>|null */\n",
        "    public $session;\n",
        "\n",
        "    function test() {\n",
        "        $this->session->get()->\n",
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
                line: 18,
                character: 35,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getId"),
                "Should resolve Optional<Session>::get() → Session through nullable union, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: generic context works when the type has no template params
/// (plain class name in `@return` still works as before).
#[tokio::test]
async fn test_non_generic_return_type_still_works() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///non_generic_still_works.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "    public function error(string $msg): void {}\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    /** @return Logger */\n",
        "    public function getLogger() {}\n",
        "\n",
        "    function test() {\n",
        "        $this->getLogger()->\n",
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
                character: 32,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"info"),
                "Plain @return Logger should still work, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"error"),
                "Plain @return Logger should still work, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: generic context is preserved when `@extends` and inline generic
/// `@var` are both in play — they should not conflict.
#[tokio::test]
async fn test_generic_extends_and_inline_var_coexist() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generic_extends_and_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class BaseRepo {\n",
        "    /** @return T */\n",
        "    public function find(int $id) {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "}\n",
        "\n",
        "class Order {\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "\n",
        "class Item {\n",
        "    public function getQuantity(): int {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends BaseRepo<Order>\n",
        " */\n",
        "class OrderRepo extends BaseRepo {\n",
        "    /** @var Collection<int, Item> */\n",
        "    public $lineItems;\n",
        "\n",
        "    function testFind() {\n",
        "        $this->find(1)->\n",
        "    }\n",
        "\n",
        "    function testLineItems() {\n",
        "        $this->lineItems->first()->\n",
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

    // Test 1: @extends substitution still works (find() → Order)
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 34,
                character: 29,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for find()"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getTotal"),
                "@extends BaseRepo<Order>: find() should return Order, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }

    // Test 2: inline @var generic also works (lineItems->first() → Item)
    let completion_params2 = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 38,
                character: 42,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result2 = backend.completion(completion_params2).await.unwrap();
    assert!(
        result2.is_some(),
        "Completion should return results for lineItems->first()"
    );

    match result2.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getQuantity"),
                "@var Collection<int, Item>: first() should return Item, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: directly accessing members on a property typed as a generic class
/// (e.g. `$this->collection->`) still offers the collection's own methods.
#[tokio::test]
async fn test_generic_property_own_methods_still_visible() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generic_prop_own_methods.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "    public function count(): int {}\n",
        "    public function isEmpty(): bool {}\n",
        "}\n",
        "\n",
        "class Tag {\n",
        "    public function getLabel(): string {}\n",
        "}\n",
        "\n",
        "class Article {\n",
        "    /** @var Collection<int, Tag> */\n",
        "    public $tags;\n",
        "\n",
        "    function test() {\n",
        "        $this->tags->\n",
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
                line: 21,
                character: 21,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"first"),
                "Collection's own 'first' method should be visible, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"count"),
                "Collection's own 'count' method should be visible, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"isEmpty"),
                "Collection's own 'isEmpty' method should be visible, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: `$box = $this->giftBox` where the property has `@var Box<Gift>`
/// should resolve `$box` to `Box<Gift>`, so `$box->unwrap()->` offers
/// `Gift`'s methods.
#[tokio::test]
async fn test_generic_property_assignment_to_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generic_prop_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Box {\n",
        "    /** @return T */\n",
        "    public function unwrap() {}\n",
        "}\n",
        "\n",
        "class Gift {\n",
        "    public function open(): string {}\n",
        "    public function getTag(): string {}\n",
        "}\n",
        "\n",
        "class GiftShop {\n",
        "    /** @var Box<Gift> */\n",
        "    public $giftBox;\n",
        "\n",
        "    function test() {\n",
        "        $box = $this->giftBox;\n",
        "        $box->unwrap()->\n",
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
                line: 20,
                character: 28,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"open"),
                "Should resolve $box = $this->giftBox (Box<Gift>), unwrap() → Gift, show 'open', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getTag"),
                "Should resolve $box = $this->giftBox (Box<Gift>), unwrap() → Gift, show 'getTag', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Trait generic substitution (@use) tests ────────────────────────────────
//
// These tests verify that `@template` parameters declared on a trait are
// correctly substituted with concrete types when a class uses
// `@use TraitName<ConcreteType>` in its docblock.

/// Basic test: a class uses a generic trait with a concrete type.
/// Methods inherited from the trait should have their template parameter
/// return types resolved to the concrete types.
#[tokio::test]
async fn test_trait_use_generic_resolves_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_use_generic_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TFactory\n",
        " */\n",
        "trait HasFactory {\n",
        "    /** @return TFactory */\n",
        "    public static function factory() {}\n",
        "}\n",
        "\n",
        "class UserFactory {\n",
        "    public function create(): void {}\n",
        "    public function count(int $n): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @use HasFactory<UserFactory>\n",
        " */\n",
        "class User {\n",
        "    use HasFactory;\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    User::factory()->\n",
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
                line: 22,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"create"),
                "Should resolve TFactory to UserFactory and show 'create', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"count"),
                "Should resolve TFactory to UserFactory and show 'count', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test with two template parameters on a trait.
#[tokio::test]
async fn test_trait_use_generic_two_params() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_use_generic_two_params.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "trait Indexable {\n",
        "    /** @return TValue */\n",
        "    public function get() {}\n",
        "    /** @return TKey */\n",
        "    public function key() {}\n",
        "}\n",
        "\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @use Indexable<int, User>\n",
        " */\n",
        "class UserList {\n",
        "    use Indexable;\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $list = new UserList();\n",
        "    $list->get()->\n",
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
                line: 25,
                character: 19,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve TValue to User and show User's 'getName', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that trait property types are also substituted via @use generics.
#[tokio::test]
async fn test_trait_use_generic_property_substitution() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_use_generic_property.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TModel\n",
        " */\n",
        "trait HasRelation {\n",
        "    /** @var TModel */\n",
        "    public $related;\n",
        "}\n",
        "\n",
        "class Address {\n",
        "    public function getCity(): string {}\n",
        "    public function getZip(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @use HasRelation<Address>\n",
        " */\n",
        "class User {\n",
        "    use HasRelation;\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $user = new User();\n",
        "    $user->related->\n",
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
                line: 23,
                character: 21,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getCity"),
                "Should resolve TModel to Address and show 'getCity', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getZip"),
                "Should resolve TModel to Address and show 'getZip', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that @phpstan-use variant is also accepted.
#[tokio::test]
async fn test_trait_phpstan_use_variant() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_phpstan_use.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "trait Wrapper {\n",
        "    /** @return T */\n",
        "    public function unwrap() {}\n",
        "}\n",
        "\n",
        "class Gift {\n",
        "    public function open(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @phpstan-use Wrapper<Gift>\n",
        " */\n",
        "class GiftBox {\n",
        "    use Wrapper;\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $box = new GiftBox();\n",
        "    $box->unwrap()->\n",
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
                line: 22,
                character: 20,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"open"),
                "Should resolve T to Gift via @phpstan-use and show 'open', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that class own members take precedence over substituted trait members.
#[tokio::test]
async fn test_trait_use_generic_class_own_wins() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_use_generic_precedence.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "trait Getter {\n",
        "    /** @return T */\n",
        "    public function get() {}\n",
        "    /** @return T */\n",
        "    public function first() {}\n",
        "}\n",
        "\n",
        "class Apple {\n",
        "    public function bite(): void {}\n",
        "}\n",
        "\n",
        "class Orange {\n",
        "    public function squeeze(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @use Getter<Apple>\n",
        " */\n",
        "class FruitBowl {\n",
        "    use Getter;\n",
        "    /** @return Orange */\n",
        "    public function get() {}\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $bowl = new FruitBowl();\n",
        "    $bowl->get()->\n",
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
                line: 30,
                character: 19,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            // Class own get() returns Orange, so we should see Orange's methods
            assert!(
                method_names.contains(&"squeeze"),
                "Class own 'get' should override trait's, returning Orange with 'squeeze', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"bite"),
                "Apple's 'bite' should NOT appear since class own 'get' returns Orange, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test trait generic substitution works across files via PSR-4.
#[tokio::test]
async fn test_trait_use_generic_cross_file_psr4() {
    let composer_json = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            (
                "src/Concerns/HasFactory.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Concerns;\n",
                    "\n",
                    "/**\n",
                    " * @template TFactory\n",
                    " */\n",
                    "trait HasFactory {\n",
                    "    /** @return TFactory */\n",
                    "    public static function factory() {}\n",
                    "}\n",
                ),
            ),
            (
                "src/Factories/UserFactory.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Factories;\n",
                    "\n",
                    "class UserFactory {\n",
                    "    public function create(): void {}\n",
                    "    public function make(): void {}\n",
                    "}\n",
                ),
            ),
            (
                "src/Models/User.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Models;\n",
                    "\n",
                    "use App\\Concerns\\HasFactory;\n",
                    "use App\\Factories\\UserFactory;\n",
                    "\n",
                    "/**\n",
                    " * @use HasFactory<UserFactory>\n",
                    " */\n",
                    "class User {\n",
                    "    use HasFactory;\n",
                    "}\n",
                ),
            ),
        ],
    );

    // Open a file that uses User::factory()
    let uri = Url::parse("file:///test_trait_cross_file.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Models\\User;\n",
        "\n",
        "function test() {\n",
        "    User::factory()->\n",
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"create"),
                "Should resolve TFactory to UserFactory across files and show 'create', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"make"),
                "Should resolve TFactory to UserFactory across files and show 'make', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that without @use generics, template params remain unresolved
/// (no crash, methods still available but return types are raw template names).
#[tokio::test]
async fn test_trait_use_without_generics_no_crash() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_no_use_generics.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "trait Wrapper {\n",
        "    /** @return T */\n",
        "    public function unwrap() {}\n",
        "    public function isEmpty(): bool {}\n",
        "}\n",
        "\n",
        "class Box {\n",
        "    use Wrapper;\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $box = new Box();\n",
        "    $box->\n",
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
                line: 16,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            // Methods from the trait should still be visible even without @use generics
            assert!(
                method_names.contains(&"unwrap"),
                "Trait method 'unwrap' should be visible even without @use generics, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"isEmpty"),
                "Trait method 'isEmpty' should be visible, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test @use generics with $this-> context inside the class.
#[tokio::test]
async fn test_trait_use_generic_this_context() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_use_generic_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TFactory\n",
        " */\n",
        "trait HasFactory {\n",
        "    /** @return TFactory */\n",
        "    public function getFactory() {}\n",
        "}\n",
        "\n",
        "class UserFactory {\n",
        "    public function create(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @use HasFactory<UserFactory>\n",
        " */\n",
        "class User {\n",
        "    use HasFactory;\n",
        "\n",
        "    public function test() {\n",
        "        $this->getFactory()->\n",
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
                line: 20,
                character: 33,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"create"),
                "Should resolve TFactory to UserFactory via $this-> and show 'create', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that `extract_generics_tag` correctly parses @use tags.
#[test]
fn test_extract_generics_tag_use_basic() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @use HasFactory<UserFactory>\n */";
    let result = extract_generics_tag(docblock, "@use");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "HasFactory");
    assert_eq!(result[0].1, vec![PhpType::parse("UserFactory")]);
}

/// Test that @phpstan-use variant is parsed by extract_generics_tag.
#[test]
fn test_extract_generics_tag_phpstan_use() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @phpstan-use HasFactory<UserFactory>\n */";
    let result = extract_generics_tag(docblock, "@use");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "HasFactory");
    assert_eq!(result[0].1, vec![PhpType::parse("UserFactory")]);
}

/// Test @use with multiple template parameters.
#[test]
fn test_extract_generics_tag_use_multiple_params() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @use Indexable<int, User>\n */";
    let result = extract_generics_tag(docblock, "@use");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "Indexable");
    assert_eq!(
        result[0].1,
        vec![PhpType::parse("int"), PhpType::parse("User")]
    );
}

/// Test @use with fully-qualified trait name.
#[test]
fn test_extract_generics_tag_use_fqn() {
    use phpantom_lsp::docblock::extract_generics_tag;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @use \\App\\Concerns\\HasFactory<\\App\\Factories\\UserFactory>\n */";
    let result = extract_generics_tag(docblock, "@use");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, "App\\Concerns\\HasFactory");
    assert_eq!(
        result[0].1,
        vec![PhpType::parse("\\App\\Factories\\UserFactory")]
    );
}

// ─── Method-level @template general case tests ──────────────────────────────
//
// These tests verify that method-level @template parameters are correctly
// substituted with concrete types resolved from call-site arguments, beyond
// the special-cased `class-string<T>` pattern.

// ── Unit tests for extract_template_param_bindings ──────────────────────────

/// Basic binding: `@template T` + `@param T $model` → `[("T", "$model")]`.
#[test]
fn test_extract_template_param_bindings_basic() {
    use phpantom_lsp::docblock::extract_template_param_bindings;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param T $model\n",
        " * @return Collection<T>\n",
        " */",
    );
    let tpl_params = vec!["T".to_string()];
    let result = extract_template_param_bindings(docblock, &tpl_params);
    assert_eq!(result, vec![("T".to_string(), "$model".to_string())]);
}

/// Nullable param type: `@param ?T $model` should still bind T.
#[test]
fn test_extract_template_param_bindings_nullable() {
    use phpantom_lsp::docblock::extract_template_param_bindings;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param ?T $model\n",
        " * @return T\n",
        " */",
    );
    let tpl_params = vec!["T".to_string()];
    let result = extract_template_param_bindings(docblock, &tpl_params);
    assert_eq!(result, vec![("T".to_string(), "$model".to_string())]);
}

/// Union-with-null param type: `@param T|null $model` should bind T.
#[test]
fn test_extract_template_param_bindings_union_null() {
    use phpantom_lsp::docblock::extract_template_param_bindings;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param T|null $model\n",
        " * @return T\n",
        " */",
    );
    let tpl_params = vec!["T".to_string()];
    let result = extract_template_param_bindings(docblock, &tpl_params);
    assert_eq!(result, vec![("T".to_string(), "$model".to_string())]);
}

/// Multiple template params with separate bindings.
#[test]
fn test_extract_template_param_bindings_multiple() {
    use phpantom_lsp::docblock::extract_template_param_bindings;

    let docblock = concat!(
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " * @param TKey $key\n",
        " * @param TValue $value\n",
        " * @return Pair<TKey, TValue>\n",
        " */",
    );
    let tpl_params = vec!["TKey".to_string(), "TValue".to_string()];
    let result = extract_template_param_bindings(docblock, &tpl_params);
    assert_eq!(
        result,
        vec![
            ("TKey".to_string(), "$key".to_string()),
            ("TValue".to_string(), "$value".to_string()),
        ]
    );
}

/// Non-template param type is not bound.
#[test]
fn test_extract_template_param_bindings_non_template_ignored() {
    use phpantom_lsp::docblock::extract_template_param_bindings;

    let docblock = concat!(
        "/**\n",
        " * @template T\n",
        " * @param string $name\n",
        " * @param T $model\n",
        " * @return T\n",
        " */",
    );
    let tpl_params = vec!["T".to_string()];
    let result = extract_template_param_bindings(docblock, &tpl_params);
    assert_eq!(result, vec![("T".to_string(), "$model".to_string())]);
}

/// Empty template params → no bindings.
#[test]
fn test_extract_template_param_bindings_empty_templates() {
    use phpantom_lsp::docblock::extract_template_param_bindings;

    let docblock = concat!("/**\n", " * @param string $name\n", " */",);
    let tpl_params: Vec<String> = vec![];
    let result = extract_template_param_bindings(docblock, &tpl_params);
    assert!(result.is_empty());
}

/// Multiple template params in a single `@param` generic wrapper:
/// `@param array<TKey, TValue> $value` → `[("TKey", "$value"), ("TValue", "$value")]`.
#[test]
fn test_extract_template_param_bindings_multi_param_generic() {
    use phpantom_lsp::docblock::extract_template_param_bindings;

    let docblock = concat!(
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TValue\n",
        " * @param array<TKey, TValue> $value\n",
        " * @return Collection<TKey, TValue>\n",
        " */",
    );
    let tpl_params = vec!["TKey".to_string(), "TValue".to_string()];
    let result = extract_template_param_bindings(docblock, &tpl_params);
    assert_eq!(
        result,
        vec![
            ("TKey".to_string(), "$value".to_string()),
            ("TValue".to_string(), "$value".to_string()),
        ]
    );
}

// ── Integration tests: function-level @template with generic return type ─────
//
// These test the `collect()` pattern where function-level `@template` params
// appear inside a generic return type (`@return Collection<TKey, TValue>`),
// not as the bare return type (`@return T`).  Two variants:
//   1. Inline chain: `collect($users)->` resolves to `Collection<User>`.
//   2. Assignment: `$collection = collect($users); $collection->` preserves
//      the generic substitution through variable assignment.

/// Inline chain: `collect($users)->` shows Collection's members.
#[tokio::test]
async fn test_function_template_collect_inline_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///function_template_collect.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class User {\n",                                       // 1
        "    public function getName(): string {}\n",           // 2
        "}\n",                                                  // 3
        "\n",                                                   // 4
        "/**\n",                                                // 5
        " * @template TKey of array-key\n",                     // 6
        " * @template TValue\n",                                // 7
        " */\n",                                                // 8
        "class Collection {\n",                                 // 9
        "    /** @return TValue */\n",                          // 10
        "    public function first(): mixed {}\n",              // 11
        "    public function count(): int {}\n",                // 12
        "}\n",                                                  // 13
        "\n",                                                   // 14
        "/**\n",                                                // 15
        " * @template TKey of array-key\n",                     // 16
        " * @template TValue\n",                                // 17
        " * @param array<TKey, TValue> $value\n",               // 18
        " * @return Collection<TKey, TValue>\n",                // 19
        " */\n",                                                // 20
        "function collect(array $value = []): Collection {}\n", // 21
        "\n",                                                   // 22
        "function test() {\n",                                  // 23
        "    /** @var User[] $users */\n",                      // 24
        "    $users = [];\n",                                   // 25
        "    collect($users)->\n",                              // 26
        "}\n",                                                  // 27
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

    // cursor right after `->` on line 26
    // "    collect($users)->" = 4+7+1+6+1+2 = 21 characters
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 26,
                character: 21,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for collect($users)->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"first"),
                "Should show Collection's 'first' method on collect($users)->, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"count"),
                "Should show Collection's 'count' method on collect($users)->, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Assignment preserves generics: `$collection = collect($users); $collection->`
/// should show Collection's members, proving the variable type preserves
/// generic args from function-level @template through assignment.
#[tokio::test]
async fn test_function_template_collect_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///function_template_collect_assign.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class User {\n",                                       // 1
        "    public function getName(): string {}\n",           // 2
        "}\n",                                                  // 3
        "\n",                                                   // 4
        "/**\n",                                                // 5
        " * @template TKey of array-key\n",                     // 6
        " * @template TValue\n",                                // 7
        " */\n",                                                // 8
        "class Collection {\n",                                 // 9
        "    /** @return TValue */\n",                          // 10
        "    public function first(): mixed {}\n",              // 11
        "    public function count(): int {}\n",                // 12
        "}\n",                                                  // 13
        "\n",                                                   // 14
        "/**\n",                                                // 15
        " * @template TKey of array-key\n",                     // 16
        " * @template TValue\n",                                // 17
        " * @param array<TKey, TValue> $value\n",               // 18
        " * @return Collection<TKey, TValue>\n",                // 19
        " */\n",                                                // 20
        "function collect(array $value = []): Collection {}\n", // 21
        "\n",                                                   // 22
        "function test() {\n",                                  // 23
        "    /** @var User[] $users */\n",                      // 24
        "    $users = [];\n",                                   // 25
        "    $collection = collect($users);\n",                 // 26
        "    $collection->\n",                                  // 27
        "}\n",                                                  // 28
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

    // cursor right after `->` on line 27
    // "    $collection->" = 4+11+2 = 17 characters
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 27,
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
        "Completion should return results for $collection-> after collect($users)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"first"),
                "Should show Collection's 'first' method on $collection->, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"count"),
                "Should show Collection's 'count' method on $collection->, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Deep chain: `collect($users)->first()->` should resolve TValue→User
/// through function-level @template substitution and show User's methods.
#[tokio::test]
async fn test_function_template_collect_deep_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///function_template_collect_deep.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class User {\n",                                       // 1
        "    public function getName(): string {}\n",           // 2
        "    public function getEmail(): string {}\n",          // 3
        "}\n",                                                  // 4
        "\n",                                                   // 5
        "/**\n",                                                // 6
        " * @template TKey of array-key\n",                     // 7
        " * @template TValue\n",                                // 8
        " */\n",                                                // 9
        "class Collection {\n",                                 // 10
        "    /** @return TValue */\n",                          // 11
        "    public function first(): mixed {}\n",              // 12
        "    public function count(): int {}\n",                // 13
        "}\n",                                                  // 14
        "\n",                                                   // 12
        "/**\n",                                                // 16
        " * @template TKey of array-key\n",                     // 17
        " * @template TValue\n",                                // 18
        " * @param array<TKey, TValue> $value\n",               // 19
        " * @return Collection<TKey, TValue>\n",                // 20
        " */\n",                                                // 21
        "function collect(array $value = []): Collection {}\n", // 22
        "\n",                                                   // 23
        "function test() {\n",                                  // 24
        "    /** @var User[] $users */\n",                      // 25
        "    $users = [];\n",                                   // 26
        "    collect($users)->first()->\n",                     // 27
        "}\n",                                                  // 28
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

    // cursor right after `->` on line 27
    // "    collect($users)->first()->" = 4+7+1+6+1+2+5+2+2 = 30 characters
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 27,
                character: 30,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for collect($users)->first()->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve TValue→User through collect() + first() chain, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should resolve TValue→User through collect() + first() chain, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Real Laravel `collect()` signature: the `@param` is a union type
/// `Arrayable<TKey, TValue>|iterable<TKey, TValue>|null`.
/// `classify_template_binding` must split the union at depth 0 before
/// matching generic wrappers, and `iterable` must be recognised as an
/// array-like wrapper so positional extraction works.
#[tokio::test]
async fn test_function_template_collect_laravel_union_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///function_template_collect_laravel.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                  // 0
        "class User {\n",                                                           // 1
        "    public function getName(): string {}\n",                               // 2
        "    public function getEmail(): string {}\n",                              // 3
        "}\n",                                                                      // 4
        "\n",                                                                       // 5
        "/**\n",                                                                    // 6
        " * @template TKey of array-key\n",                                         // 7
        " * @template TValue\n",                                                    // 8
        " */\n",                                                                    // 9
        "class Collection {\n",                                                     // 10
        "    /** @return TValue */\n",                                              // 11
        "    public function first(): mixed {}\n",                                  // 12
        "    public function count(): int {}\n",                                    // 13
        "}\n",                                                                      // 14
        "\n",                                                                       // 15
        "/**\n",                                                                    // 16
        " * @template TKey of array-key\n",                                         // 17
        " * @template TValue\n",                                                    // 18
        " * @param  Arrayable<TKey, TValue>|iterable<TKey, TValue>|null  $value\n", // 19
        " * @return Collection<TKey, TValue>\n",                                    // 20
        " */\n",                                                                    // 21
        "function collect($value = []): Collection {}\n",                           // 22
        "\n",                                                                       // 23
        "function test() {\n",                                                      // 24
        "    /** @var User[] $users */\n",                                          // 25
        "    $users = [];\n",                                                       // 26
        "    collect($users)->first()->\n",                                         // 27
        "}\n",                                                                      // 28
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

    // cursor right after `->` on line 27
    // "    collect($users)->first()->" = 30 characters
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 27,
                character: 30,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for collect($users)->first()-> with Laravel union @param"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve TValue→User through collect() with union @param + first(), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should resolve TValue→User through collect() with union @param + first(), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ── Integration tests: inline chain with general @template ──────────────────

/// `@template T` + `@param T $model` + `@return Collection<T>`:
/// `$repo->wrap($user)->` should show Collection<User> methods.
#[tokio::test]
async fn test_method_template_general_inline_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_inline.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "\n",
        "/** @template TValue */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first(): mixed {}\n",
        "}\n",
        "\n",
        "class Repository {\n",
        "    /**\n",
        "     * @template T of object\n",
        "     * @param T $model\n",
        "     * @return Collection<T>\n",
        "     */\n",
        "    public function wrap(object $model): Collection {}\n",
        "}\n",
        "\n",
        "function test(Repository $repo, User $user) {\n",
        "    $repo->wrap($user)->first()->\n",
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
                line: 22,
                character: 35,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve T→User via Collection<T>.first()→User, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should resolve T→User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `$this->wrap($user)->` inside a class — template resolves via $this context.
#[tokio::test]
async fn test_method_template_general_this_context() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function getPrice(): float {}\n",
        "    public function getTitle(): string {}\n",
        "}\n",
        "\n",
        "/** @template TValue */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first(): mixed {}\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param T $item\n",
        "     * @return Collection<T>\n",
        "     */\n",
        "    public function collect(mixed $item): Collection {}\n",
        "\n",
        "    public function run(Product $product) {\n",
        "        $this->collect($product)->first()->\n",
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
                line: 21,
                character: 46,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "Should resolve T→Product, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getTitle"),
                "Should resolve T→Product, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Assignment case: `$collection = $repo->wrap($user); $collection->first()->`
/// should resolve through the template.
#[tokio::test]
async fn test_method_template_general_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "/** @template TValue */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first(): mixed {}\n",
        "}\n",
        "\n",
        "class Repository {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param T $model\n",
        "     * @return Collection<T>\n",
        "     */\n",
        "    public function wrap(object $model): Collection {}\n",
        "}\n",
        "\n",
        "function test(Repository $repo, User $user) {\n",
        "    $collection = $repo->wrap($user);\n",
        "    $collection->first()->\n",
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
                line: 22,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve T→User through assignment, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Static method with general @template: `Repository::wrap($user)->`
#[tokio::test]
async fn test_method_template_general_static_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "/** @template TValue */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first(): mixed {}\n",
        "}\n",
        "\n",
        "class Repository {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param T $model\n",
        "     * @return Collection<T>\n",
        "     */\n",
        "    public static function wrap(object $model): Collection {}\n",
        "}\n",
        "\n",
        "function test(User $user) {\n",
        "    Repository::wrap($user)->first()->\n",
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
                line: 21,
                character: 39,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Static method @template T + @param T should resolve T→User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Direct return of template param: `@template T` + `@param T $model` + `@return T`
/// `$repo->identity($user)->` should resolve to User.
#[tokio::test]
async fn test_method_template_general_direct_return() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_direct.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "class Util {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param T $item\n",
        "     * @return T\n",
        "     */\n",
        "    public function identity(mixed $item): mixed {}\n",
        "}\n",
        "\n",
        "function test(Util $util, User $user) {\n",
        "    $util->identity($user)->\n",
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
                character: 29,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Direct @return T with @param T should resolve T→User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Multiple template params: `@template TKey` + `@template TValue`
/// with separate param bindings.
#[tokio::test]
async fn test_method_template_general_multiple_params() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Category {\n",
        "    public function getLabel(): string {}\n",
        "}\n",
        "\n",
        "class Product {\n",
        "    public function getPrice(): float {}\n",
        "}\n",
        "\n",
        "/** @template TValue */\n",
        "class Wrapper {\n",
        "    /** @return TValue */\n",
        "    public function unwrap(): mixed {}\n",
        "}\n",
        "\n",
        "class Factory {\n",
        "    /**\n",
        "     * @template TKey\n",
        "     * @template TValue\n",
        "     * @param TKey $key\n",
        "     * @param TValue $value\n",
        "     * @return Wrapper<TValue>\n",
        "     */\n",
        "    public function make(mixed $key, mixed $value): Wrapper {}\n",
        "}\n",
        "\n",
        "function test(Factory $f, Category $cat, Product $prod) {\n",
        "    $f->make($cat, $prod)->unwrap()->\n",
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
                line: 27,
                character: 38,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "TValue should resolve to Product (second arg), got: {:?}",
                method_names
            );
            // Category methods should NOT appear because the return uses TValue not TKey.
            assert!(
                !method_names.contains(&"getLabel"),
                "TKey (Category) should not leak into TValue resolution, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Existing class-string<T> conditional should still work (regression guard).
/// This verifies the general template path doesn't break the existing
/// synthesized conditional path.
#[tokio::test]
async fn test_method_template_general_does_not_break_class_string() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_regression.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "class Repository {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param class-string<T> $class\n",
        "     * @return T\n",
        "     */\n",
        "    public function find(string $class): object {}\n",
        "}\n",
        "\n",
        "function test(Repository $repo) {\n",
        "    $repo->find(User::class)->\n",
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
                character: 30,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "class-string<T> path should still work, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `self::class` passed to a `class-string<T>` template parameter should
/// resolve T to the enclosing class, not the class that owns the method.
#[tokio::test]
async fn test_method_template_self_class_resolves_to_enclosing_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///self_class_tpl.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class OrmService {\n",
        "    /**\n",
        "     * @template T of AbstractEntity\n",
        "     * @param class-string<T> $class\n",
        "     * @return T[]\n",
        "     */\n",
        "    public function getByQuery(string $class, string $query): array {}\n",
        "}\n",
        "class AbstractEntity {}\n",
        "class Category extends AbstractEntity {\n",
        "    public function isVisible(): bool {}\n",
        "    public function getChildren(): array {\n",
        "        $children = (new OrmService())->getByQuery(self::class, 'SELECT 1');\n",
        "        foreach ($children as $child) {\n",
        "            $child->\n",
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

    let line = text.lines().position(|l| l.contains("$child->")).unwrap() as u32;
    let col = text
        .lines()
        .nth(line as usize)
        .unwrap()
        .find("$child->")
        .unwrap() as u32
        + 8;
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line,
                character: col,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let result = backend.completion(completion_params).await.unwrap();
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"isVisible"),
                "self::class should resolve T to Category, got methods: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// $this->method($arg) inside a class body via assignment.
/// Verifies the AST-based resolve_rhs_expression path.
#[tokio::test]
async fn test_method_template_general_this_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_this_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "/** @template TValue */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first(): mixed {}\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param T $item\n",
        "     * @return Collection<T>\n",
        "     */\n",
        "    public function wrap(mixed $item): Collection {}\n",
        "\n",
        "    public function run(User $user) {\n",
        "        $result = $this->wrap($user);\n",
        "        $result->first()->\n",
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
                line: 21,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "$this->wrap($user) assignment should resolve T→User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file PSR-4: method-level @template resolves across files.
/// Tests `$repo->wrap($user)->` resolves to Collection<User> and shows
/// Collection's own methods (like `first`).
#[tokio::test]
async fn test_method_template_general_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let user_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "    public function getEmail(): string {}\n",
        "}\n",
    );

    let coll_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "/** @template TValue */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first(): mixed {}\n",
        "    public function count(): int {}\n",
        "}\n",
    );

    let repo_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Repository {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param T $model\n",
        "     * @return Collection<T>\n",
        "     */\n",
        "    public function wrap(object $model): Collection {}\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("src/User.php", user_php),
            ("src/Collection.php", coll_php),
            ("src/Repository.php", repo_php),
        ],
    );

    // Open a consumer file that uses the template method.
    // Test the simpler chain `$repo->wrap($user)->` which should
    // resolve to Collection<User> and show Collection's methods.
    let test_text = concat!(
        "<?php\n",                                                      // 0
        "namespace App;\n",                                             // 1
        "\n",                                                           // 2
        "use App\\Repository;\n",                                       // 3
        "use App\\User;\n",                                             // 4
        "\n",                                                           // 5
        "class TestConsumer {\n",                                       // 6
        "    public function handle(Repository $repo, User $user) {\n", // 7
        "        $repo->wrap($user)->\n",                               // 8
        "    }\n",                                                      // 9
        "}\n",                                                          // 10
    );

    let uri = Url::parse("file:///test_cross_tpl.php").unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: test_text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
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
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"first"),
                "Cross-file @template T + @param T should resolve to Collection<User> showing 'first', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"count"),
                "Cross-file should also show Collection's 'count', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Method-level template with `new ClassName()` as argument:
/// `$util->wrap(new User())->` should resolve T to User.
#[tokio::test]
async fn test_method_template_general_new_expression_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_new_arg.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "class Util {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param T $item\n",
        "     * @return T\n",
        "     */\n",
        "    public function identity(mixed $item): mixed {}\n",
        "}\n",
        "\n",
        "function test(Util $util) {\n",
        "    $util->identity(new User())->\n",
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
                character: 34,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "`new User()` arg should resolve T→User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Method with @phpstan-template variant (common in PHPStan stubs).
#[tokio::test]
async fn test_method_template_general_phpstan_prefix() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_phpstan.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "class Util {\n",
        "    /**\n",
        "     * @phpstan-template T\n",
        "     * @param T $item\n",
        "     * @return T\n",
        "     */\n",
        "    public function identity(mixed $item): mixed {}\n",
        "}\n",
        "\n",
        "function test(Util $util, User $user) {\n",
        "    $util->identity($user)->\n",
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
                character: 29,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "@phpstan-template variant should resolve T→User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Non-template method should still resolve normally (no regression).
#[tokio::test]
async fn test_method_template_general_non_template_unaffected() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_no_tpl.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function getUser(): User {}\n",
        "}\n",
        "\n",
        "function test(Service $svc) {\n",
        "    $svc->getUser()->\n",
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
                line: 10,
                character: 21,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Non-template method should still work, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Argument with `$this->property` resolves through template.
#[tokio::test]
async fn test_method_template_general_this_property_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_general_prop_arg.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "class Util {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param T $item\n",
        "     * @return T\n",
        "     */\n",
        "    public function identity(mixed $item): mixed {}\n",
        "}\n",
        "\n",
        "class Controller {\n",
        "    private User $user;\n",
        "\n",
        "    public function handle(Util $util) {\n",
        "        $util->identity($this->user)->\n",
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
                line: 18,
                character: 39,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "$this->property arg should resolve T→User, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// 3-level chain with `new ClassName()` argument through template:
/// `$mapper->wrap(new Product())->first()->` should resolve to Product.
#[tokio::test]
async fn test_method_template_general_deep_chain_new_arg() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_deep_chain.php").unwrap();
    let text = concat!(
        "<?php\n",                                                      // 0
        "class Product {\n",                                            // 1
        "    public function getPrice(): float {}\n",                   // 2
        "}\n",                                                          // 3
        "\n",                                                           // 4
        "/** @template TValue */\n",                                    // 5
        "class TypedCollection {\n",                                    // 6
        "    /** @return TValue */\n",                                  // 7
        "    public function first(): mixed {}\n",                      // 8
        "}\n",                                                          // 9
        "\n",                                                           // 10
        "class ObjectMapper {\n",                                       // 11
        "    /**\n",                                                    // 12
        "     * @template T\n",                                         // 13
        "     * @param T $item\n",                                      // 14
        "     * @return TypedCollection<T>\n",                          // 15
        "     */\n",                                                    // 16
        "    public function wrap(object $item): TypedCollection {}\n", // 17
        "}\n",                                                          // 18
        "\n",                                                           // 19
        "function test() {\n",                                          // 20
        "    $mapper = new ObjectMapper();\n",                          // 21
        "    $mapper->wrap(new Product())->first()->\n",                // 22
        "}\n",                                                          // 23
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
                line: 22,
                character: 47,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "3-level chain: wrap(new Product())->first()-> should resolve to Product, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same 3-level chain but inside a namespace (reproduces example.php failure).
/// The namespace causes class names to be FQ-resolved, which can break
/// the substitution path if short names don't match after resolution.
#[tokio::test]
async fn test_method_template_general_deep_chain_namespaced() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_deep_chain_ns.php").unwrap();
    let text = concat!(
        "<?php\n",                                                      // 0
        "namespace App;\n",                                             // 1
        "\n",                                                           // 2
        "class Product {\n",                                            // 3
        "    public function getPrice(): float {}\n",                   // 4
        "}\n",                                                          // 5
        "\n",                                                           // 6
        "/** @template TValue */\n",                                    // 7
        "class TypedCollection {\n",                                    // 8
        "    /** @return TValue */\n",                                  // 9
        "    public function first(): mixed {}\n",                      // 10
        "}\n",                                                          // 11
        "\n",                                                           // 12
        "class ObjectMapper {\n",                                       // 13
        "    /**\n",                                                    // 14
        "     * @template T\n",                                         // 15
        "     * @param T $item\n",                                      // 16
        "     * @return TypedCollection<T>\n",                          // 17
        "     */\n",                                                    // 18
        "    public function wrap(object $item): TypedCollection {}\n", // 19
        "}\n",                                                          // 20
        "\n",                                                           // 21
        "function test() {\n",                                          // 22
        "    $mapper = new ObjectMapper();\n",                          // 23
        "    $mapper->wrap(new Product())->first()->\n",                // 24
        "}\n",                                                          // 25
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
                line: 24,
                character: 47,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "Namespaced 3-level chain: wrap(new Product())->first()-> should resolve to Product, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a variable is assigned a `::class` value through a `match` expression
/// and then passed to a method with `@template T` + `@param class-string<T>`
/// + `@return T`, the resolver should trace the class-string back through the
/// match arms and produce a union of all possible return types.
#[tokio::test]
async fn test_match_class_string_forwarded_to_method_conditional_return() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_class_string_method.php").unwrap();
    let text = concat!(
        "<?php\n",                                                      // 0
        "class GetCreditnotesRequest {\n",                              // 1
        "    public function getCreditnotes(): array {}\n",             // 2
        "}\n",                                                          // 3
        "\n",                                                           // 4
        "class GetOrdersRequest {\n",                                   // 5
        "    public function getOrders(): array {}\n",                  // 6
        "}\n",                                                          // 7
        "\n",                                                           // 8
        "class Container {\n",                                          // 9
        "    /**\n",                                                    // 10
        "     * @template T\n",                                         // 11
        "     * @param class-string<T> $abstract\n",                    // 12
        "     * @return T\n",                                           // 13
        "     */\n",                                                    // 14
        "    public function make(string $abstract): object {}\n",      // 15
        "}\n",                                                          // 16
        "\n",                                                           // 17
        "class App {\n",                                                // 18
        "    public function run(string $typeName): void {\n",          // 19
        "        $container = new Container();\n",                      // 20
        "        $requestType = match ($typeName) {\n",                 // 21
        "            'creditnotes' => GetCreditnotesRequest::class,\n", // 22
        "            'orders'      => GetOrdersRequest::class,\n",      // 23
        "        };\n",                                                 // 24
        "        $requestBody = $container->make($requestType);\n",     // 25
        "        $requestBody->\n",                                     // 26
        "    }\n",                                                      // 27
        "}\n",                                                          // 28
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

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 26,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $requestBody->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getCreditnotes"),
                "Should include getCreditnotes from GetCreditnotesRequest, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getOrders"),
                "Should include getOrders from GetOrdersRequest, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same as above but using a standalone function instead of a method.
/// `$result = resolve($matchVar)` where `resolve` has `@template T` +
/// `@param class-string<T>` + `@return T`.
#[tokio::test]
async fn test_match_class_string_forwarded_to_function_conditional_return() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_class_string_func.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // 0
        "class Alpha {\n",                                 // 1
        "    public function alphaMethod(): void {}\n",    // 2
        "}\n",                                             // 3
        "\n",                                              // 4
        "class Beta {\n",                                  // 5
        "    public function betaMethod(): void {}\n",     // 6
        "}\n",                                             // 7
        "\n",                                              // 8
        "/**\n",                                           // 9
        " * @template T\n",                                // 10
        " * @param class-string<T> $class\n",              // 11
        " * @return T\n",                                  // 12
        " */\n",                                           // 13
        "function resolve(string $class): object {}\n",    // 14
        "\n",                                              // 15
        "class Service {\n",                               // 16
        "    public function handle(int $type): void {\n", // 17
        "        $cls = match ($type) {\n",                // 18
        "            1 => Alpha::class,\n",                // 19
        "            2 => Beta::class,\n",                 // 20
        "        };\n",                                    // 21
        "        $obj = resolve($cls);\n",                 // 22
        "        $obj->\n",                                // 23
        "    }\n",                                         // 24
        "}\n",                                             // 25
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

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 23,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $obj->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"alphaMethod"),
                "Should include alphaMethod from Alpha, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"betaMethod"),
                "Should include betaMethod from Beta, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// A simple `$cls = User::class` (not a match) forwarded to a method with
/// `@template T` + `@param class-string<T>` + `@return T` should also work.
#[tokio::test]
async fn test_simple_class_string_variable_forwarded_to_conditional_return() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///simple_class_string_var.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class User {\n",                                       // 1
        "    public function getName(): string {}\n",           // 2
        "}\n",                                                  // 3
        "\n",                                                   // 4
        "class Repository {\n",                                 // 5
        "    /**\n",                                            // 6
        "     * @template T\n",                                 // 7
        "     * @param class-string<T> $class\n",               // 8
        "     * @return T\n",                                   // 9
        "     */\n",                                            // 10
        "    public function find(string $class): object {}\n", // 11
        "}\n",                                                  // 12
        "\n",                                                   // 13
        "class Service {\n",                                    // 14
        "    public function handle(): void {\n",               // 15
        "        $repo = new Repository();\n",                  // 16
        "        $cls = User::class;\n",                        // 17
        "        $user = $repo->find($cls);\n",                 // 18
        "        $user->\n",                                    // 19
        "    }\n",                                              // 20
        "}\n",                                                  // 21
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

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
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
    assert!(
        result.is_some(),
        "Completion should return results for $user->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve $cls = User::class through variable and show getName, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Ternary expression assigning class-string values to a variable, then
/// forwarded to a conditional return type method.
#[tokio::test]
async fn test_ternary_class_string_forwarded_to_conditional_return() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///ternary_class_string.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "class Admin {\n",                                          // 1
        "    public function getRole(): string {}\n",               // 2
        "}\n",                                                      // 3
        "\n",                                                       // 4
        "class Guest {\n",                                          // 5
        "    public function getToken(): string {}\n",              // 6
        "}\n",                                                      // 7
        "\n",                                                       // 8
        "class Container {\n",                                      // 9
        "    /**\n",                                                // 10
        "     * @template T\n",                                     // 11
        "     * @param class-string<T> $abstract\n",                // 12
        "     * @return T\n",                                       // 13
        "     */\n",                                                // 14
        "    public function make(string $abstract): object {}\n",  // 15
        "}\n",                                                      // 16
        "\n",                                                       // 17
        "class Handler {\n",                                        // 18
        "    public function handle(bool $isAdmin): void {\n",      // 19
        "        $container = new Container();\n",                  // 20
        "        $cls = $isAdmin ? Admin::class : Guest::class;\n", // 21
        "        $user = $container->make($cls);\n",                // 22
        "        $user->\n",                                        // 23
        "    }\n",                                                  // 24
        "}\n",                                                      // 25
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

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 23,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $user->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getRole"),
                "Should include getRole from Admin (ternary true branch), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getToken"),
                "Should include getToken from Guest (ternary false branch), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Inline chain: `$container->make($matchVar)->` should also resolve when
/// the argument is a variable holding class-string values from a match.
#[tokio::test]
async fn test_match_class_string_inline_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_class_string_inline.php").unwrap();
    let text = concat!(
        "<?php\n",                                                 // 0
        "class Foo {\n",                                           // 1
        "    public function fooMethod(): void {}\n",              // 2
        "}\n",                                                     // 3
        "\n",                                                      // 4
        "class Bar {\n",                                           // 5
        "    public function barMethod(): void {}\n",              // 6
        "}\n",                                                     // 7
        "\n",                                                      // 8
        "class Container {\n",                                     // 9
        "    /**\n",                                               // 10
        "     * @template T\n",                                    // 11
        "     * @param class-string<T> $abstract\n",               // 12
        "     * @return T\n",                                      // 13
        "     */\n",                                               // 14
        "    public function make(string $abstract): object {}\n", // 15
        "}\n",                                                     // 16
        "\n",                                                      // 17
        "class Runner {\n",                                        // 18
        "    public function run(int $which): void {\n",           // 19
        "        $container = new Container();\n",                 // 20
        "        $cls = match ($which) {\n",                       // 21
        "            1 => Foo::class,\n",                          // 22
        "            2 => Bar::class,\n",                          // 23
        "        };\n",                                            // 24
        "        $container->make($cls)->\n",                      // 25
        "    }\n",                                                 // 26
        "}\n",                                                     // 27
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

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 25,
                character: 38,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for inline chain $container->make($cls)->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"fooMethod"),
                "Should include fooMethod from Foo via inline chain, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"barMethod"),
                "Should include barMethod from Bar via inline chain, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Static method call: `ClassName::create($matchVar)` where `create` has a
/// `@template T` + `@param class-string<T>` + `@return T` conditional.
#[tokio::test]
async fn test_match_class_string_forwarded_to_static_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_class_string_static.php").unwrap();
    let text = concat!(
        "<?php\n",                                                       // 0
        "class Widget {\n",                                              // 1
        "    public function render(): string {}\n",                     // 2
        "}\n",                                                           // 3
        "\n",                                                            // 4
        "class Factory {\n",                                             // 5
        "    /**\n",                                                     // 6
        "     * @template T\n",                                          // 7
        "     * @param class-string<T> $class\n",                        // 8
        "     * @return T\n",                                            // 9
        "     */\n",                                                     // 10
        "    public static function create(string $class): object {}\n", // 11
        "}\n",                                                           // 12
        "\n",                                                            // 13
        "class Builder {\n",                                             // 14
        "    public function build(string $name): void {\n",             // 15
        "        $cls = match ($name) {\n",                              // 16
        "            'widget' => Widget::class,\n",                      // 17
        "        };\n",                                                  // 18
        "        $instance = Factory::create($cls);\n",                  // 19
        "        $instance->\n",                                         // 20
        "    }\n",                                                       // 21
        "}\n",                                                           // 22
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

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 20,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $instance->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"render"),
                "Should resolve $cls from match through static Factory::create and show render, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Array shape bodies should have their template parameters substituted
/// when a child class extends a generic parent.  Previously, the bare `T`
/// inside `array{data: T, items: list<T>}` was left unsubstituted because
/// `apply_substitution` did not recurse into `{…}` blocks.
#[tokio::test]
async fn test_generic_array_shape_substitution() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_shape.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class ShapeBase {\n",
        "    /** @return array{data: T, items: list<T>} */\n",
        "    public function getResult(): array {}\n",
        "}\n",
        "\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends ShapeBase<User>\n",
        " */\n",
        "class UserShapeChild extends ShapeBase {\n",
        "    public function test(): void {\n",
        "        $result = $this->getResult();\n",
        "        $result['data']->\n",
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
                line: 19,
                character: 28,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve array shape data: T → User and show 'getName', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Object shape bodies should also have template parameters substituted.
#[tokio::test]
async fn test_generic_object_shape_substitution() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_object_shape.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class ObjectShapeBase {\n",
        "    /** @return object{payload: T} */\n",
        "    public function fetch(): object {}\n",
        "}\n",
        "\n",
        "class Order {\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends ObjectShapeBase<Order>\n",
        " */\n",
        "class OrderFetcher extends ObjectShapeBase {\n",
        "    public function test(): void {\n",
        "        $obj = $this->fetch();\n",
        "        $obj->payload->\n",
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
                line: 19,
                character: 23,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getTotal"),
                "Should resolve object shape payload: T → Order and show 'getTotal', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Variable assigned from a string-key bracket access into an array shape.
/// `$first = $result['data']; $first->` should resolve `$first` to `Gift`
/// via `extract_array_shape_value_type` in the RHS resolution path.
#[tokio::test]
async fn test_generic_shape_string_key_variable_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_shape_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class ShapeAssignBase {\n",
        "    /** @return array{data: T, items: list<T>} */\n",
        "    public function getResult(): array {}\n",
        "}\n",
        "\n",
        "class Gift {\n",
        "    public function open(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends ShapeAssignBase<Gift>\n",
        " */\n",
        "class GiftShapeAssign extends ShapeAssignBase {\n",
        "    public function test(): void {\n",
        "        $result = $this->getResult();\n",
        "        $first = $result['data'];\n",
        "        $first->\n",
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
                line: 20,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"open"),
                "Should resolve $first from $result['data'] (shape key) to Gift and show 'open', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Variable assigned from chained bracket access: string key then element.
/// `$first = $result['items'][0]; $first->` should walk
/// StringKey("items") → `list<Gift>`, then ElementAccess → `Gift`.
#[tokio::test]
async fn test_generic_shape_chained_bracket_variable_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///generics_shape_chain_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class ShapeChainAssignBase {\n",
        "    /** @return array{data: T, items: list<T>} */\n",
        "    public function getResult(): array {}\n",
        "}\n",
        "\n",
        "class Gift {\n",
        "    public function open(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends ShapeChainAssignBase<Gift>\n",
        " */\n",
        "class GiftShapeChainAssign extends ShapeChainAssignBase {\n",
        "    public function test(): void {\n",
        "        $result = $this->getResult();\n",
        "        $first = $result['items'][0];\n",
        "        $first->\n",
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
                line: 20,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"open"),
                "Should resolve $first from $result['items'][0] (shape + list<T>) to Gift and show 'open', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Variable assigned from string key on a plain (non-inherited) @var shape.
/// `/** @var array{name: User} $data */ ... $name = $data['name']; $name->`
#[tokio::test]
async fn test_shape_string_key_from_var_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///shape_var_annotation.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "class Demo {\n",
        "    public function test(): void {\n",
        "        /** @var array{name: User, age: int} $data */\n",
        "        $data = getData();\n",
        "        $name = $data['name'];\n",
        "        $name->\n",
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
                line: 10,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve $name from $data['name'] (shape via @var) to User and show 'getName', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Foreach over a class whose `@extends` parent is transitively iterable
/// should resolve the element type.  Uses a single-level chain first
/// (direct `@extends` of an iterable class) to isolate the resolution path.
#[tokio::test]
async fn test_foreach_extends_generic_collection_single_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_single_level.php").unwrap();
    // SimpleCollection @extends AbstractReflectionCollection<Item>
    // AbstractReflectionCollection @implements IteratorAggregate<T>
    // → foreach element should be Item
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function itemMethod(): void {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template T\n",
        " * @implements IteratorAggregate<T>\n",
        " */\n",
        "abstract class BaseCollection implements IteratorAggregate {}\n",
        "\n",
        "/**\n",
        " * @extends BaseCollection<Item>\n",
        " */\n",
        "class ItemCollection extends BaseCollection {}\n",
        "\n",
        "class Demo {\n",
        "    function test() {\n",
        "        $items = new ItemCollection();\n",
        "        foreach ($items as $item) {\n",
        "            $item->\n",
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

    // Line 20: `            $item->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 20,
                character: 19,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"itemMethod"),
                "Foreach over ItemCollection @extends BaseCollection<Item> should resolve to Item and show 'itemMethod', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Multi-level generic collection: interface -> abstract class -> concrete class
/// should resolve the element type in foreach through the full chain.
/// Mirrors the `multi_level_collection_foreach.fixture` test.
#[tokio::test]
async fn test_multi_level_generic_collection_foreach() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///multi_level_foreach.php").unwrap();
    // Top-level code (matching the fixture pattern) — the foreach
    // resolution at the top level walks `program.statements` directly
    // via `walk_statements_for_assignments`.
    let text = concat!(
        "<?php\n",                                                                          // 0
        "/**\n",                                                                            // 1
        " * @template T\n",                                                                 // 2
        " * @extends IteratorAggregate<T>\n",                                               // 3
        " */\n",                                                                            // 4
        "interface ReflectionCollection extends IteratorAggregate {}\n",                    // 5
        "\n",                                                                               // 6
        "/**\n",                                                                            // 7
        " * @template T\n",                                                                 // 8
        " * @implements ReflectionCollection<T>\n",                                         // 9
        " */\n",                                                                            // 10
        "abstract class AbstractReflectionCollection implements ReflectionCollection {}\n", // 11
        "\n",                                                                               // 12
        "/**\n",                                                                            // 13
        " * @extends AbstractReflectionCollection<ReflectionArgument>\n",                   // 14
        " */\n",                                                                            // 15
        "class ReflectionArgumentCollection extends AbstractReflectionCollection {}\n",     // 16
        "\n",                                                                               // 17
        "class ReflectionArgument {\n",                                                     // 18
        "    public function argMethod(): void {}\n",                                       // 19
        "}\n",                                                                              // 20
        "\n",                                                                               // 21
        "class ReflectionNode {\n",                                                         // 22
        "    public function arguments(): ReflectionArgumentCollection {}\n",               // 23
        "}\n",                                                                              // 24
        "\n",                                                                               // 25
        "$node = new ReflectionNode();\n",                                                  // 26
        "$collection = $node->arguments();\n",                                              // 27
        "\n",                                                                               // 28
        "foreach ($collection as $item) {\n",                                               // 29
        "    $item->\n",                                                                    // 30
        "}\n",                                                                              // 31
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

    // Line 30: `    $item->`  (4 spaces + `$item->` = char 11 is after `->`)
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 30,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"argMethod"),
                "Multi-level generic foreach should resolve element type to ReflectionArgument and show 'argMethod', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Chain-level generic substitution tests ─────────────────────────────────
//
// These tests verify that when a method returns a parameterised generic type
// like `Collection<int, Product>`, calling a method on that result correctly
// substitutes the class-level template parameters into the called method's
// return type.  For example, `Collection<int, Product>::first()` should
// resolve `TValue` → `Product` so that `->first()->` shows `Product` members.

/// When a method returns `Collection<int, Product>`, calling `first()` on the
/// result should resolve `TValue` to `Product` and offer `Product` members.
#[tokio::test]
async fn test_chain_level_generic_substitution_first() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_generic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function getPrice(): float {}\n",
        "    public function getSku(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "    /** @return TValue */\n",
        "    public function last() {}\n",
        "}\n",
        "\n",
        "class ProductService {\n",
        "    /** @return Collection<int, Product> */\n",
        "    public function getProducts(): Collection {}\n",
        "\n",
        "    function test() {\n",
        "        $this->getProducts()->first()->\n",
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
                line: 22,
                character: 42,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "Chain should resolve TValue→Product via Collection<int, Product>::first() and show 'getPrice', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getSku"),
                "Chain should resolve TValue→Product via Collection<int, Product>::first() and show 'getSku', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a variable is assigned from a chain that returns a parameterised
/// generic, calling a method on the result should substitute template params.
#[tokio::test]
async fn test_chain_level_generic_substitution_variable_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///chain_generic_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getEmail(): string {}\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "}\n",
        "\n",
        "class UserRepo {\n",
        "    /** @return Collection<int, User> */\n",
        "    public function findAll(): Collection {}\n",
        "\n",
        "    function test() {\n",
        "        $users = $this->findAll();\n",
        "        $first = $users->first();\n",
        "        $first->\n",
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
                line: 22,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getEmail"),
                "Variable assigned from Collection<int, User>::first() should resolve to User and show 'getEmail', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getName"),
                "Variable assigned from Collection<int, User>::first() should resolve to User and show 'getName', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a mixin carries generic arguments (e.g. `@mixin Builder<TRelatedModel>`),
/// the template params of the mixin class should be substituted with the
/// provided generic arguments when merging members.
#[tokio::test]
async fn test_mixin_generic_substitution() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///mixin_generic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function getPrice(): float {}\n",
        "    public function getSku(): string {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template TModel\n",
        " */\n",
        "class Builder {\n",
        "    /** @return TModel */\n",
        "    public function firstOrFail() {}\n",
        "    /** @return TModel */\n",
        "    public function find() {}\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template TRelatedModel\n",
        " * @mixin Builder<TRelatedModel>\n",
        " */\n",
        "class Relation {\n",
        "}\n",
        "\n",
        "/**\n",
        " * @extends Relation<Product>\n",
        " */\n",
        "class BelongsTo extends Relation {\n",
        "}\n",
        "\n",
        "class OrderLine {\n",
        "    public function product(): BelongsTo {}\n",
        "\n",
        "    function test() {\n",
        "        $this->product()->firstOrFail()->\n",
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
                line: 33,
                character: 45,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "Mixin generic substitution should resolve TModel→Product via @mixin Builder<TRelatedModel> and show 'getPrice', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getSku"),
                "Mixin generic substitution should resolve TModel→Product via @mixin Builder<TRelatedModel> and show 'getSku', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Inherited property template substitution on $this-> ───────────────

/// When a class extends a generic parent with `@extends Parent<Concrete>`,
/// inherited properties whose types use the parent's template parameters
/// should have those parameters substituted.  For example, a property
/// typed `array<TKey, TValue>` should become `array<int, Message>`.
#[tokio::test]
async fn test_inherited_array_property_template_substitution() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///t19_array_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Message {\n",
        "    public string $text;\n",
        "    public function getText(): string { return ''; }\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @var array<TKey, TValue> */\n",
        "    public array $items = [];\n",
        "\n",
        "    /** @return TValue|null */\n",
        "    public function first(): mixed { return null; }\n",
        "}\n",
        "\n",
        "/** @extends Collection<int, Message> */\n",
        "final class MessageCollection extends Collection {\n",
        "    public function test(): void {\n",
        "        foreach ($this->items as $item) {\n",
        "            $item->\n",
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

    // Cursor after `$item->` on the foreach body line
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 22,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $item-> inside foreach over $this->items"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getText"),
                "Foreach over $this->items (array<int, Message>) should resolve $item to Message and show 'getText', got methods: {:?}",
                method_names
            );
            assert!(
                prop_names.contains(&"text"),
                "Foreach over $this->items (array<int, Message>) should resolve $item to Message and show 'text', got props: {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Direct access to an inherited generic-typed property should resolve
/// the substituted type for chaining (e.g. `$this->items[0]->`).
#[tokio::test]
async fn test_inherited_property_bracket_access_template_substitution() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///t19_bracket.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Task {\n",
        "    public string $title;\n",
        "    public function getTitle(): string { return ''; }\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class TypedList {\n",
        "    /** @var list<T> */\n",
        "    public array $data = [];\n",
        "}\n",
        "\n",
        "/** @extends TypedList<Task> */\n",
        "class TaskList extends TypedList {\n",
        "    public function demo(): void {\n",
        "        $first = $this->data[0];\n",
        "        $first->\n",
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
                line: 18,
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
        "Completion should return results for $first-> after $this->data[0]"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getTitle"),
                "Should resolve $this->data[0] to Task and show 'getTitle', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Method-level template parameter resolution inside method body.
/// When `@template T of Builder` and `@param T $query`, accessing
/// `$query->` inside the method body should resolve T to its bound
/// (`Builder`) and offer Builder's methods.
#[tokio::test]
async fn test_method_template_param_resolves_to_bound_inside_body() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_body_bound.php").unwrap();
    let text = concat!(
        "<?php\n",                                                              // 0
        "class Builder {\n",                                                    // 1
        "    public function where(string $col): static { return $this; }\n",   // 2
        "    public function orderBy(string $col): static { return $this; }\n", // 3
        "}\n",                                                                  // 4
        "\n",                                                                   // 5
        "class Country {}\n",                                                   // 6
        "\n",                                                                   // 7
        "class ProductRepository {\n",                                          // 8
        "    /**\n",                                                            // 9
        "     * @template T of Builder\n",                                      // 10
        "     * @param T $query\n",                                             // 11
        "     * @return T\n",                                                   // 12
        "     */\n",                                                            // 13
        "    private static function filterDisabled(Builder $query, Country $code): Builder {\n", // 14
        "        $query->\n", // 15
        "    }\n",            // 16
        "}\n",                // 17
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"where"),
                "Should resolve T to Builder and show 'where', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"orderBy"),
                "Should resolve T to Builder and show 'orderBy', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Method-level template with union bound inside method body.
/// `@template T of Builder|QueryBuilder` should resolve to both types.
#[tokio::test]
async fn test_method_template_union_bound_resolves_inside_body() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tpl_union_body.php").unwrap();
    let text = concat!(
        "<?php\n",                                                              // 0
        "class Builder {\n",                                                    // 1
        "    public function where(string $col): static { return $this; }\n",   // 2
        "}\n",                                                                  // 3
        "\n",                                                                   // 4
        "class QueryBuilder {\n",                                               // 5
        "    public function orderBy(string $col): static { return $this; }\n", // 6
        "    public function where(string $col): static { return $this; }\n",   // 7
        "}\n",                                                                  // 8
        "\n",                                                                   // 9
        "class Country {}\n",                                                   // 10
        "\n",                                                                   // 11
        "class ProductRepository {\n",                                          // 12
        "    /**\n",                                                            // 13
        "     * @template T of Builder|QueryBuilder\n",                         // 14
        "     * @param T $query\n",                                             // 15
        "     * @return T\n",                                                   // 16
        "     */\n",                                                            // 17
        "    private static function filterDisabled(Builder $query, Country $code): Builder {\n", // 18
        "        $query->\n", // 19
        "    }\n",            // 20
        "}\n",                // 21
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
                line: 19,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"where"),
                "Should resolve T to Builder|QueryBuilder and show 'where', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"orderBy"),
                "Should resolve T to Builder|QueryBuilder and show 'orderBy', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Trait method-level template inside body.
/// `@template TRelation of Relation` — `$relation->getQuery()` should
/// resolve via the Relation bound.
#[tokio::test]
async fn test_trait_method_template_param_resolves_inside_body() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///trait_tpl_body.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                     // 0
        "class QueryBuilder {\n",                                                      // 1
        "    public function where(string $col): static { return $this; }\n",          // 2
        "}\n",                                                                         // 3
        "\n",                                                                          // 4
        "class Relation {\n",                                                          // 5
        "    public function getQuery(): QueryBuilder {}\n",                           // 6
        "}\n",                                                                         // 7
        "\n",                                                                          // 8
        "trait GetMarketTrait {\n",                                                    // 9
        "    /**\n",                                                                   // 10
        "     * @template TRelation of Relation\n",                                    // 11
        "     * @param TRelation $relation\n",                                         // 12
        "     * @return TRelation\n",                                                  // 13
        "     */\n",                                                                   // 14
        "    protected function whereCurrentMarket(Relation $relation): Relation {\n", // 15
        "        $relation->\n",                                                       // 16
        "    }\n",                                                                     // 17
        "}\n",                                                                         // 18
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
                character: 20,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getQuery"),
                "Should resolve TRelation to Relation and show 'getQuery', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Function-level template parameter resolution inside body.
/// Standalone function with `@template T of SomeClass` and `@param T $item`.
#[tokio::test]
async fn test_function_template_param_resolves_to_bound_inside_body() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///func_tpl_body_bound.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // 0
        "class Model {\n",                            // 1
        "    public function save(): bool {}\n",      // 2
        "    public function delete(): bool {}\n",    // 3
        "}\n",                                        // 4
        "\n",                                         // 5
        "/**\n",                                      // 6
        " * @template T of Model\n",                  // 7
        " * @param T $entity\n",                      // 8
        " * @return T\n",                             // 9
        " */\n",                                      // 10
        "function persist(Model $entity): Model {\n", // 11
        "    $entity->\n",                            // 12
        "}\n",                                        // 13
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
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"save"),
                "Should resolve T to Model and show 'save', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"delete"),
                "Should resolve T to Model and show 'delete', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a class declares `@template TWraps` and `@mixin TWraps`, and a
/// property is typed as `Wrapper<ConcreteApi>`, the mixin should resolve
/// `TWraps` → `ConcreteApi` and expose `ConcreteApi`'s methods on the
/// property.  This is the "direct property access" scenario — no child
/// class extends the wrapper; the concrete type comes entirely from the
/// `@var` generic annotation on the property.
///
/// Reproduces the Klaviyo SDK pattern:
///   `/** @var Subclient<EventsApi> */ public $Events;`
#[tokio::test]
async fn test_mixin_template_param_resolved_via_property_generic_type() {
    let backend = create_test_backend();

    let wrapper_uri = Url::parse("file:///Subclient.php").unwrap();
    let wrapper_text = r#"<?php
/**
 * @template TWraps of object
 * @mixin TWraps
 */
class Subclient {
    public function getApiInstance(): object {}
}
"#;

    let api_uri = Url::parse("file:///EventsApi.php").unwrap();
    let api_text = r#"<?php
class EventsApi {
    public function createEvent(array $body): array {}
    public function getEvents(string $filter): array {}
    public function deleteEvent(string $id): void {}
}
"#;

    let consumer_uri = Url::parse("file:///KlaviyoClient.php").unwrap();
    let consumer_text = r#"<?php
class KlaviyoClient {
    /** @var Subclient<EventsApi> */
    public $Events;

    function test() {
        $this->Events->
    }
}
"#;

    // Open all files
    for (uri, text) in [
        (&wrapper_uri, wrapper_text),
        (&api_uri, api_text),
        (&consumer_uri, consumer_text),
    ] {
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
    }

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: consumer_uri.clone(),
            },
            position: Position {
                line: 6,
                character: 24,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"createEvent"),
                "Mixin template param via @var Subclient<EventsApi> should expose 'createEvent', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEvents"),
                "Mixin template param via @var Subclient<EventsApi> should expose 'getEvents', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"deleteEvent"),
                "Mixin template param via @var Subclient<EventsApi> should expose 'deleteEvent', got: {:?}",
                method_names
            );
            // The wrapper's own method should also be present.
            assert!(
                method_names.contains(&"getApiInstance"),
                "Wrapper's own method 'getApiInstance' should still be present, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `@mixin Foo|Bar` is used, members from both classes should be available.
#[tokio::test]
async fn test_mixin_union_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///mixin_union.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Webpage {\n",
        "    public function getUrl(): string {}\n",
        "}\n",
        "class AwaitableWebpage {\n",
        "    public function doAwait(): void {}\n",
        "}\n",
        "/**\n",
        " * @mixin Webpage|AwaitableWebpage\n",
        " */\n",
        "final class PendingAwaitablePage {\n",
        "    public function pending(): bool {}\n",
        "}\n",
        "$page = new PendingAwaitablePage();\n",
        "$page->\n",
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

    let line = text.lines().count() as u32 - 1;
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position { line, character: 7 },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(result.is_some(), "Completion should return results");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(
                names.contains(&"getUrl"),
                "Union mixin should include Webpage::getUrl(), got: {:?}",
                names
            );
            assert!(
                names.contains(&"doAwait"),
                "Union mixin should include AwaitableWebpage::doAwait(), got: {:?}",
                names
            );
            assert!(
                names.contains(&"pending"),
                "Own method pending() should be present, got: {:?}",
                names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `@param Closure(T): void $cb` receives `fn(User $u) => ...`,
/// the template param `T` should be inferred from the closure's parameter
/// type (contravariant position).
#[tokio::test]
async fn test_template_inferred_from_closure_param_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///closure_param_template.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "class User {\n",                                                 // 1
        "    public function getName(): string {}\n",                     // 2
        "    public function getEmail(): string {}\n",                    // 3
        "}\n",                                                            // 4
        "\n",                                                             // 5
        "class EventBus {\n",                                             // 6
        "    /**\n",                                                      // 7
        "     * @template T\n",                                           // 8
        "     * @param Closure(T): void $callback\n",                     // 9
        "     * @return T\n",                                             // 10
        "     */\n",                                                      // 11
        "    public function listen(Closure $callback): mixed {}\n",      // 12
        "}\n",                                                            // 13
        "\n",                                                             // 14
        "function test() {\n",                                            // 15
        "    $bus = new EventBus();\n",                                   // 16
        "    $result = $bus->listen(fn(User $u): void => $u->save());\n", // 17
        "    $result->\n",                                                // 18
        "}\n",                                                            // 19
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
                character: 13,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "T should be inferred as User from closure param type, showing 'getName', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "T should be inferred as User from closure param type, showing 'getEmail', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `@param callable(TCarry, TValue): TCarry $cb` receives a closure
/// with a typed return, `TCarry` should be inferred from both the return
/// type *and* the first parameter type.  This models `Collection::reduce`.
#[tokio::test]
async fn test_template_inferred_from_closure_return_type_reduce_pattern() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///closure_reduce_template.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                     // 0
        "class Decimal {\n",                                                           // 1
        "    public function add(Decimal $other): Decimal {}\n",                       // 2
        "    public function getValue(): string {}\n",                                 // 3
        "}\n",                                                                         // 4
        "\n",                                                                          // 5
        "class Collection {\n",                                                        // 6
        "    /**\n",                                                                   // 7
        "     * @template TReduceReturnType\n",                                        // 8
        "     * @param callable(TReduceReturnType, mixed): TReduceReturnType $cb\n",   // 9
        "     * @param TReduceReturnType $initial\n",                                  // 10
        "     * @return TReduceReturnType\n",                                          // 11
        "     */\n",                                                                   // 12
        "    public function reduce(callable $cb, mixed $initial = null): mixed {}\n", // 13
        "}\n",                                                                         // 14
        "\n",                                                                          // 15
        "function test() {\n",                                                         // 16
        "    $c = new Collection();\n",                                                // 17
        "    $total = $c->reduce(fn(Decimal $carry, $item): Decimal => $carry, new Decimal());\n", // 18
        "    $total->\n", // 19
        "}\n",            // 20
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
                line: 19,
                character: 12,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"add"),
                "TReduceReturnType should resolve to Decimal showing 'add', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getValue"),
                "TReduceReturnType should resolve to Decimal showing 'getValue', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Template param inferred from the second closure parameter position.
/// `@param Closure(int, T): void $cb` with `fn(int $i, Order $o) => ...`
/// should infer `T` = `Order`.
#[tokio::test]
async fn test_template_inferred_from_closure_second_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///closure_second_param_template.php").unwrap();
    let text = concat!(
        "<?php\n",                                                           // 0
        "class Order {\n",                                                   // 1
        "    public function getTotal(): float {}\n",                        // 2
        "    public function getStatus(): string {}\n",                      // 3
        "}\n",                                                               // 4
        "\n",                                                                // 5
        "class Processor {\n",                                               // 6
        "    /**\n",                                                         // 7
        "     * @template T\n",                                              // 8
        "     * @param Closure(int, T): void $cb\n",                         // 9
        "     * @return T\n",                                                // 10
        "     */\n",                                                         // 11
        "    public function process(Closure $cb): mixed {}\n",              // 12
        "}\n",                                                               // 13
        "\n",                                                                // 14
        "function test() {\n",                                               // 15
        "    $proc = new Processor();\n",                                    // 16
        "    $item = $proc->process(fn(int $i, Order $o): void => null);\n", // 17
        "    $item->\n",                                                     // 18
        "}\n",                                                               // 19
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getTotal"),
                "T should be inferred as Order from second closure param, showing 'getTotal', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getStatus"),
                "T should be inferred as Order from second closure param, showing 'getStatus', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Function-level template inferred from closure param (contravariant).
/// `@param Closure(T): void $cb` on a standalone function.
#[tokio::test]
async fn test_function_template_inferred_from_closure_param_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///func_closure_param_template.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class Product {\n",                                    // 1
        "    public function getPrice(): float {}\n",           // 2
        "    public function getSku(): string {}\n",            // 3
        "}\n",                                                  // 4
        "\n",                                                   // 5
        "/**\n",                                                // 6
        " * @template T\n",                                     // 7
        " * @param Closure(T): void $cb\n",                     // 8
        " * @return T\n",                                       // 9
        " */\n",                                                // 10
        "function tap(Closure $cb): mixed {}\n",                // 11
        "\n",                                                   // 12
        "$result = tap(fn(Product $p): void => $p->save());\n", // 13
        "$result->\n",                                          // 14
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
                character: 9,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "T should be inferred as Product from closure param, showing 'getPrice', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getSku"),
                "T should be inferred as Product from closure param, showing 'getSku', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Closure with full `function` keyword (not arrow fn) should also have
/// its parameter type extracted for template inference.
#[tokio::test]
async fn test_template_inferred_from_full_closure_param_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///full_closure_param_template.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                    // 0
        "class Invoice {\n",                                                          // 1
        "    public function getAmount(): float {}\n",                                // 2
        "    public function isPaid(): bool {}\n",                                    // 3
        "}\n",                                                                        // 4
        "\n",                                                                         // 5
        "class Pipeline {\n",                                                         // 6
        "    /**\n",                                                                  // 7
        "     * @template T\n",                                                       // 8
        "     * @param Closure(T): void $handler\n",                                  // 9
        "     * @return T\n",                                                         // 10
        "     */\n",                                                                  // 11
        "    public function handle(Closure $handler): mixed {}\n",                   // 12
        "}\n",                                                                        // 13
        "\n",                                                                         // 14
        "function test() {\n",                                                        // 15
        "    $pipe = new Pipeline();\n",                                              // 16
        "    $inv = $pipe->handle(function(Invoice $i): void { $i->process(); });\n", // 17
        "    $inv->\n",                                                               // 18
        "}\n",                                                                        // 19
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getAmount"),
                "T should be inferred as Invoice from full closure param, showing 'getAmount', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"isPaid"),
                "T should be inferred as Invoice from full closure param, showing 'isPaid', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `Collection::reduce()` with the real Laravel signature that uses TWO
/// template params: `TReduceInitial` for the `$initial` arg and
/// `TReduceReturnType` for the closure return type.  The callable's first
/// parameter is typed as the union `TReduceInitial|TReduceReturnType`.
///
/// PHPantom must infer `TReduceReturnType = Decimal` from the closure's
/// return type annotation so that `reduce()` returns `Decimal`.
#[tokio::test]
async fn test_reduce_two_template_params_union_callable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///reduce_two_tpl.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class Decimal {\n",                                     // 1
        "    public function add(Decimal $other): Decimal {}\n", // 2
        "    public function getValue(): string {}\n",           // 3
        "}\n",                                                   // 4
        "\n",                                                    // 5
        "class OrderProduct {\n",                                // 6
        "    public float $price;\n",                            // 7
        "}\n",                                                   // 8
        "\n",                                                    // 9
        "/**\n",                                                 // 10
        " * @template TKey\n",                                   // 11
        " * @template TValue\n",                                 // 12
        " */\n",                                                 // 13
        "class Collection {\n",                                  // 14
        "    /**\n",                                             // 15
        "     * @template TReduceInitial\n",                     // 16
        "     * @template TReduceReturnType\n",                  // 17
        "     * @param callable(TReduceInitial|TReduceReturnType, TValue, TKey): TReduceReturnType $callback\n", // 18
        "     * @param TReduceInitial $initial\n", // 19
        "     * @return TReduceReturnType\n",      // 20
        "     */\n",                               // 21
        "    public function reduce(callable $callback, mixed $initial = null): mixed {}\n", // 22
        "}\n",                                     // 23
        "\n",                                      // 24
        "function test() {\n",                     // 25
        "    /** @var Collection<int, OrderProduct> $products */\n", // 26
        "    $products = new Collection();\n",     // 27
        "    $total = $products->reduce(fn(Decimal $carry, OrderProduct $p): Decimal => $carry->add($p->price), new Decimal('0'));\n", // 28
        "    $total->\n", // 29
        "}\n",            // 30
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
                line: 29,
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
        "Completion should return results for $total->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"add"),
                "TReduceReturnType should resolve to Decimal via closure return type, showing 'add', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getValue"),
                "TReduceReturnType should resolve to Decimal via closure return type, showing 'getValue', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a method returns `ArrayIterator<int, Rule>`, calling `->current()`
/// on it should resolve to `Rule` via the class stub patch that adds
/// `@template TKey, TValue` and `@implements SeekableIterator<TKey, TValue>`
/// to `ArrayIterator`.
///
/// The resolution chain is:
/// 1. `getRules()` return type `ArrayIterator<int, Rule>` loads the stub
/// 2. Stub patch adds `@template TKey of array-key`, `@template TValue`
/// 3. `apply_generic_args` substitutes `TKey=int`, `TValue=Rule`
/// 4. Interface merge via `@implements SeekableIterator<TKey, TValue>` →
///    `SeekableIterator<int, Rule>` → `Iterator<int, Rule>`
/// 5. `Iterator::current()` returns `TValue` → substituted to `Rule`
///
/// This test uses a direct method chain (`$ruleSet->getRules()->current()->`)
/// to avoid variable-resolution complexity.
#[tokio::test]
async fn test_array_iterator_current_resolves_generic_value_type() {
    let backend = create_spl_generic_backend();

    let uri = Url::parse("file:///array_iterator_generics.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Rule {\n",
        "    public function getPriority(): int { return 0; }\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "\n",
        "class RuleSet {\n",
        "    /** @return \\ArrayIterator<int, Rule> */\n",
        "    public function getRules(): \\ArrayIterator { return new \\ArrayIterator(); }\n",
        "}\n",
        "\n",
        "function test(RuleSet $ruleSet) {\n",
        "    $ruleSet->getRules()->current()->\n",
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

    // Cursor is after `->` on the chain line (line 12, end of `->`)
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 42,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPriority"),
                "ArrayIterator<int, Rule>::current() should resolve to Rule, showing 'getPriority', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getName"),
                "ArrayIterator<int, Rule>::current() should resolve to Rule, showing 'getName', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same as above but verifies variable assignment: `$rule = ...->current(); $rule->`
#[tokio::test]
async fn test_array_iterator_current_via_variable_assignment() {
    let backend = create_spl_generic_backend();

    let uri = Url::parse("file:///array_iterator_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Priority {\n",
        "    public function getLevel(): int { return 0; }\n",
        "}\n",
        "\n",
        "class Container {\n",
        "    /** @return \\ArrayIterator<int, Priority> */\n",
        "    public function items(): \\ArrayIterator { return new \\ArrayIterator(); }\n",
        "}\n",
        "\n",
        "function test(Container $c) {\n",
        "    $item = $c->items()->current();\n",
        "    $item->\n",
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

    // Cursor is after `$item->` (line 12, character 11)
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
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
        "Completion should return results for $item->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getLevel"),
                "ArrayIterator<int, Priority>::current() should resolve to Priority via variable, showing 'getLevel', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_new_variable_with_class_string_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///new_class_string.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function speak(): string { return ''; }\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Dog extends Animal {\n",
        "    public function fetch(): void {}\n",
        "}\n",
        "class Factory {\n",
        "    /**\n",
        "     * @template T of Animal\n",
        "     * @param class-string<T> $class\n",
        "     * @return T\n",
        "     */\n",
        "    public function create(string $class): object {\n",
        "        $instance = new $class();\n",
        "        $instance->\n",
        "        return $instance;\n",
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
                    character: 21,
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

            // $class is class-string<T> where T of Animal, so new $class()
            // should resolve to Animal (the bound), giving access to its methods.
            assert!(
                method_names.contains(&"speak"),
                "Should include 'speak' from Animal (class-string bound), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getName"),
                "Should include 'getName' from Animal (class-string bound), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_implements_generic_interface_completion() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///implements_generic_interface.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template K\n",
        " * @template V\n",
        " */\n",
        "interface Collection {\n",
        "    /** @return V */\n",
        "    public function get(mixed $key): mixed;\n",
        "}\n",
        "/**\n",
        " * @implements Collection<string, User>\n",
        " */\n",
        "class UserCollection implements Collection {\n",
        "    public function get(mixed $key): mixed { return new User(); }\n",
        "}\n",
        "class Svc {\n",
        "    public function test(): void {\n",
        "        $coll = new UserCollection();\n",
        "        $coll->get('foo')->\n",
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
                line: 22,
                character: 30,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve V to User and show User's 'getName' method, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should resolve V to User and show User's 'getEmail' method, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Class-level template parameters must survive through a chained method call
/// that returns `self<T>`.  When `products()` returns `Collection<Product>`
/// and `filter()` returns `self<TItem>`, completing after `->` should show
/// Collection's methods (proving the type is still Collection<Product>).
#[tokio::test]
async fn test_class_template_params_preserved_through_chained_calls() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b8_chain_generic.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // 0
        "class Product {\n",                               // 1
        "    public function getPrice(): float {}\n",      // 2
        "    public function getTitle(): string {}\n",     // 3
        "}\n",                                             // 4
        "\n",                                              // 5
        "/**\n",                                           // 6
        " * @template TItem\n",                            // 7
        " */\n",                                           // 8
        "class Collection {\n",                            // 9
        "    /** @return TItem */\n",                      // 10
        "    public function first() {}\n",                // 11
        "    /** @return self<TItem> */\n",                // 12
        "    public function filter() {}\n",               // 13
        "    /** @return self<TItem> */\n",                // 14
        "    public function sort() {}\n",                 // 15
        "}\n",                                             // 16
        "\n",                                              // 17
        "class Store {\n",                                 // 18
        "    /** @return Collection<Product> */\n",        // 19
        "    public function products(): Collection {}\n", // 20
        "\n",                                              // 21
        "    function test() {\n",                         // 22
        "        $this->products()->filter()->\n",         // 23
        "    }\n",                                         // 24
        "}\n",                                             // 25
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

    // Line 23: "        $this->products()->filter()->"
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 23,
                character: 99,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            // After filter(), we should still have Collection<Product> so
            // Collection's own methods should be visible.
            assert!(
                method_names.contains(&"first"),
                "Chained products()->filter()-> should show Collection methods like 'first', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"sort"),
                "Chained products()->filter()-> should show Collection methods like 'sort', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// After calling a method that returns `self<TItem>` on a Collection<Product>,
/// further chaining `->first()->` should resolve TItem to Product and show
/// Product's methods.  Uses a variable assignment to keep the chain to 2 levels
/// at the completion point.
#[tokio::test]
async fn test_class_template_params_preserved_through_self_return_then_first() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///b8_chain_first.php").unwrap();
    let text = concat!(
        "<?php\n",                                            // 0
        "class Product {\n",                                  // 1
        "    public function getPrice(): float {}\n",         // 2
        "    public function getTitle(): string {}\n",        // 3
        "}\n",                                                // 4
        "\n",                                                 // 5
        "/**\n",                                              // 6
        " * @template TItem\n",                               // 7
        " */\n",                                              // 8
        "class Collection {\n",                               // 9
        "    /** @return TItem */\n",                         // 10
        "    public function first() {}\n",                   // 11
        "    /** @return self<TItem> */\n",                   // 12
        "    public function filter() {}\n",                  // 13
        "}\n",                                                // 14
        "\n",                                                 // 15
        "class Store {\n",                                    // 16
        "    /** @return Collection<Product> */\n",           // 17
        "    public function products(): Collection {}\n",    // 18
        "\n",                                                 // 19
        "    function test() {\n",                            // 20
        "        $filtered = $this->products()->filter();\n", // 21
        "        $filtered->first()->\n",                     // 22
        "    }\n",                                            // 23
        "}\n",                                                // 24
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

    // Line 22: "        $filtered->first()->"
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 22,
                character: 99,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "After filter() (self<TItem>), first() should resolve TItem→Product and show 'getPrice', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getTitle"),
                "After filter() (self<TItem>), first() should resolve TItem→Product and show 'getTitle', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_static_return_preserves_generic_parameters() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_static_return_generics.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                  // 0
        "/** @template T */\n",                                                     // 1
        "class Collection {\n",                                                     // 2
        "    /** @var list<T> */\n",                                                // 3
        "    private array $items = [];\n",                                         // 4
        "    public function lateBinding(): static { return new static(); }\n",     // 5
        "    /** @return T|null */\n",                                              // 6
        "    public function first(): mixed { return $this->items[0] ?? null; }\n", // 7
        "}\n",                                                                      // 8
        "class User { public function getName(): string { return ''; } }\n",        // 9
        "/** @var Collection<User> $c */\n",                                        // 10
        "$c = new Collection();\n",                                                 // 11
        "$result = $c->lateBinding();\n",                                           // 12
        "$item = $result->first();\n",                                              // 13
        "$item->\n",                                                                // 14
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
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 14,
                    character: 7,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "After lateBinding() returning static (Collection<User>), first() should resolve T→User and show 'getName', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Chained instantiation `(new C("foo"))->get()` should propagate
/// constructor-inferred generic args to the method call so that `get()`
/// returns the concrete type (`string`) instead of `mixed`.
#[tokio::test]
async fn test_chained_new_expression_propagates_constructor_generics() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///chained_new_generic.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "/**\n",                                                          // 1
        " * @template T\n",                                               // 2
        " */\n",                                                          // 3
        "class Box {\n",                                                  // 4
        "    /** @var T */\n",                                            // 5
        "    private $val;\n",                                            // 6
        "    /** @param T $val */\n",                                     // 7
        "    public function __construct($val) { $this->val = $val; }\n", // 8
        "    /** @return T */\n",                                         // 9
        "    public function get() { return $this->val; }\n",             // 10
        "}\n",                                                            // 11
        "\n",                                                             // 12
        "class Product {\n",                                              // 13
        "    public function getPrice(): float {}\n",                     // 14
        "    public function getTitle(): string {}\n",                    // 15
        "}\n",                                                            // 16
        "\n",                                                             // 17
        "function demo() {\n",                                            // 18
        "    (new Box(new Product()))->get()->\n",                        // 19
        "}\n",                                                            // 20
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
                line: 19,
                character: 39,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getPrice"),
                "Chained (new Box(new Product()))->get()-> should resolve T→Product and show 'getPrice', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getTitle"),
                "Chained (new Box(new Product()))->get()-> should resolve T→Product and show 'getTitle', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Chained instantiation with a string literal: `(new Box("hello"))->get()`
/// should infer T=string so that `get()` returns `string`.
#[tokio::test]
async fn test_chained_new_expression_string_literal_generic() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///chained_new_string.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "/**\n",                                                          // 1
        " * @template T\n",                                               // 2
        " */\n",                                                          // 3
        "class Wrapper {\n",                                              // 4
        "    /** @var T */\n",                                            // 5
        "    private $val;\n",                                            // 6
        "    /** @param T $val */\n",                                     // 7
        "    public function __construct($val) { $this->val = $val; }\n", // 8
        "    /** @return T */\n",                                         // 9
        "    public function unwrap() { return $this->val; }\n",          // 10
        "}\n",                                                            // 11
        "\n",                                                             // 12
        "class StringHelper {\n",                                         // 13
        "    public function toUpper(): string {}\n",                     // 14
        "}\n",                                                            // 15
        "\n",                                                             // 16
        "function demo() {\n",                                            // 17
        "    (new Wrapper(new StringHelper()))->unwrap()->\n",            // 18
        "}\n",                                                            // 19
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
                character: 51,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"toUpper"),
                "Chained (new Wrapper(new StringHelper()))->unwrap()-> should resolve T→StringHelper and show 'toUpper', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Bare `new Box(new Product())->get()->` (PHP 8.4 style, no parens)
/// should also propagate constructor generics.
#[tokio::test]
async fn test_bare_new_expression_chain_propagates_constructor_generics() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///bare_new_chain.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "/**\n",                                                          // 1
        " * @template T\n",                                               // 2
        " */\n",                                                          // 3
        "class Container {\n",                                            // 4
        "    /** @var T */\n",                                            // 5
        "    private $val;\n",                                            // 6
        "    /** @param T $val */\n",                                     // 7
        "    public function __construct($val) { $this->val = $val; }\n", // 8
        "    /** @return T */\n",                                         // 9
        "    public function value() { return $this->val; }\n",           // 10
        "}\n",                                                            // 11
        "\n",                                                             // 12
        "class Item {\n",                                                 // 13
        "    public function getSku(): string {}\n",                      // 14
        "}\n",                                                            // 15
        "\n",                                                             // 16
        "function demo() {\n",                                            // 17
        "    new Container(new Item())->value()->\n",                     // 18
        "}\n",                                                            // 19
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
                character: 43,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getSku"),
                "Bare new Container(new Item())->value()-> should resolve T→Item and show 'getSku', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Constructor with `@param array<TKey, TValue> $kv` should infer key and
/// value types from an array literal argument like `["a" => 1]`.
#[tokio::test]
async fn test_constructor_array_key_value_template_inference() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///array_kv_ctor.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TValue\n",
        " */\n",
        "class TypedMap {\n",
        "    /** @var array<TKey, TValue> */\n",
        "    private $data;\n",
        "    /** @param array<TKey, TValue> $data */\n",
        "    public function __construct(array $data) { $this->data = $data; }\n",
        "    /** @return TValue */\n",
        "    public function first() { return reset($this->data); }\n",
        "    /** @return TKey */\n",
        "    public function firstKey() { return array_key_first($this->data); }\n",
        "}\n",
        "\n",
        "class Product {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "function demo() {\n",
        "    $map = new TypedMap([\"a\" => new Product()]);\n",
        "    $map->first()->\n",
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
                line: 22,
                character: 20,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "array<TKey, TValue> constructor inference should resolve TValue\u{2192}Product, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a child class extends ArrayObject with `@template-extends \ArrayObject<TKey,TValue>`,
/// calling `getIterator()` should return `ArrayIterator` with substituted template params,
/// enabling completion on the iterator's methods like `current()`.
#[tokio::test]
async fn test_arrayobject_getiterator_template_substitution() {
    let backend = create_spl_generic_backend();

    let uri = Url::parse("file:///arrayobj_iter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TValue\n",
        " * @template-extends \\ArrayObject<TKey,TValue>\n",
        " */\n",
        "class TypedArrayObject extends \\ArrayObject {\n",
        "    /**\n",
        "     * @param array<TKey,TValue> $kv\n",
        "     */\n",
        "    public function __construct(array $kv) {\n",
        "        parent::__construct($kv);\n",
        "    }\n",
        "}\n",
        "\n",
        "class Product {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "function demo() {\n",
        "    $c = new TypedArrayObject([\"a\" => new Product()]);\n",
        "    $iter = $c->getIterator();\n",
        "    $iter->current()->\n",
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

    // Completion after $iter->current()->
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 22,
                character: 24,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "ArrayObject::getIterator()->current()-> should resolve through template-extends to Product, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `key-of<T>` and `value-of<T>` type operators evaluate after template
/// substitution through `@extends`, enabling completion on the resolved type.
#[tokio::test]
async fn test_key_of_and_value_of_type_operators() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///key_of_test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TData as array\n",
        " */\n",
        "class DataBag {\n",
        "    /** @var TData */\n",
        "    protected $data;\n",
        "    /** @param TData $data */\n",
        "    public function __construct(array $data) { $this->data = $data; }\n",
        "    /** @return value-of<TData> */\n",
        "    public function firstValue() { return reset($this->data); }\n",
        "}\n",
        "\n",
        "class User {\n",
        "    public function getName(): string {}\n",
        "}\n",
        "\n",
        "/** @extends DataBag<array{name: string, user: User}> */\n",
        "class MyBag extends DataBag {}\n",
        "\n",
        "function demo() {\n",
        "    $bag = new MyBag(['name' => 'test', 'user' => new User()]);\n",
        "    $bag->firstValue()->\n",
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

    // Line 22: `    $bag->firstValue()->`
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 22,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "value-of<TData> should evaluate to string|User after @extends substitution, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test that @template on @method tags resolves at call sites.
/// `@method TVal doThing<TVal of mixed>(TVal $param)` should infer TVal from the argument.
#[tokio::test]
async fn test_method_tag_template_resolves_at_call_site() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_tag_template.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "\n",
        "/**\n",
        " * @method TVal get<TVal of mixed>(TVal $default)\n",
        " */\n",
        "class Config {\n",
        "    public function __call(string $name, array $args): mixed { return null; }\n",
        "}\n",
        "\n",
        "function test() {\n",
        "    $config = new Config();\n",
        "    $user = new User();\n",
        "    $result = $config->get($user);\n",
        "    $result->\n",
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

    // Cursor after `$result->` on line 17
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 17,
                character: 13,
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
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should resolve TVal to User and show User's 'getName' method, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getEmail"),
                "Should resolve TVal to User and show User's 'getEmail' method, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}
