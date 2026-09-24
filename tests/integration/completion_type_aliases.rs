use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helper ─────────────────────────────────────────────────────────────────

/// Extract completion item labels filtered by kind.
fn labels_by_kind(items: &[CompletionItem], kind: CompletionItemKind) -> Vec<&str> {
    items
        .iter()
        .filter(|i| i.kind == Some(kind))
        .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
        .collect()
}

// ─── @phpstan-type: array shape alias ───────────────────────────────────────

/// A method returning a `@phpstan-type` alias that resolves to an array shape
/// should offer the shape's keys on `['` completion.
#[tokio::test]
async fn test_phpstan_type_array_shape_key_completion() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///type_alias_shape.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @phpstan-type UserData array{name: string, email: string, age: int}\n",
        " */\n",
        "class UserService {\n",
        "    /** @return UserData */\n",
        "    public function getData(): array {\n",
        "        return ['name' => '', 'email' => '', 'age' => 0];\n",
        "    }\n",
        "    public function run(): void {\n",
        "        $data = $this->getData();\n",
        "        $data['\n",
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

    assert!(
        result.is_some(),
        "Should return array key completions for @phpstan-type alias"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let keys: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                keys.contains(&"name"),
                "Should offer 'name' key, got: {:?}",
                keys
            );
            assert!(
                keys.contains(&"email"),
                "Should offer 'email' key, got: {:?}",
                keys
            );
            assert!(
                keys.contains(&"age"),
                "Should offer 'age' key, got: {:?}",
                keys
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-type: class alias → member completion ─────────────────────────

/// A method returning a `@phpstan-type` alias that resolves to a class name
/// should offer that class's members on `->` completion.
#[tokio::test]
async fn test_phpstan_type_class_alias_member_completion() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///type_alias_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @phpstan-type ActiveUser User\n",
        " */\n",
        "class UserRepo {\n",
        "    /** @return ActiveUser */\n",
        "    public function findActive(): object {\n",
        "        return new User();\n",
        "    }\n",
        "    public function run(): void {\n",
        "        $u = $this->findActive();\n",
        "        $u->\n",
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
        "Should return class member completions for @phpstan-type alias"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let methods = labels_by_kind(&items, CompletionItemKind::METHOD);
            assert!(
                methods.contains(&"getName"),
                "Should include User::getName(), got: {:?}",
                methods
            );
            assert!(
                methods.contains(&"getEmail"),
                "Should include User::getEmail(), got: {:?}",
                methods
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @psalm-type: equivalent to @phpstan-type ───────────────────────────────

/// `@psalm-type` should work identically to `@phpstan-type`.
#[tokio::test]
async fn test_psalm_type_alias_works() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///psalm_type.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "}\n",
        "/**\n",
        " * @psalm-type OrderItem Order\n",
        " */\n",
        "class OrderService {\n",
        "    /** @return OrderItem */\n",
        "    public function current(): object {\n",
        "        return new Order();\n",
        "    }\n",
        "    public function run(): void {\n",
        "        $item = $this->current();\n",
        "        $item->\n",
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

    assert!(
        result.is_some(),
        "Should return completions for @psalm-type alias"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let methods = labels_by_kind(&items, CompletionItemKind::METHOD);
            assert!(
                methods.contains(&"getTotal"),
                "Should include Order::getTotal(), got: {:?}",
                methods
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── Chained named @psalm-type aliases ──────────────────────────────────────

/// Ported from Mago issue_1116.php: three separately *named* `@psalm-type`
/// aliases (not one inline nested shape) chain into each other by name —
/// `Bar` references `Baz`, `Baz` references `Qux` — and one leaf alias is
/// `@psalm-import-type`'d from an unrelated class. Array key completion
/// must resolve each named hop in turn: `$val['baz']` through the `Bar`
/// alias to find `Baz`, then `['qux']` through `Baz` to find `Qux`.
#[tokio::test]
async fn test_psalm_type_chained_named_aliases_nested_key_completion() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///chained_named_aliases.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @psalm-type FooValueType = int\n",
        " */\n",
        "final class Foo {\n",
        "}\n",
        "/**\n",
        " * @psalm-type Bar = array{'baz': Baz}\n",
        " * @psalm-type Baz = array{'qux': Qux}\n",
        " * @psalm-type Qux = array{'foo': FooValueType}\n",
        " *\n",
        " * @psalm-import-type FooValueType from Foo\n",
        " */\n",
        "class BarService {\n",
        "    /** @param Bar $val */\n",
        "    public function run(mixed $val): void {\n",
        "        $val['baz']['qux']['\n",
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

    // `$val['baz']['qux']['` on line 16
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

    assert!(
        result.is_some(),
        "Should return array key completions through chained named aliases"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let keys = labels_by_kind(&items, CompletionItemKind::FIELD);
            assert!(
                keys.contains(&"foo"),
                "Should include 'foo' from the Qux alias at the end of the chain, got: {:?}",
                keys
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-type with `=` separator ───────────────────────────────────────

/// Both `@phpstan-type Name Definition` and `@phpstan-type Name = Definition`
/// formats should work.
#[tokio::test]
async fn test_phpstan_type_with_equals_separator() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///type_alias_eq.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @phpstan-type Config = array{host: string, port: int}\n",
        " */\n",
        "class DbConnection {\n",
        "    /** @return Config */\n",
        "    public function getConfig(): array {\n",
        "        return ['host' => '', 'port' => 0];\n",
        "    }\n",
        "    public function run(): void {\n",
        "        $cfg = $this->getConfig();\n",
        "        $cfg['\n",
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

    assert!(
        result.is_some(),
        "Should return array key completions for @phpstan-type alias with '=' separator"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let keys: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                keys.contains(&"host"),
                "Should offer 'host' key, got: {:?}",
                keys
            );
            assert!(
                keys.contains(&"port"),
                "Should offer 'port' key, got: {:?}",
                keys
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-type: union type alias ────────────────────────────────────────

/// A `@phpstan-type` alias that expands to a union of class names should
/// offer members from all union components.
#[tokio::test]
async fn test_phpstan_type_union_alias() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///type_alias_union.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "/**\n",
        " * @phpstan-type Pet Cat|Dog\n",
        " */\n",
        "class PetShop {\n",
        "    /** @return Pet */\n",
        "    public function adopt(): object {\n",
        "        return new Cat();\n",
        "    }\n",
        "    public function run(): void {\n",
        "        $pet = $this->adopt();\n",
        "        $pet->\n",
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
                    line: 17,
                    character: 14,
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
        "Should return completions for union @phpstan-type alias"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let methods = labels_by_kind(&items, CompletionItemKind::METHOD);
            assert!(
                methods.contains(&"purr"),
                "Should include Cat::purr(), got: {:?}",
                methods
            );
            assert!(
                methods.contains(&"bark"),
                "Should include Dog::bark(), got: {:?}",
                methods
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-type used in @param ───────────────────────────────────────────

/// A `@phpstan-type` alias used in a `@param` annotation should resolve
/// correctly for parameter type resolution.
#[tokio::test]
async fn test_phpstan_type_alias_in_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///type_alias_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Invoice {\n",
        "    public function getAmount(): float { return 0.0; }\n",
        "}\n",
        "/**\n",
        " * @phpstan-type BillableItem Invoice\n",
        " */\n",
        "class BillingService {\n",
        "    /**\n",
        "     * @param BillableItem $item\n",
        "     */\n",
        "    public function process($item): void {\n",
        "        $item->\n",
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

    assert!(
        result.is_some(),
        "Should return completions for @param with @phpstan-type alias"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let methods = labels_by_kind(&items, CompletionItemKind::METHOD);
            assert!(
                methods.contains(&"getAmount"),
                "Should include Invoice::getAmount(), got: {:?}",
                methods
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-type: multiple aliases on one class ───────────────────────────

/// A class can define multiple `@phpstan-type` aliases and use them in
/// different methods.
#[tokio::test]
async fn test_phpstan_type_multiple_aliases() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///multi_alias.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @phpstan-type RequestData array{method: string, uri: string}\n",
        " * @phpstan-type ResponseData array{status: int, body: string}\n",
        " */\n",
        "class HttpClient {\n",
        "    /** @return ResponseData */\n",
        "    public function send(): array { return []; }\n",
        "    public function run(): void {\n",
        "        $resp = $this->send();\n",
        "        $resp['\n",
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

    assert!(
        result.is_some(),
        "Should return keys from ResponseData (not RequestData)"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let keys: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                keys.contains(&"status"),
                "Should offer 'status' key from ResponseData, got: {:?}",
                keys
            );
            assert!(
                keys.contains(&"body"),
                "Should offer 'body' key from ResponseData, got: {:?}",
                keys
            );
            // RequestData keys should NOT appear
            assert!(
                !keys.contains(&"method"),
                "Should NOT offer 'method' from RequestData, got: {:?}",
                keys
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-import-type: same-file import ─────────────────────────────────

/// `@phpstan-import-type` imports a type alias from another class.
#[tokio::test]
async fn test_phpstan_import_type_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///import_type.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Address {\n",
        "    public function getCity(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @phpstan-type Location Address\n",
        " */\n",
        "class GeoService {\n",
        "}\n",
        "/**\n",
        " * @phpstan-import-type Location from GeoService\n",
        " */\n",
        "class MapRenderer {\n",
        "    /** @return Location */\n",
        "    public function getCenter(): object { return new Address(); }\n",
        "    public function run(): void {\n",
        "        $loc = $this->getCenter();\n",
        "        $loc->\n",
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
                    line: 17,
                    character: 14,
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
        "Should return completions for @phpstan-import-type alias"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let methods = labels_by_kind(&items, CompletionItemKind::METHOD);
            assert!(
                methods.contains(&"getCity"),
                "Should include Address::getCity() via imported Location alias, got: {:?}",
                methods
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-import-type combined with @template-implements ───────────────

/// Ported from Mago issue_1040.php: a generic interface's method parameter
/// is typed by the interface's own template (`TConfiguration`); the
/// implementing class satisfies that template via `@template-implements`
/// with an alias imported (`@phpstan-import-type`) from a third class,
/// and does not redeclare the shape on its own override. Array key
/// completion on the parameter inside the *implementing* method's body
/// must chase: template-implements substitution -> imported alias ->
/// array shape, none of which is written on the override itself.
#[tokio::test]
async fn test_phpstan_import_type_via_template_implements() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///import_type_template_implements.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template TConfiguration of array<array-key, mixed>\n",
        " */\n",
        "interface ContainerInterface {\n",
        "    /** @param TConfiguration $configuration */\n",
        "    public function get(array $configuration): string;\n",
        "}\n",
        "/**\n",
        " * @phpstan-type MyConfiguration array{name: string, value: int}\n",
        " */\n",
        "class Calculator {\n",
        "}\n",
        "/**\n",
        " * @phpstan-import-type MyConfiguration from Calculator\n",
        " * @template-implements ContainerInterface<MyConfiguration>\n",
        " */\n",
        "class MyContainer implements ContainerInterface {\n",
        "    public function get(array $configuration): string {\n",
        "        return $configuration['\n",
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

    // `$configuration['` on line 19
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 19,
                    character: 31,
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
        "Should return array key completions through template-implements + imported alias"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let keys = labels_by_kind(&items, CompletionItemKind::FIELD);
            assert!(
                keys.contains(&"name"),
                "Should include 'name' from the imported MyConfiguration shape, got: {:?}",
                keys
            );
            assert!(
                keys.contains(&"value"),
                "Should include 'value' from the imported MyConfiguration shape, got: {:?}",
                keys
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-import-type with `as` rename ──────────────────────────────────

/// `@phpstan-import-type Foo from Bar as Baz` should use `Baz` as the
/// local alias name.
#[tokio::test]
async fn test_phpstan_import_type_with_as_rename() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///import_as.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function getPrice(): float { return 0.0; }\n",
        "}\n",
        "/**\n",
        " * @phpstan-type Item Product\n",
        " */\n",
        "class Catalog {\n",
        "}\n",
        "/**\n",
        " * @phpstan-import-type Item from Catalog as CartItem\n",
        " */\n",
        "class ShoppingCart {\n",
        "    /** @return CartItem */\n",
        "    public function first(): object { return new Product(); }\n",
        "    public function run(): void {\n",
        "        $item = $this->first();\n",
        "        $item->\n",
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
                    line: 17,
                    character: 15,
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
        "Should return completions for @phpstan-import-type with 'as' rename"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let methods = labels_by_kind(&items, CompletionItemKind::METHOD);
            assert!(
                methods.contains(&"getPrice"),
                "Should include Product::getPrice() via CartItem alias, got: {:?}",
                methods
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-import-type: cross-file via PSR-4 ────────────────────────────

/// `@phpstan-import-type` should resolve aliases from classes in other files.
#[tokio::test]
async fn test_phpstan_import_type_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Config.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "/**\n",
                " * @phpstan-type DbConfig array{host: string, port: int, name: string}\n",
                " */\n",
                "class Config {\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///consumer.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Config;\n",
        "/**\n",
        " * @phpstan-import-type DbConfig from Config\n",
        " */\n",
        "class Database {\n",
        "    /** @return DbConfig */\n",
        "    public function getConfig(): array { return []; }\n",
        "    public function run(): void {\n",
        "        $cfg = $this->getConfig();\n",
        "        $cfg['\n",
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
                    character: 14,
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
        "Should return array key completions for cross-file @phpstan-import-type"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let keys: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                keys.contains(&"host"),
                "Should offer 'host' key from imported DbConfig, got: {:?}",
                keys
            );
            assert!(
                keys.contains(&"port"),
                "Should offer 'port' key from imported DbConfig, got: {:?}",
                keys
            );
            assert!(
                keys.contains(&"name"),
                "Should offer 'name' key from imported DbConfig, got: {:?}",
                keys
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-type: object shape alias ──────────────────────────────────────

/// A `@phpstan-type` alias that resolves to an object shape should offer
/// property completions via `->`.
#[tokio::test]
async fn test_phpstan_type_object_shape_alias() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///type_alias_objshape.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @phpstan-type Point object{x: float, y: float}\n",
        " */\n",
        "class Canvas {\n",
        "    /** @return Point */\n",
        "    public function getOrigin(): object {\n",
        "        return (object)['x' => 0.0, 'y' => 0.0];\n",
        "    }\n",
        "    public function run(): void {\n",
        "        $p = $this->getOrigin();\n",
        "        $p->\n",
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
        "Should return property completions for object shape @phpstan-type alias"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let props = labels_by_kind(&items, CompletionItemKind::PROPERTY);
            assert!(
                props.contains(&"x"),
                "Should include 'x' property, got: {:?}",
                props
            );
            assert!(
                props.contains(&"y"),
                "Should include 'y' property, got: {:?}",
                props
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-type on a different class in the same file ────────────────────

/// Type aliases should be found even when defined on a different class in the
/// same file (not the owning class of the method).
#[tokio::test]
async fn test_phpstan_type_from_different_class_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///alias_other_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @phpstan-type Renderable Widget\n",
        " */\n",
        "class WidgetFactory {\n",
        "}\n",
        "class Dashboard {\n",
        "    /** @return Renderable */\n",
        "    public function getWidget(): object { return new Widget(); }\n",
        "    public function run(): void {\n",
        "        $w = $this->getWidget();\n",
        "        $w->\n",
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
        "Should return completions from alias on different class in same file"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let methods = labels_by_kind(&items, CompletionItemKind::METHOD);
            assert!(
                methods.contains(&"render"),
                "Should include Widget::render() via Renderable alias, got: {:?}",
                methods
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── @phpstan-type used in @var annotation ──────────────────────────────────

/// A `@phpstan-type` alias used in an inline `@var` annotation should resolve.
#[tokio::test]
async fn test_phpstan_type_alias_in_var_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///alias_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "}\n",
        "/**\n",
        " * @phpstan-type LoggerInstance Logger\n",
        " */\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        /** @var LoggerInstance $log */\n",
        "        $log = getLogger();\n",
        "        $log->\n",
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

    assert!(
        result.is_some(),
        "Should return completions for @var with @phpstan-type alias"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let methods = labels_by_kind(&items, CompletionItemKind::METHOD);
            assert!(
                methods.contains(&"info"),
                "Should include Logger::info() via LoggerInstance alias, got: {:?}",
                methods
            );
        }
        other => panic!("Expected CompletionResponse::Array, got: {:?}", other),
    }
}

// ─── Docblock parsing unit tests ────────────────────────────────────────────

#[cfg(test)]
mod docblock_parsing {
    use phpantom_lsp::docblock::extract_type_aliases;
    use phpantom_lsp::types::TypeAliasDef;

    /// Helper: assert a local alias has the expected type string representation.
    fn assert_local_alias(
        aliases: &std::collections::HashMap<String, TypeAliasDef>,
        name: &str,
        expected: &str,
    ) {
        match aliases.get(name) {
            Some(TypeAliasDef::Local(php_type)) => {
                assert_eq!(
                    php_type.to_string(),
                    expected,
                    "alias '{name}' type mismatch"
                );
            }
            Some(TypeAliasDef::Import { .. }) => {
                panic!("expected Local alias for '{name}', got Import");
            }
            None => panic!("alias '{name}' not found"),
        }
    }

    /// Helper: assert an imported alias has the expected source class and original name.
    fn assert_import_alias(
        aliases: &std::collections::HashMap<String, TypeAliasDef>,
        name: &str,
        expected_source: &str,
        expected_original: &str,
    ) {
        match aliases.get(name) {
            Some(TypeAliasDef::Import {
                source_class,
                original_name,
            }) => {
                assert_eq!(
                    source_class, expected_source,
                    "alias '{name}' source_class mismatch"
                );
                assert_eq!(
                    original_name, expected_original,
                    "alias '{name}' original_name mismatch"
                );
            }
            Some(TypeAliasDef::Local(_)) => {
                panic!("expected Import alias for '{name}', got Local");
            }
            None => panic!("alias '{name}' not found"),
        }
    }

    #[test]
    fn test_extract_phpstan_type_basic() {
        let doc = "/**\n * @phpstan-type UserData array{name: string, email: string}\n */";
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 1);
        assert_local_alias(&aliases, "UserData", "array{name: string, email: string}");
    }

    #[test]
    fn test_extract_phpstan_type_with_equals() {
        let doc = "/**\n * @phpstan-type Config = array{host: string, port: int}\n */";
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 1);
        assert_local_alias(&aliases, "Config", "array{host: string, port: int}");
    }

    #[test]
    fn test_extract_psalm_type() {
        let doc = "/**\n * @psalm-type StatusCode int\n */";
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 1);
        assert_local_alias(&aliases, "StatusCode", "int");
    }

    #[test]
    fn test_extract_multiple_aliases() {
        let doc = concat!(
            "/**\n",
            " * @phpstan-type Foo array{a: int}\n",
            " * @phpstan-type Bar array{b: string}\n",
            " */",
        );
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 2);
        assert_local_alias(&aliases, "Foo", "array{a: int}");
        assert_local_alias(&aliases, "Bar", "array{b: string}");
    }

    #[test]
    fn test_extract_import_type_basic() {
        let doc = "/**\n * @phpstan-import-type UserData from UserService\n */";
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 1);
        assert_import_alias(&aliases, "UserData", "UserService", "UserData");
    }

    #[test]
    fn test_extract_import_type_with_as() {
        let doc = "/**\n * @phpstan-import-type UserData from UserService as UserRecord\n */";
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 1);
        // The local alias is "UserRecord", not "UserData"
        assert!(!aliases.contains_key("UserData"));
        assert_import_alias(&aliases, "UserRecord", "UserService", "UserData");
    }

    #[test]
    fn test_extract_psalm_import_type() {
        let doc = "/**\n * @psalm-import-type Config from AppConfig\n */";
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 1);
        assert_import_alias(&aliases, "Config", "AppConfig", "Config");
    }

    #[test]
    fn test_extract_mixed_local_and_imported() {
        let doc = concat!(
            "/**\n",
            " * @phpstan-type LocalAlias array{x: int}\n",
            " * @phpstan-import-type RemoteAlias from OtherClass\n",
            " */",
        );
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 2);
        assert_local_alias(&aliases, "LocalAlias", "array{x: int}");
        assert_import_alias(&aliases, "RemoteAlias", "OtherClass", "RemoteAlias");
    }

    #[test]
    fn test_extract_complex_type_alias() {
        let doc = "/**\n * @phpstan-type Callback callable(string, int): bool\n */";
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 1);
        assert_local_alias(&aliases, "Callback", "callable(string, int): bool");
    }

    #[test]
    fn test_extract_union_type_alias() {
        let doc = "/**\n * @phpstan-type StringOrInt string|int\n */";
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 1);
        assert_local_alias(&aliases, "StringOrInt", "string|int");
    }

    #[test]
    fn test_empty_docblock() {
        let doc = "/** */";
        let aliases = extract_type_aliases(doc);
        assert!(aliases.is_empty());
    }

    #[test]
    fn test_no_type_aliases() {
        let doc = concat!(
            "/**\n",
            " * @param string $name\n",
            " * @return void\n",
            " */",
        );
        let aliases = extract_type_aliases(doc);
        assert!(aliases.is_empty());
    }

    #[test]
    fn test_phpstan_type_not_confused_with_other_tags() {
        // `@phpstan-type-alias` (hypothetical) should not match
        // because the parser checks that nothing follows `@phpstan-type`
        // except whitespace.
        let doc = "/**\n * @phpstan-type-alias Foo string\n */";
        let aliases = extract_type_aliases(doc);
        assert!(aliases.is_empty(), "Should not match @phpstan-type-alias");
    }

    #[test]
    fn test_object_shape_alias() {
        let doc = "/**\n * @phpstan-type Point object{x: float, y: float}\n */";
        let aliases = extract_type_aliases(doc);
        assert_eq!(aliases.len(), 1);
        assert_local_alias(&aliases, "Point", "object{x: float, y: float}");
    }
}
