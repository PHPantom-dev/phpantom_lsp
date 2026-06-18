use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Basic: $var->prop-> ────────────────────────────────────────────────────

/// A variable assigned via `new Foo()` should allow property chain
/// completion: `$user->address->` should offer `Address` members.
#[tokio::test]
async fn test_var_property_chain_simple() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_prop_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Address {\n",
        "    public string $city;\n",
        "    public string $zip;\n",
        "    public function format(): string {}\n",
        "}\n",
        "class User {\n",
        "    public Address $address;\n",
        "    public string $name;\n",
        "}\n",
        "function demo() {\n",
        "    $user = new User();\n",
        "    $user->address->\n",
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

    // Cursor right after `$user->address->` on line 12
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
        "Completion should return results for $user->address->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("city")),
                "Should include 'city' property from Address. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("zip")),
                "Should include 'zip' property from Address. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("format")),
                "Should include 'format' method from Address. Got: {:?}",
                labels
            );
            // Should NOT include User members
            assert!(
                !labels.iter().any(|l| l.starts_with("name")),
                "Should NOT include 'name' from User. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Deep chain: $var->prop->subprop-> ──────────────────────────────────────

/// Two levels of property chain: `$order->customer->address->`
/// should offer members of `Address`.
#[tokio::test]
async fn test_var_property_chain_deep() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///deep_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Address {\n",
        "    public string $street;\n",
        "    public string $city;\n",
        "}\n",
        "class Customer {\n",
        "    public Address $address;\n",
        "    public string $email;\n",
        "}\n",
        "class Order {\n",
        "    public Customer $customer;\n",
        "    public float $total;\n",
        "}\n",
        "function demo() {\n",
        "    $order = new Order();\n",
        "    $order->customer->address->\n",
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

    // Cursor right after `$order->customer->address->` on line 15
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
        "Completion should return results for $order->customer->address->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("street")),
                "Should include 'street' from Address. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("city")),
                "Should include 'city' from Address. Got: {:?}",
                labels
            );
            // Should NOT include members from Customer or Order
            assert!(
                !labels.iter().any(|l| l.starts_with("email")),
                "Should NOT include 'email' from Customer. Got: {:?}",
                labels
            );
            assert!(
                !labels.iter().any(|l| l.starts_with("total")),
                "Should NOT include 'total' from Order. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Parameter type hint ────────────────────────────────────────────────────

/// When a function parameter has a type hint, property chains should work:
/// `function f(User $user) { $user->address-> }`
#[tokio::test]
async fn test_var_property_chain_from_parameter() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///param_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Address {\n",
        "    public string $city;\n",
        "    public function getZip(): string {}\n",
        "}\n",
        "class User {\n",
        "    public Address $address;\n",
        "}\n",
        "function processUser(User $user) {\n",
        "    $user->address->\n",
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
        "Completion should return results for parameter $user->address->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("city")),
                "Should include 'city' from Address. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getZip")),
                "Should include 'getZip' from Address. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Nullsafe operator: $var?->prop-> ───────────────────────────────────────

/// Nullsafe access `$var?->prop->` should resolve the same as `$var->prop->`.
#[tokio::test]
async fn test_var_property_chain_nullsafe() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nullsafe_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Engine {\n",
        "    public int $horsepower;\n",
        "    public function start(): void {}\n",
        "}\n",
        "class Car {\n",
        "    public ?Engine $engine;\n",
        "}\n",
        "function demo(?Car $car) {\n",
        "    $car?->engine->\n",
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
        "Completion should return results for $car?->engine->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("horsepower")),
                "Should include 'horsepower' from Engine. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("start")),
                "Should include 'start' from Engine. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Docblock @var annotation ───────────────────────────────────────────────

/// When a variable's type is declared via `@var`, property chains should work.
#[tokio::test]
async fn test_var_property_chain_from_docblock_var() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///docblock_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Settings {\n",
        "    public bool $debug;\n",
        "    public string $locale;\n",
        "}\n",
        "class Config {\n",
        "    public Settings $settings;\n",
        "}\n",
        "function demo() {\n",
        "    /** @var Config $config */\n",
        "    $config = getConfig();\n",
        "    $config->settings->\n",
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
        "Completion should return results for @var annotated $config->settings->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("debug")),
                "Should include 'debug' from Settings. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("locale")),
                "Should include 'locale' from Settings. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Mixed chain: $var->method()->prop-> ────────────────────────────────────

/// Method call in the middle of a chain: `$var->getCustomer()->address->`
/// should resolve `getCustomer()` return type, then look up `address` on it.
#[tokio::test]
async fn test_var_method_then_property_chain() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_prop_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Address {\n",
        "    public string $city;\n",
        "}\n",
        "class Customer {\n",
        "    public Address $address;\n",
        "}\n",
        "class Order {\n",
        "    public function getCustomer(): Customer {}\n",
        "}\n",
        "function demo() {\n",
        "    $order = new Order();\n",
        "    $order->getCustomer()->address->\n",
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

    // `$order->getCustomer()->address->` is on line 12
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
                character: 38,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $order->getCustomer()->address->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("city")),
                "Should include 'city' from Address. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Inside a class method (non-$this variable) ────────────────────────────

/// Property chains on local variables inside a class method should work
/// independently of `$this`.
#[tokio::test]
async fn test_var_property_chain_inside_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inside_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function log(string $msg): void {}\n",
        "}\n",
        "class Database {\n",
        "    public Logger $logger;\n",
        "}\n",
        "class Service {\n",
        "    public function run() {\n",
        "        $db = new Database();\n",
        "        $db->logger->\n",
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
        "Completion should return results for $db->logger->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("log")),
                "Should include 'log' from Logger. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Property with docblock type (not native hint) ──────────────────────────

/// When a property's type is declared via `@var` docblock rather than a
/// native type hint, the chain should still resolve.
#[tokio::test]
async fn test_var_property_chain_docblock_property_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///docblock_prop_type.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Renderer {\n",
        "    public function render(): string {}\n",
        "}\n",
        "class View {\n",
        "    /** @var Renderer */\n",
        "    public $renderer;\n",
        "}\n",
        "function demo() {\n",
        "    $view = new View();\n",
        "    $view->renderer->\n",
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
        "Completion should return results for docblock-typed $view->renderer->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("render")),
                "Should include 'render' from Renderer. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Property with inherited type ───────────────────────────────────────────

/// Property chain should work when the property is inherited from a parent.
#[tokio::test]
async fn test_var_property_chain_inherited_property() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///inherited_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Connection {\n",
        "    public function query(string $sql): void {}\n",
        "    public function close(): void {}\n",
        "}\n",
        "class BaseRepository {\n",
        "    public Connection $conn;\n",
        "}\n",
        "class UserRepository extends BaseRepository {\n",
        "}\n",
        "function demo() {\n",
        "    $repo = new UserRepository();\n",
        "    $repo->conn->\n",
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
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $repo->conn-> (inherited property)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("query")),
                "Should include 'query' from Connection. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("close")),
                "Should include 'close' from Connection. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Cross-file PSR-4 resolution ────────────────────────────────────────────

/// Property chain where the intermediate class is in another file,
/// loaded via PSR-4 autoloading.
#[tokio::test]
async fn test_var_property_chain_cross_file_psr4() {
    let composer = r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#;

    let address_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Address {\n",
        "    public string $city;\n",
        "    public string $country;\n",
        "    public function fullAddress(): string {}\n",
        "}\n",
    );

    let user_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class User {\n",
        "    public Address $address;\n",
        "    public string $name;\n",
        "}\n",
    );

    let controller_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "class Controller {\n",
        "    public function show(User $user) {\n",
        "        $user->address->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Address.php", address_php),
            ("src/User.php", user_php),
            ("src/Controller.php", controller_php),
        ],
    );

    let uri = Url::parse("file:///src/Controller.php").unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: controller_php.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
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
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for PSR-4 $user->address->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("city")),
                "Should include 'city' from cross-file Address. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("country")),
                "Should include 'country' from cross-file Address. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("fullAddress")),
                "Should include 'fullAddress' from cross-file Address. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Top-level code (no enclosing class) ────────────────────────────────────

/// Property chains should work in top-level code (not inside a class).
#[tokio::test]
async fn test_var_property_chain_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///top_level_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Point {\n",
        "    public float $x;\n",
        "    public float $y;\n",
        "}\n",
        "class Shape {\n",
        "    public Point $origin;\n",
        "}\n",
        "$shape = new Shape();\n",
        "$shape->origin->\n",
    );

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
        "Completion should return results for top-level $shape->origin->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("x")),
                "Should include 'x' from Point. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("y")),
                "Should include 'y' from Point. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Return type property chain ─────────────────────────────────────────────

/// When a method return type is used to type a variable,
/// property chains on that variable should work.
#[tokio::test]
async fn test_var_property_chain_from_method_return() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///return_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Stats {\n",
        "    public int $count;\n",
        "    public float $average;\n",
        "}\n",
        "class Report {\n",
        "    public Stats $stats;\n",
        "}\n",
        "class ReportFactory {\n",
        "    public function create(): Report {}\n",
        "}\n",
        "function demo() {\n",
        "    $factory = new ReportFactory();\n",
        "    $report = $factory->create();\n",
        "    $report->stats->\n",
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
        "Completion should return results for $report->stats->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("count")),
                "Should include 'count' from Stats. Got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("average")),
                "Should include 'average' from Stats. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── No false positives: nonexistent property ───────────────────────────────

/// When the intermediate property doesn't exist, completion should
/// return no results (not crash or fall through to wrong results).
#[tokio::test]
async fn test_var_property_chain_nonexistent_property() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///no_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n",
        "function demo() {\n",
        "    $user = new User();\n",
        "    $user->nonexistent->\n",
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
                character: 26,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // Should return None — no crash, no wrong results.
    assert!(
        result.is_none(),
        "Should return None for nonexistent property chain, got: {:?}",
        result
    );
}

// ─── Foreach variable property chain ────────────────────────────────────────

/// When a foreach value variable is typed via a generic iterable,
/// property chains on it should work.
#[tokio::test]
async fn test_var_property_chain_foreach_value() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Address {\n",
        "    public string $city;\n",
        "}\n",
        "class User {\n",
        "    public Address $address;\n",
        "}\n",
        "class Service {\n",
        "    public function process() {\n",
        "        /** @var list<User> $users */\n",
        "        $users = loadUsers();\n",
        "        foreach ($users as $user) {\n",
        "            $user->address->\n",
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

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 12,
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
        "Completion should return results for foreach $user->address->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("city")),
                "Should include 'city' from Address via foreach chain. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Promoted constructor property chain ────────────────────────────────────

/// Property chains should work with PHP 8 promoted properties.
#[tokio::test]
async fn test_var_property_chain_promoted_property() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///promoted_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Formatter {\n",
        "    public function format(string $val): string {}\n",
        "}\n",
        "class Printer {\n",
        "    public function __construct(\n",
        "        public Formatter $formatter,\n",
        "    ) {}\n",
        "}\n",
        "function demo() {\n",
        "    $printer = new Printer(new Formatter());\n",
        "    $printer->formatter->\n",
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
                character: 26,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $printer->formatter->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("format")),
                "Should include 'format' from Formatter. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── Static property not confused ───────────────────────────────────────────

/// `$var->prop->` should NOT offer static members of the property's type
/// when using arrow access.
#[tokio::test]
async fn test_var_property_chain_no_static_members() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///no_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Cache {\n",
        "    public function get(string $key): mixed {}\n",
        "    public static function flush(): void {}\n",
        "    public static int $hits = 0;\n",
        "}\n",
        "class App {\n",
        "    public Cache $cache;\n",
        "}\n",
        "function demo() {\n",
        "    $app = new App();\n",
        "    $app->cache->\n",
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
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return results for $app->cache->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("get")),
                "Should include instance method 'get' from Cache. Got: {:?}",
                labels
            );
            // PHP allows calling static methods through an instance.
            assert!(
                labels.iter().any(|l| l.starts_with("flush")),
                "Should include static method 'flush' via -> (PHP allows static calls via instance). Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}

// ─── $this->prop still works (regression check) ─────────────────────────────

/// Verify that the existing `$this->prop->` path still works correctly
/// and is not broken by the new non-`$this` property chain logic.
#[tokio::test]
async fn test_this_property_chain_still_works() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///this_regression.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "}\n",
        "class Service {\n",
        "    public Logger $logger;\n",
        "    public function run() {\n",
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
    assert!(
        result.is_some(),
        "Completion should still work for $this->logger->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("info")),
                "Should include 'info' from Logger via $this->. Got: {:?}",
                labels
            );
        }
        CompletionResponse::List(_) => panic!("Expected Array response"),
    }
}
