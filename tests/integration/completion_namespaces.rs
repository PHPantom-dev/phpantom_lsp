use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_completion_inside_namespaced_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///namespaced.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "\n",
        "class User {\n",
        "    public function login() {}\n",
        "    public function logout() {}\n",
        "    public function test() {\n",
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
    assert!(
        result.is_some(),
        "Completion should return results for namespaced class"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_items: Vec<&CompletionItem> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .collect();
            assert_eq!(method_items.len(), 3, "Should return 3 method completions");

            let filter_texts: Vec<&str> = method_items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(filter_texts.contains(&"login"), "Should contain 'login'");
            assert!(filter_texts.contains(&"logout"), "Should contain 'logout'");

            for item in &method_items {
                assert_eq!(item.kind, Some(CompletionItemKind::METHOD));
            }
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_namespaced_class_with_properties_and_methods() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///ns_full.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Entity;\n",
        "\n",
        "class Product {\n",
        "    public string $name;\n",
        "    public float $price;\n",
        "    public function getName(): string {}\n",
        "    public function setPrice(float $price): void {}\n",
        "    public function test() {\n",
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
    match result.unwrap() {
        CompletionResponse::Array(items) => {
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

            // Check method insert texts
            let get_name = method_items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("getName"))
                .unwrap();
            assert_eq!(get_name.insert_text.as_deref(), Some("getName()$0"));
            assert_eq!(get_name.insert_text_format, Some(InsertTextFormat::SNIPPET));
            assert_eq!(get_name.label, "getName()");

            let set_price = method_items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("setPrice"))
                .unwrap();
            assert_eq!(
                set_price.insert_text.as_deref(),
                Some("setPrice(${1:\\$price})$0")
            );
            assert_eq!(
                set_price.insert_text_format,
                Some(InsertTextFormat::SNIPPET)
            );
            assert_eq!(set_price.label, "setPrice($price)");

            // Check property labels
            let prop_labels: Vec<&str> = property_items.iter().map(|i| i.label.as_str()).collect();
            assert!(prop_labels.contains(&"name"));
            assert!(prop_labels.contains(&"price"));
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Use-As Alias Resolution ────────────────────────────────────────────────

/// `use Swagger\OpenAPI as OA;` followed by `new OA\Endpoint()` should
/// resolve `OA\Endpoint` to `Swagger\OpenAPI\Endpoint` and offer its
/// members for completion.
#[tokio::test]
async fn test_completion_use_as_alias_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///use_alias.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "namespace Swagger\\OpenAPI;\n",                            // 1
        "\n",                                                       // 2
        "class Endpoint {\n",                                       // 3
        "    public function getPath(): string { return ''; }\n",   // 4
        "    public function getMethod(): string { return ''; }\n", // 5
        "}\n",                                                      // 6
    );

    let consumer_uri = Url::parse("file:///consumer.php").unwrap();
    let consumer_text = concat!(
        "<?php\n",                                   // 0
        "use Swagger\\OpenAPI as OA;\n",             // 1
        "\n",                                        // 2
        "class App {\n",                             // 3
        "    public function run(): void {\n",       // 4
        "        $endpoint = new OA\\Endpoint();\n", // 5
        "        $endpoint->\n",                     // 6
        "    }\n",                                   // 7
        "}\n",                                       // 8
    );

    // Open both files so the class is in the AST map
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

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: consumer_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: consumer_text.to_string(),
            },
        })
        .await;

    // Cursor after `$endpoint->` on line 6
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: consumer_uri },
                position: Position {
                    line: 6,
                    character: 20,
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
        "Should return completions for OA\\Endpoint resolved via use-as alias"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("getPath")),
                "Should include getPath from Endpoint, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getMethod")),
                "Should include getMethod from Endpoint, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file PSR-4: `use App\Services as Svc;` alias resolving to a class
/// loaded from a PSR-4 mapped file.
#[tokio::test]
async fn test_completion_use_as_alias_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\Services\\": "src/Services/"
            }
        }
    }"#;

    let service_content = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "\n",
        "class PaymentGateway {\n",
        "    public function charge(int $amount): bool { return true; }\n",
        "    public function refund(int $amount): bool { return true; }\n",
        "}\n",
    );

    let consumer_content = concat!(
        "<?php\n",                            // 0
        "use App\\Services as Svc;\n",        // 1
        "\n",                                 // 2
        "$gw = new Svc\\PaymentGateway();\n", // 3
        "$gw->\n",                            // 4
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[("src/Services/PaymentGateway.php", service_content)],
    );

    let uri = Url::parse("file:///consumer_psr4.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: consumer_content.to_string(),
            },
        })
        .await;

    // Cursor after `$gw->` on line 4
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 4,
                    character: 5,
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
        "Should return completions for Svc\\PaymentGateway resolved via use-as alias + PSR-4"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("charge")),
                "Should include charge() from PaymentGateway, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("refund")),
                "Should include refund() from PaymentGateway, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `use Foo\Bar as FB;` with a class alias (not namespace alias).
/// `$x = new FB();` should resolve to `Foo\Bar` and offer its members.
#[tokio::test]
async fn test_completion_use_as_class_alias() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///class_alias_def.php").unwrap();
    let text = concat!(
        "<?php\n",                                                  // 0
        "namespace Foo;\n",                                         // 1
        "\n",                                                       // 2
        "class Bar {\n",                                            // 3
        "    public function doWork(): void {}\n",                  // 4
        "    public function getStatus(): string { return ''; }\n", // 5
        "}\n",                                                      // 6
    );

    let consumer_uri = Url::parse("file:///class_alias_use.php").unwrap();
    let consumer_text = concat!(
        "<?php\n",               // 0
        "use Foo\\Bar as FB;\n", // 1
        "\n",                    // 2
        "$x = new FB();\n",      // 3
        "$x->\n",                // 4
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

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: consumer_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: consumer_text.to_string(),
            },
        })
        .await;

    // Cursor after `$x->` on line 4
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: consumer_uri },
                position: Position {
                    line: 4,
                    character: 4,
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
        "Should return completions for FB resolved via use-as class alias"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("doWork")),
                "Should include doWork from Bar, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("getStatus")),
                "Should include getStatus from Bar, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Go-to-definition on a method accessed via an aliased namespace should
/// resolve correctly.
#[tokio::test]
async fn test_goto_definition_use_as_alias() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///alias_goto_def.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "namespace Vendor\\Lib;\n",                 // 1
        "\n",                                       // 2
        "class Client {\n",                         // 3
        "    public function request(): void {}\n", // 4
        "}\n",                                      // 5
    );

    let consumer_uri = Url::parse("file:///alias_goto_consumer.php").unwrap();
    let consumer_text = concat!(
        "<?php\n",                  // 0
        "use Vendor\\Lib as VL;\n", // 1
        "\n",                       // 2
        "$c = new VL\\Client();\n", // 3
        "$c->request();\n",         // 4
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

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: consumer_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: consumer_text.to_string(),
            },
        })
        .await;

    // Cursor on `request` in `$c->request();` (line 4)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: consumer_uri.clone(),
            },
            position: Position {
                line: 4,
                character: 5,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $c->request() when $c is VL\\Client via use-as alias"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 4,
                "request() is declared on line 4 in Client"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Verify that completion in a multi-namespace file resolves the correct
/// namespace for each block.  In `namespace B { }`, a `new` expression
/// for class `Bar` (declared in the same namespace block) should resolve
/// to `B\Bar`, not `A\Bar`.
#[tokio::test]
async fn test_completion_multi_namespace_blocks() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///multi_ns.php").unwrap();
    let text = r#"<?php
namespace A {
    class Foo {
        public function fromA(): string { return 'a'; }
    }
}
namespace B {
    class Bar {
        public function fromB(): int { return 1; }
    }
    class Consumer {
        public function test() {
            $b = new Bar();
            $b->
        }
    }
}
"#
    .to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.clone(),
        },
    };
    backend.did_open(open_params).await;

    // Find the line containing `$b->`
    let trigger_line = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("$b->"))
        .map(|(i, _)| i as u32)
        .expect("trigger line not found");

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: trigger_line,
                character: 17, // after `$b->`
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    let items = match result {
        Some(CompletionResponse::List(list)) => list.items,
        Some(CompletionResponse::Array(items)) => items,
        None => panic!("Expected completions, got None"),
    };

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // $b is `Bar` from namespace B, so `fromB` must be present.
    assert!(
        labels.iter().any(|l| l.starts_with("fromB")),
        "Expected 'fromB' from B\\Bar, got: {:?}",
        labels
    );
    // `fromA` belongs to A\Foo and must NOT appear.
    assert!(
        !labels.iter().any(|l| l.starts_with("fromA")),
        "'fromA' from A\\Foo should not appear in B\\Bar completions, got: {:?}",
        labels
    );
}

/// A union whose members share a short name across namespaces
/// (`NsA\Thing|NsB\Thing`) must keep both receivers.  Candidate classes
/// are deduplicated so a fluent chain through a union does not double the
/// receiver set at every link; keying that dedup on the short name instead
/// of the FQN collapsed the two `Thing` classes into one and lost every
/// member of the second.
#[tokio::test]
async fn test_completion_union_of_same_short_name_classes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///short_name_union.php").unwrap();
    let text = r#"<?php
namespace NsA {
    class Thing {
        public function onlyNsA(): string { return ''; }
    }
}
namespace NsB {
    class Thing {
        public function onlyNsB(): string { return ''; }
    }
}
namespace App {
    class Maker {
        /** @return \NsA\Thing|\NsB\Thing */
        public function make() { return new \NsA\Thing(); }
    }
    class Consumer {
        public function test(Maker $maker) {
            $maker->make()->
        }
    }
}
"#
    .to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.clone(),
        },
    };
    backend.did_open(open_params).await;

    let trigger_line = text
        .lines()
        .position(|l| l.trim() == "$maker->make()->")
        .expect("trigger line not found") as u32;
    let character = text.lines().nth(trigger_line as usize).unwrap().len() as u32;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: trigger_line,
                    character,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    let items = match result {
        Some(CompletionResponse::List(list)) => list.items,
        Some(CompletionResponse::Array(items)) => items,
        None => panic!("Expected completions, got None"),
    };

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    for expected in ["onlyNsA", "onlyNsB"] {
        assert!(
            labels.iter().any(|l| l.starts_with(expected)),
            "Expected '{}' from the NsA\\Thing|NsB\\Thing union, got: {:?}",
            expected,
            labels
        );
    }
}

/// Two `namespace` blocks importing the same short name from different
/// namespaces each complete the members of their own class.
#[tokio::test]
async fn test_completion_resolves_through_the_blocks_own_import() {
    let text = r#"<?php
namespace X {
    class Foo { public function fromX(): void {} }
}
namespace Y {
    class Foo { public function fromY(): void {} }
}
namespace A {
    use X\Foo;
    function a(Foo $f): void {
        $f->
    }
}
namespace B {
    use Y\Foo;
    function b(Foo $f): void {
        $f->
    }
}
"#;
    for (line, own, other) in [(10, "fromX", "fromY"), (16, "fromY", "fromX")] {
        let backend = create_test_backend();
        let uri = Url::parse("file:///blocks.php").unwrap();
        let labels = crate::common::complete_labels_at(&backend, &uri, text, line, 12).await;
        assert!(
            labels.iter().any(|l| l.starts_with(own))
                && !labels.iter().any(|l| l.starts_with(other)),
            "line {line} should offer `{own}` only, got: {labels:?}"
        );
    }
}
