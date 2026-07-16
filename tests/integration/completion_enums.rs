use crate::common::{
    create_psr4_workspace, create_psr4_workspace_with_enum_stubs, create_test_backend,
    create_test_backend_with_stubs,
};

use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Nullsafe operator ?-> completion ───────────────────────────────────────

/// Test: `Priority::tryFrom($int)?->` should suggest `name` and `value`
/// just like `Priority::tryFrom($int)->` does.
#[tokio::test]
async fn test_completion_nullsafe_arrow_on_tryfrom() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///nullsafe_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Priority: int\n",
        "{\n",
        "    case Low = 1;\n",
        "    case Medium = 2;\n",
        "    case High = 3;\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function test(int $val): void {\n",
        "        Priority::tryFrom($val)?->\n",
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
                    character: 38,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Completion should return results for ?->");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // BackedEnum instances have `name` and `value` properties.
            assert!(
                labels.iter().any(|l| l.contains("name")),
                "?-> after tryFrom() should include 'name', got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.contains("value")),
                "?-> after tryFrom() should include 'value', got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: `Priority::cases()[0]->` should suggest `name` and `value`.
///
/// The `UnitEnum::cases()` stub is declared `: array`, so the element
/// type of an inline `cases()[0]` index is only known once `cases()` is
/// refined to `list<Priority>` for the concrete enum.
#[tokio::test]
async fn test_completion_arrow_on_cases_index() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///cases_index_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Priority: int\n",
        "{\n",
        "    case Low = 1;\n",
        "    case Medium = 2;\n",
        "    case High = 3;\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function test(): void {\n",
        "        Priority::cases()[0]->\n",
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

    assert!(
        result.is_some(),
        "Completion should return results for cases()[0]->"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.contains("name")),
                "cases()[0]-> should include 'name', got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.contains("value")),
                "cases()[0]-> should include 'value', got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: Verify that `->` (without `?`) on tryFrom still works (regression guard).
#[tokio::test]
async fn test_completion_regular_arrow_on_tryfrom() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///regular_arrow_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Priority: int\n",
        "{\n",
        "    case Low = 1;\n",
        "    case Medium = 2;\n",
        "    case High = 3;\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function test(int $val): void {\n",
        "        Priority::tryFrom($val)->\n",
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
                    character: 37,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Completion should return results for ->");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.contains("name")),
                "-> after tryFrom() should include 'name', got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.contains("value")),
                "-> after tryFrom() should include 'value', got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: `$var?->` on a regular class should also work with the nullsafe operator.
#[tokio::test]
async fn test_completion_nullsafe_arrow_on_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nullsafe_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public string $label;\n",
        "    public function render(): void {}\n",
        "}\n",
        "\n",
        "class Page {\n",
        "    public function test(?Widget $w): void {\n",
        "        $w?->\n",
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
                    character: 13,
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
        "Completion should return results for $w?->"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.contains("render")),
                "$w?-> should include 'render', got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.contains("label")),
                "$w?-> should include 'label', got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: `$this->getWidget()?->` should complete on the return type of getWidget().
#[tokio::test]
async fn test_completion_nullsafe_arrow_on_method_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nullsafe_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public string $label;\n",
        "    public function render(): void {}\n",
        "}\n",
        "\n",
        "class Page {\n",
        "    public function getWidget(): ?Widget { return null; }\n",
        "    public function test(): void {\n",
        "        $this->getWidget()?->\n",
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
                    character: 33,
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
        "Completion should return results for getWidget()?->"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.contains("render")),
                "getWidget()?-> should include 'render', got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.contains("label")),
                "getWidget()?-> should include 'label', got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Basic enum case completion via :: ──────────────────────────────────────

/// Test: Completing on `EnumName::` should show enum cases as constants.
#[tokio::test]
async fn test_completion_enum_cases_via_double_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum CustomerAvailabilityStatus: int\n",
        "{\n",
        "    case CUSTOMER_NOT_IN_AUDIENCE = -1;\n",
        "    case AVAILABLE_TO_CUSTOMER = 0;\n",
        "}\n",
        "\n",
        "class Service {\n",
        "    public function test(): void {\n",
        "        CustomerAvailabilityStatus::\n",
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
                    character: 36,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                constant_names.contains(&"CUSTOMER_NOT_IN_AUDIENCE"),
                "Should include enum case 'CUSTOMER_NOT_IN_AUDIENCE', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"AVAILABLE_TO_CUSTOMER"),
                "Should include enum case 'AVAILABLE_TO_CUSTOMER', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Unit enum completion ───────────────────────────────────────────────────

/// Test: Completing on a unit enum (no backing type) should show cases.
#[tokio::test]
async fn test_completion_unit_enum_cases() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unit_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Color\n",
        "{\n",
        "    case Red;\n",
        "    case Green;\n",
        "    case Blue;\n",
        "}\n",
        "\n",
        "class Painter {\n",
        "    public function test(): void {\n",
        "        Color::\n",
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

    assert!(result.is_some(), "Completion should return results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                constant_names.contains(&"Red"),
                "Should include enum case 'Red', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Green"),
                "Should include enum case 'Green', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Blue"),
                "Should include enum case 'Blue', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Enum with methods ──────────────────────────────────────────────────────

/// Test: Completing on an enum via `::` should show cases (as constants) and
/// static methods.  Instance methods are only shown via `->` access.
#[tokio::test]
async fn test_completion_enum_cases_and_static_methods_via_double_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_methods.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Suit: string\n",
        "{\n",
        "    case Hearts = 'H';\n",
        "    case Diamonds = 'D';\n",
        "    case Clubs = 'C';\n",
        "    case Spades = 'S';\n",
        "\n",
        "    public function color(): string\n",
        "    {\n",
        "        return 'red';\n",
        "    }\n",
        "\n",
        "    public static function fromSymbol(string $s): self\n",
        "    {\n",
        "        return self::Hearts;\n",
        "    }\n",
        "}\n",
        "\n",
        "class Game {\n",
        "    public function test(): void {\n",
        "        Suit::\n",
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
                    line: 21,
                    character: 14,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                constant_names.contains(&"Hearts"),
                "Should include enum case 'Hearts', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Spades"),
                "Should include enum case 'Spades', got: {:?}",
                constant_names
            );
            assert!(
                method_names.contains(&"fromSymbol"),
                "Should include static method 'fromSymbol', got: {:?}",
                method_names
            );
            // Instance methods should NOT appear via `::` access
            assert!(
                !method_names.contains(&"color"),
                "Should NOT include instance method 'color' via '::', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: Completing on `$this->` inside an enum method should show the
/// enum's own instance methods.
#[tokio::test]
async fn test_completion_enum_instance_methods_via_arrow() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_arrow.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "enum Suit: string\n",                   // 1
        "{\n",                                   // 2
        "    case Hearts = 'H';\n",              // 3
        "    case Spades = 'S';\n",              // 4
        "\n",                                    // 5
        "    public function color(): string\n", // 6
        "    {\n",                               // 7
        "        return 'red';\n",               // 8
        "    }\n",                               // 9
        "\n",                                    // 10
        "    public function isRed(): bool\n",   // 11
        "    {\n",                               // 12
        "        return true;\n",                // 13
        "    }\n",                               // 14
        "\n",                                    // 15
        "    public function test(): void\n",    // 16
        "    {\n",                               // 17
        "        $this->\n",                     // 18
        "    }\n",                               // 19
        "}\n",                                   // 20
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
                    line: 18,
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
                method_names.contains(&"color"),
                "Should include instance method 'color' via '->', got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"isRed"),
                "Should include instance method 'isRed' via '->', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Enum with real constants and cases ─────────────────────────────────────

/// Test: Enum with both `const` declarations and `case` declarations should
/// show all of them as constants in completion.
#[tokio::test]
async fn test_completion_enum_mixed_constants_and_cases() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_mixed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Status: int\n",
        "{\n",
        "    const DEFAULT_STATUS = 0;\n",
        "    case Active = 1;\n",
        "    case Inactive = 2;\n",
        "}\n",
        "\n",
        "class Handler {\n",
        "    public function test(): void {\n",
        "        Status::\n",
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
                    character: 16,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                constant_names.contains(&"DEFAULT_STATUS"),
                "Should include real constant 'DEFAULT_STATUS', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Active"),
                "Should include enum case 'Active', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Inactive"),
                "Should include enum case 'Inactive', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Goto definition: enum case ─────────────────────────────────────────────

/// Test: Clicking on `Status::Active` should jump to the `case Active` line.
#[tokio::test]
async fn test_goto_definition_enum_case_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_goto.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "enum Status: int\n",                   // 1
        "{\n",                                  // 2
        "    case Active = 1;\n",               // 3
        "    case Inactive = 2;\n",             // 4
        "}\n",                                  // 5
        "\n",                                   // 6
        "class Service {\n",                    // 7
        "    public function test(): void {\n", // 8
        "        $s = Status::Active;\n",       // 9
        "    }\n",                              // 10
        "}\n",                                  // 11
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

    // Click on "Active" in `Status::Active` on line 9
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 23,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve goto-definition for enum case"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "case Active is declared on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Test: Goto-definition on a real `const` inside an enum still works.
#[tokio::test]
async fn test_goto_definition_enum_const_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_const_goto.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "enum Status: int\n",                     // 1
        "{\n",                                    // 2
        "    const DEFAULT_STATUS = 0;\n",        // 3
        "    case Active = 1;\n",                 // 4
        "    case Inactive = 2;\n",               // 5
        "}\n",                                    // 6
        "\n",                                     // 7
        "class Service {\n",                      // 8
        "    public function test(): void {\n",   // 9
        "        $d = Status::DEFAULT_STATUS;\n", // 10
        "    }\n",                                // 11
        "}\n",                                    // 12
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

    // Click on "DEFAULT_STATUS" in `Status::DEFAULT_STATUS` on line 10
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve goto-definition for enum const"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "const DEFAULT_STATUS is declared on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Test: Goto-definition on an enum method via `$this->` inside the enum
/// should jump to the method declaration.
#[tokio::test]
async fn test_goto_definition_enum_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_method_goto.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "enum Suit: string\n",                   // 1
        "{\n",                                   // 2
        "    case Hearts = 'H';\n",              // 3
        "    case Spades = 'S';\n",              // 4
        "\n",                                    // 5
        "    public function color(): string\n", // 6
        "    {\n",                               // 7
        "        return 'red';\n",               // 8
        "    }\n",                               // 9
        "\n",                                    // 10
        "    public function test(): void\n",    // 11
        "    {\n",                               // 12
        "        $this->color();\n",             // 13
        "    }\n",                               // 14
        "}\n",                                   // 15
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

    // Click on "color" in `$this->color()` on line 13
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve goto-definition for enum method"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 6,
                "method color() is declared on line 6"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Cross-file enum resolution (PSR-4) ─────────────────────────────────────

/// Test: Completing on an enum from another file via PSR-4 autoloading.
#[tokio::test]
async fn test_completion_enum_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\Enums\\": "src/Enums/"
            }
        }
    }"#;

    let enum_content = concat!(
        "<?php\n",
        "namespace App\\Enums;\n",
        "\n",
        "enum Priority: int\n",
        "{\n",
        "    case Low = 0;\n",
        "    case Medium = 1;\n",
        "    case High = 2;\n",
        "    case Critical = 3;\n",
        "\n",
        "    public static function fromValue(int $v): self\n",
        "    {\n",
        "        return self::Low;\n",
        "    }\n",
        "}\n",
    );

    let (backend, dir) =
        create_psr4_workspace(composer_json, &[("src/Enums/Priority.php", enum_content)]);

    let main_uri = Url::from_file_path(dir.path().join("main.php")).unwrap();
    let main_text = concat!(
        "<?php\n",
        "use App\\Enums\\Priority;\n",
        "\n",
        "class TaskService {\n",
        "    public function test(): void {\n",
        "        Priority::\n",
        "    }\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: main_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: main_text.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: main_uri },
                position: Position {
                    line: 5,
                    character: 18,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                constant_names.contains(&"Low"),
                "Should include enum case 'Low', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"High"),
                "Should include enum case 'High', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Critical"),
                "Should include enum case 'Critical', got: {:?}",
                constant_names
            );
            assert!(
                method_names.contains(&"fromValue"),
                "Should include static method 'fromValue', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: Goto-definition on an enum case from another file via PSR-4.
#[tokio::test]
async fn test_goto_definition_enum_case_cross_file_psr4() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\Enums\\": "src/Enums/"
            }
        }
    }"#;

    let enum_content = concat!(
        "<?php\n",                 // 0
        "namespace App\\Enums;\n", // 1
        "\n",                      // 2
        "enum Direction\n",        // 3
        "{\n",                     // 4
        "    case North;\n",       // 5
        "    case South;\n",       // 6
        "    case East;\n",        // 7
        "    case West;\n",        // 8
        "}\n",                     // 9
    );

    let (backend, dir) =
        create_psr4_workspace(composer_json, &[("src/Enums/Direction.php", enum_content)]);

    let main_uri = Url::from_file_path(dir.path().join("main.php")).unwrap();
    let main_text = concat!(
        "<?php\n",
        "use App\\Enums\\Direction;\n",
        "\n",
        "class Navigator {\n",
        "    public function test(): void {\n",
        "        $d = Direction::North;\n",
        "    }\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: main_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: main_text.to_string(),
            },
        })
        .await;

    let enum_uri = Url::from_file_path(dir.path().join("src/Enums/Direction.php")).unwrap();

    // Click on "North" in `Direction::North` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: main_uri.clone(),
            },
            position: Position {
                line: 5,
                character: 26,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve goto-definition for cross-file enum case"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, enum_uri, "Should jump to the enum file");
            assert_eq!(
                location.range.start.line, 5,
                "case North is declared on line 5"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Enum inside namespace (same file) ──────────────────────────────────────

/// Test: Completing on an enum defined inside a namespace in the same file.
#[tokio::test]
async fn test_completion_enum_in_namespace_same_file() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_ns.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Enums;\n",
        "\n",
        "enum Visibility\n",
        "{\n",
        "    case Published;\n",
        "    case Draft;\n",
        "    case Archived;\n",
        "}\n",
        "\n",
        "class ContentService {\n",
        "    public function test(): void {\n",
        "        Visibility::\n",
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                constant_names.contains(&"Published"),
                "Should include 'Published', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Draft"),
                "Should include 'Draft', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Archived"),
                "Should include 'Archived', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Enum with trait use ────────────────────────────────────────────────────

/// Test: An enum that uses a trait should expose the trait's cases via `::`
/// and the trait's instance methods via `->`.
#[tokio::test]
async fn test_completion_enum_with_trait_cases_via_double_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_trait.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait HasDescription {\n",
        "    public function describe(): string { return ''; }\n",
        "}\n",
        "\n",
        "enum Size\n",
        "{\n",
        "    use HasDescription;\n",
        "\n",
        "    case Small;\n",
        "    case Medium;\n",
        "    case Large;\n",
        "}\n",
        "\n",
        "class Shop {\n",
        "    public function test(): void {\n",
        "        Size::\n",
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
                    character: 14,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                constant_names.contains(&"Small"),
                "Should include enum case 'Small', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Medium"),
                "Should include enum case 'Medium', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Large"),
                "Should include enum case 'Large', got: {:?}",
                constant_names
            );
            // Instance method `describe` from the trait should NOT appear
            // via `::` access — it's only accessible on instances via `->`.
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(
                !method_names.contains(&"describe"),
                "Instance method 'describe' should NOT appear via '::', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Enum implements interface ──────────────────────────────────────────────

/// Test: Enums implementing an interface should still parse correctly and
/// show their cases via `::`.
#[tokio::test]
async fn test_completion_enum_implements_interface() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_iface.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface HasLabel {\n",
        "    public function label(): string;\n",
        "}\n",
        "\n",
        "enum Fruit: string implements HasLabel\n",
        "{\n",
        "    case Apple = 'apple';\n",
        "    case Banana = 'banana';\n",
        "    case Cherry = 'cherry';\n",
        "\n",
        "    public function label(): string\n",
        "    {\n",
        "        return ucfirst($this->value);\n",
        "    }\n",
        "}\n",
        "\n",
        "class FruitStand {\n",
        "    public function test(): void {\n",
        "        Fruit::\n",
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
                    line: 19,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                constant_names.contains(&"Apple"),
                "Should include enum case 'Apple', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Cherry"),
                "Should include enum case 'Cherry', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Goto-definition for the enum name itself ───────────────────────────────

/// Test: Clicking on an enum name (e.g., `Status` in `Status::Active`)
/// should jump to the enum definition.
#[tokio::test]
async fn test_goto_definition_enum_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_name_goto.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "enum Status: int\n",                   // 1
        "{\n",                                  // 2
        "    case Active = 1;\n",               // 3
        "    case Inactive = 2;\n",             // 4
        "}\n",                                  // 5
        "\n",                                   // 6
        "class Service {\n",                    // 7
        "    public function test(): void {\n", // 8
        "        $s = Status::Active;\n",       // 9
        "    }\n",                              // 10
        "}\n",                                  // 11
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

    // Click on "Status" in `Status::Active` on line 9
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve goto-definition for enum name"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "enum Status is declared on line 1"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── self:: inside enum ─────────────────────────────────────────────────────

/// Test: Completing on `self::` inside an enum method should show the
/// enum's own cases and static methods.
#[tokio::test]
async fn test_completion_self_inside_enum() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_self.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Priority: int\n",
        "{\n",
        "    case Low = 0;\n",
        "    case Medium = 1;\n",
        "    case High = 2;\n",
        "\n",
        "    public function isUrgent(): bool\n",
        "    {\n",
        "        return $this === self::\n",
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
                    character: 31,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                constant_names.contains(&"Low"),
                "Should include enum case 'Low', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Medium"),
                "Should include enum case 'Medium', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"High"),
                "Should include enum case 'High', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Implicit UnitEnum / BackedEnum interface inheritance ───────────────────

/// When a UnitEnum stub is available, a unit enum should inherit its methods
/// (e.g. `cases()`) via the implicit interface added to `used_traits`.
#[tokio::test]
async fn test_completion_unit_enum_inherits_cases_from_stub() {
    let backend = create_test_backend();

    // Open the UnitEnum stub so it lands in the ast_map.
    let stub_uri = Url::parse("file:///stubs/UnitEnum.php").unwrap();
    let unit_enum_stub = concat!(
        "<?php\n",
        "interface UnitEnum\n",
        "{\n",
        "    public static function cases(): array;\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: stub_uri,
                language_id: "php".to_string(),
                version: 1,
                text: unit_enum_stub.to_string(),
            },
        })
        .await;

    // Open a file containing a unit enum and a class that uses it.
    let uri = Url::parse("file:///enum_stub_unit.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Color\n",
        "{\n",
        "    case Red;\n",
        "    case Green;\n",
        "    case Blue;\n",
        "}\n",
        "\n",
        "class Palette {\n",
        "    public function test(): void {\n",
        "        Color::\n",
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

    assert!(result.is_some(), "Completion should return results");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"cases"),
                "Unit enum should inherit 'cases()' from UnitEnum stub, got methods: {:?}",
                method_names
            );
            assert!(
                constant_names.contains(&"Red"),
                "Should still include enum case 'Red', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a BackedEnum stub is available, a backed enum should inherit its
/// methods (e.g. `from()`, `tryFrom()`, `cases()`) via the implicit
/// interface added to `used_traits`.
#[tokio::test]
async fn test_completion_backed_enum_inherits_from_and_tryfrom_from_stub() {
    let backend = create_test_backend();

    // Open the BackedEnum stub so it lands in the ast_map.
    let stub_uri = Url::parse("file:///stubs/BackedEnum.php").unwrap();
    let backed_enum_stub = concat!(
        "<?php\n",
        "interface BackedEnum\n",
        "{\n",
        "    public static function from(int|string $value): static;\n",
        "    public static function tryFrom(int|string $value): ?static;\n",
        "    public static function cases(): array;\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: stub_uri,
                language_id: "php".to_string(),
                version: 1,
                text: backed_enum_stub.to_string(),
            },
        })
        .await;

    // Open a file containing a backed enum and a class that uses it.
    let uri = Url::parse("file:///enum_stub_backed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Priority: int\n",
        "{\n",
        "    case Low = 0;\n",
        "    case Medium = 1;\n",
        "    case High = 2;\n",
        "}\n",
        "\n",
        "class TaskService {\n",
        "    public function test(): void {\n",
        "        Priority::\n",
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
                    character: 18,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"from"),
                "Backed enum should inherit 'from()' from BackedEnum stub, got methods: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"tryFrom"),
                "Backed enum should inherit 'tryFrom()' from BackedEnum stub, got methods: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"cases"),
                "Backed enum should inherit 'cases()' from BackedEnum stub, got methods: {:?}",
                method_names
            );
            assert!(
                constant_names.contains(&"Low"),
                "Should still include enum case 'Low', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"High"),
                "Should still include enum case 'High', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Parser-level check: a unit enum should have `\UnitEnum` in used_traits,
/// and a backed enum should have `\BackedEnum`.
#[tokio::test]
async fn test_parser_enum_implicit_interface_in_used_traits() {
    let backend = create_test_backend();

    // Unit enum
    let unit_php = concat!(
        "<?php\n",
        "enum Direction\n",
        "{\n",
        "    case Up;\n",
        "    case Down;\n",
        "}\n",
    );
    let unit_classes = backend.parse_php(unit_php);
    assert_eq!(unit_classes.len(), 1);
    assert!(
        unit_classes[0]
            .used_traits
            .iter()
            .any(|t| t == "\\UnitEnum"),
        "Unit enum should have \\UnitEnum in used_traits, got: {:?}",
        unit_classes[0].used_traits
    );
    assert!(
        !unit_classes[0]
            .used_traits
            .iter()
            .any(|t| t == "\\BackedEnum"),
        "Unit enum should NOT have \\BackedEnum, got: {:?}",
        unit_classes[0].used_traits
    );

    // Backed enum (int)
    let backed_php = concat!(
        "<?php\n",
        "enum Status: int\n",
        "{\n",
        "    case Active = 1;\n",
        "    case Inactive = 0;\n",
        "}\n",
    );
    let backed_classes = backend.parse_php(backed_php);
    assert_eq!(backed_classes.len(), 1);
    assert!(
        backed_classes[0]
            .used_traits
            .iter()
            .any(|t| t == "\\BackedEnum"),
        "Backed enum should have \\BackedEnum in used_traits, got: {:?}",
        backed_classes[0].used_traits
    );
    assert!(
        !backed_classes[0]
            .used_traits
            .iter()
            .any(|t| t == "\\UnitEnum"),
        "Backed enum should NOT have \\UnitEnum, got: {:?}",
        backed_classes[0].used_traits
    );

    // Backed enum (string)
    let string_php = concat!(
        "<?php\n",
        "enum Suit: string\n",
        "{\n",
        "    case Hearts = 'H';\n",
        "}\n",
    );
    let string_classes = backend.parse_php(string_php);
    assert_eq!(string_classes.len(), 1);
    assert!(
        string_classes[0]
            .used_traits
            .iter()
            .any(|t| t == "\\BackedEnum"),
        "String-backed enum should have \\BackedEnum, got: {:?}",
        string_classes[0].used_traits
    );
}

/// An enum that also uses an explicit trait should have both the trait
/// and the implicit interface in `used_traits`.
#[tokio::test]
async fn test_parser_enum_with_trait_also_has_implicit_interface() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "trait HasLabel {\n",
        "    public function label(): string { return 'label'; }\n",
        "}\n",
        "\n",
        "enum Status: int\n",
        "{\n",
        "    use HasLabel;\n",
        "\n",
        "    case Active = 1;\n",
        "    case Inactive = 0;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    let enum_info = classes.iter().find(|c| c.name == "Status").unwrap();

    assert!(
        enum_info.used_traits.iter().any(|t| t == "HasLabel"),
        "Should include the explicit trait, got: {:?}",
        enum_info.used_traits
    );
    assert!(
        enum_info.used_traits.iter().any(|t| t == "\\BackedEnum"),
        "Should include implicit \\BackedEnum, got: {:?}",
        enum_info.used_traits
    );
}

// ─── Embedded stub tests (no manual stub loading) ───────────────────────────

/// With embedded stubs, a unit enum should automatically get `cases()` and
/// `$name` without any manually opened stub files.
#[tokio::test]
async fn test_completion_unit_enum_gets_cases_from_embedded_stub() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///embedded_unit.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Direction\n",
        "{\n",
        "    case Up;\n",
        "    case Down;\n",
        "    case Left;\n",
        "    case Right;\n",
        "}\n",
        "\n",
        "class Nav {\n",
        "    public function test(): void {\n",
        "        Direction::\n",
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
                    character: 20,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"cases"),
                "Unit enum should auto-inherit 'cases()' from embedded UnitEnum stub, got: {:?}",
                method_names
            );
            assert!(
                constant_names.contains(&"Up"),
                "Should include enum case 'Up', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"Right"),
                "Should include enum case 'Right', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// With embedded stubs, a backed enum should automatically get `from()`,
/// `tryFrom()`, `cases()`, `$name`, and `$value` without any manually
/// opened stub files — including members inherited through
/// BackedEnum extends UnitEnum.
#[tokio::test]
async fn test_completion_backed_enum_gets_all_spl_members_from_embedded_stubs() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///embedded_backed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum HttpStatus: int\n",
        "{\n",
        "    case Ok = 200;\n",
        "    case NotFound = 404;\n",
        "    case ServerError = 500;\n",
        "}\n",
        "\n",
        "class Router {\n",
        "    public function test(): void {\n",
        "        HttpStatus::\n",
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
                    character: 20,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // BackedEnum's own methods
            assert!(
                method_names.contains(&"from"),
                "Backed enum should auto-inherit 'from()' from embedded BackedEnum stub, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"tryFrom"),
                "Backed enum should auto-inherit 'tryFrom()' from embedded BackedEnum stub, got: {:?}",
                method_names
            );

            // UnitEnum's method inherited through BackedEnum extends UnitEnum
            assert!(
                method_names.contains(&"cases"),
                "Backed enum should auto-inherit 'cases()' from UnitEnum via extends chain, got: {:?}",
                method_names
            );

            // Enum cases
            assert!(
                constant_names.contains(&"Ok"),
                "Should include enum case 'Ok', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"NotFound"),
                "Should include enum case 'NotFound', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// With embedded stubs, $this-> inside a backed enum should show `$name`
/// (from UnitEnum) and `$value` (from BackedEnum) plus instance methods.
#[tokio::test]
async fn test_completion_backed_enum_arrow_gets_properties_from_embedded_stubs() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///embedded_arrow.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Color: string\n",
        "{\n",
        "    case Red = 'red';\n",
        "    case Blue = 'blue';\n",
        "\n",
        "    public function label(): string\n",
        "    {\n",
        "        return $this->\n",
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
                    character: 22,
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
            let property_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // $name from UnitEnum (via BackedEnum extends UnitEnum)
            assert!(
                property_names.contains(&"name"),
                "Should auto-inherit 'name' property from UnitEnum stub, got: {:?}",
                property_names
            );

            // $value from BackedEnum
            assert!(
                property_names.contains(&"value"),
                "Should auto-inherit 'value' property from BackedEnum stub, got: {:?}",
                property_names
            );

            // Own instance method
            assert!(
                method_names.contains(&"label"),
                "Should include own instance method 'label', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Embedded stubs are cached after first access — verify that a second
/// enum in the same session also gets the methods without re-parsing.
#[tokio::test]
async fn test_completion_embedded_stub_caching_across_files() {
    let backend = create_test_backend_with_stubs();

    // First file: a unit enum
    let uri1 = Url::parse("file:///cache_test_1.php").unwrap();
    let text1 = concat!(
        "<?php\n",
        "enum Suit\n",
        "{\n",
        "    case Hearts;\n",
        "    case Spades;\n",
        "}\n",
        "\n",
        "class Game1 {\n",
        "    public function test(): void {\n",
        "        Suit::\n",
        "    }\n",
        "}\n",
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

    // Trigger completion on the first file to cause stub parsing + caching
    let result1 = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri1 },
                position: Position {
                    line: 9,
                    character: 14,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    // Verify first file works
    if let Some(CompletionResponse::Array(items)) = &result1 {
        let method_names: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|i| i.filter_text.as_deref().unwrap())
            .collect();
        assert!(
            method_names.contains(&"cases"),
            "First enum should get 'cases()' from stub, got: {:?}",
            method_names
        );
    } else {
        panic!("Expected CompletionResponse::Array for first file");
    }

    // Second file: another unit enum — stubs should already be cached
    let uri2 = Url::parse("file:///cache_test_2.php").unwrap();
    let text2 = concat!(
        "<?php\n",
        "enum Season\n",
        "{\n",
        "    case Spring;\n",
        "    case Summer;\n",
        "}\n",
        "\n",
        "class Game2 {\n",
        "    public function test(): void {\n",
        "        Season::\n",
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

    // Verify second file also works (from cached stubs)
    if let Some(CompletionResponse::Array(items)) = &result2 {
        let method_names: Vec<&str> = items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
            .map(|i| i.filter_text.as_deref().unwrap())
            .collect();
        assert!(
            method_names.contains(&"cases"),
            "Second enum should also get 'cases()' from cached stub, got: {:?}",
            method_names
        );
    } else {
        panic!("Expected CompletionResponse::Array for second file");
    }
}

/// A backed enum should inherit members from both BackedEnum AND UnitEnum
/// (because BackedEnum extends UnitEnum).  This validates that the
/// `merge_traits_into` parent_class chain walk works for interface
/// inheritance.
#[tokio::test]
async fn test_completion_backed_enum_inherits_unit_enum_members_through_extends() {
    let backend = create_test_backend();

    // Open a UnitEnum stub with `cases()` and `$name`.
    let unit_stub_uri = Url::parse("file:///stubs/UnitEnum.php").unwrap();
    let unit_stub = concat!(
        "<?php\n",
        "interface UnitEnum\n",
        "{\n",
        "    public readonly string $name;\n",
        "    public static function cases(): array;\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: unit_stub_uri,
                language_id: "php".to_string(),
                version: 1,
                text: unit_stub.to_string(),
            },
        })
        .await;

    // Open a BackedEnum stub that extends UnitEnum.
    let backed_stub_uri = Url::parse("file:///stubs/BackedEnum.php").unwrap();
    let backed_stub = concat!(
        "<?php\n",
        "interface BackedEnum extends UnitEnum\n",
        "{\n",
        "    public readonly int|string $value;\n",
        "    public static function from(int|string $value): static;\n",
        "    public static function tryFrom(int|string $value): ?static;\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: backed_stub_uri,
                language_id: "php".to_string(),
                version: 1,
                text: backed_stub.to_string(),
            },
        })
        .await;

    // Open a file with a backed enum and a class that accesses it.
    let uri = Url::parse("file:///enum_backed_extends.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Priority: int\n",
        "{\n",
        "    case Low = 0;\n",
        "    case Medium = 1;\n",
        "    case High = 2;\n",
        "}\n",
        "\n",
        "class TaskService {\n",
        "    public function test(): void {\n",
        "        Priority::\n",
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
                    character: 18,
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
            let constant_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // BackedEnum's own methods
            assert!(
                method_names.contains(&"from"),
                "Should inherit 'from()' from BackedEnum, got methods: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"tryFrom"),
                "Should inherit 'tryFrom()' from BackedEnum, got methods: {:?}",
                method_names
            );

            // UnitEnum's methods inherited through BackedEnum extends UnitEnum
            assert!(
                method_names.contains(&"cases"),
                "Should inherit 'cases()' from UnitEnum via BackedEnum extends, got methods: {:?}",
                method_names
            );

            // Enum cases should still be present
            assert!(
                constant_names.contains(&"Low"),
                "Should include enum case 'Low', got: {:?}",
                constant_names
            );
            assert!(
                constant_names.contains(&"High"),
                "Should include enum case 'High', got: {:?}",
                constant_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same as above but tests arrow access ($instance->) to verify instance
/// members like `$name` and `$value` are inherited through the interface
/// extends chain.
#[tokio::test]
async fn test_completion_backed_enum_inherits_properties_through_extends() {
    let backend = create_test_backend();

    // Open stubs
    let unit_stub_uri = Url::parse("file:///stubs2/UnitEnum.php").unwrap();
    let unit_stub = concat!(
        "<?php\n",
        "interface UnitEnum\n",
        "{\n",
        "    public readonly string $name;\n",
        "    public static function cases(): array;\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: unit_stub_uri,
                language_id: "php".to_string(),
                version: 1,
                text: unit_stub.to_string(),
            },
        })
        .await;

    let backed_stub_uri = Url::parse("file:///stubs2/BackedEnum.php").unwrap();
    let backed_stub = concat!(
        "<?php\n",
        "interface BackedEnum extends UnitEnum\n",
        "{\n",
        "    public readonly int|string $value;\n",
        "    public static function from(int|string $value): static;\n",
        "    public static function tryFrom(int|string $value): ?static;\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: backed_stub_uri,
                language_id: "php".to_string(),
                version: 1,
                text: backed_stub.to_string(),
            },
        })
        .await;

    // A backed enum with a method that uses $this->
    let uri = Url::parse("file:///enum_arrow_extends.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Status: int\n",
        "{\n",
        "    case Active = 1;\n",
        "    case Inactive = 0;\n",
        "\n",
        "    public function describe(): string\n",
        "    {\n",
        "        return $this->\n",
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
                    character: 22,
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
            let property_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // BackedEnum's own property
            assert!(
                property_names.contains(&"value"),
                "Should inherit 'value' property from BackedEnum, got properties: {:?}",
                property_names
            );

            // UnitEnum's property inherited through BackedEnum extends UnitEnum
            assert!(
                property_names.contains(&"name"),
                "Should inherit 'name' property from UnitEnum via BackedEnum extends, got properties: {:?}",
                property_names
            );

            // The enum's own instance method should be present
            assert!(
                method_names.contains(&"describe"),
                "Should include own instance method 'describe', got methods: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Enum case arrow completion (e.g. Status::Active->label()) ──────────────

/// Test: `Status::Active->` in top-level code should suggest instance methods
/// defined on the enum.
#[tokio::test]
async fn test_completion_enum_case_arrow_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_case_arrow.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "enum Status: string\n",                  // 1
        "{\n",                                    // 2
        "    case Active = 'active';\n",          // 3
        "    case Inactive = 'inactive';\n",      // 4
        "\n",                                     // 5
        "    public function label(): string\n",  // 6
        "    {\n",                                // 7
        "        return 'Label';\n",              // 8
        "    }\n",                                // 9
        "\n",                                     // 10
        "    public function isActive(): bool\n", // 11
        "    {\n",                                // 12
        "        return true;\n",                 // 13
        "    }\n",                                // 14
        "}\n",                                    // 15
        "\n",                                     // 16
        "Status::Active->\n",                     // 17
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
                    character: 17,
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
        "Completion should return results for Status::Active->"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"label"),
                "Should include instance method 'label' via Status::Active->, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"isActive"),
                "Should include instance method 'isActive' via Status::Active->, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: `Status::Active->` inside a method body should also suggest
/// instance methods on the enum.
#[tokio::test]
async fn test_completion_enum_case_arrow_inside_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_case_arrow_method.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "enum Priority: int\n",                     // 1
        "{\n",                                      // 2
        "    case Low = 1;\n",                      // 3
        "    case High = 2;\n",                     // 4
        "\n",                                       // 5
        "    public function describe(): string\n", // 6
        "    {\n",                                  // 7
        "        return 'priority';\n",             // 8
        "    }\n",                                  // 9
        "}\n",                                      // 10
        "\n",                                       // 11
        "class Ticket {\n",                         // 12
        "    public function test(): void\n",       // 13
        "    {\n",                                  // 14
        "        Priority::High->\n",               // 15
        "    }\n",                                  // 16
        "}\n",                                      // 17
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
                    character: 24,
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
        "Completion should return results for Priority::High-> inside a method"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"describe"),
                "Should include instance method 'describe' via Priority::High->, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Test: `$var = Status::Active; $var->` in top-level code should suggest
/// instance methods on the enum (variable assigned from enum case).
#[tokio::test]
async fn test_completion_variable_assigned_enum_case_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///enum_var_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",                                            // 0
        "enum Status: string\n",                              // 1
        "{\n",                                                // 2
        "    case Active = 'active';\n",                      // 3
        "    case Inactive = 'inactive';\n",                  // 4
        "\n",                                                 // 5
        "    public function label(): string\n",              // 6
        "    {\n",                                            // 7
        "        return 'Label';\n",                          // 8
        "    }\n",                                            // 9
        "}\n",                                                // 10
        "\n",                                                 // 11
        "class Svc {\n",                                      // 12
        "    public function run(): string { return ''; }\n", // 13
        "}\n",                                                // 14
        "\n",                                                 // 15
        "$svc = new Svc();\n",                                // 16
        "$svc->\n",                                           // 17
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
                    character: 6,
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
        "Completion should return results for top-level $svc = new Svc()"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"run"),
                "Should include 'run' for top-level $svc-> , got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Regression test: `$value = $value->value` after `instanceof BackedEnum`
/// must not cause infinite recursion / stack overflow.
///
/// The variable `$value` is reassigned from its own property access.  Without
/// the cursor-offset limiting fix in `check_expression_for_assignment`, the
/// resolver would try to resolve `$value` on the RHS, re-discover the same
/// assignment, and recurse until the stack overflows — crashing the LSP
/// server into a zombie process with no error log.
#[tokio::test]
async fn test_completion_self_referential_assignment_backed_enum_value() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///self_ref_enum.php").unwrap();
    let text = concat!(
        "<?php\n",                                                   // 0
        "class ConvertHelper {\n",                                   // 1
        "    public static function toBool(mixed $value): bool {\n", // 2
        "        if ($value instanceof \\BackedEnum) {\n",           // 3
        "            $value = $value->value;\n",                     // 4
        "        }\n",                                               // 5
        "        $value->\n",                                        // 6
        "    }\n",                                                   // 7
        "}\n",                                                       // 8
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

    // The main assertion is that this does NOT hang / crash.
    // The completion request must return within a reasonable time.
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        backend.completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 6,
                    character: 17,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        }),
    )
    .await;

    // If we got here without timeout/crash, the fix works.
    assert!(
        result.is_ok(),
        "Completion request should not hang on self-referential assignment"
    );
}

/// Go-to-definition on `$value->value` after `instanceof BackedEnum` should
/// resolve to the `$value` property on the `BackedEnum` interface stub, NOT
/// to a standalone function named `value()`.
///
/// When member resolution fails (e.g. because the subject can't be resolved
/// or the property isn't found on the resolved class), the definition handler
/// falls through to global function lookup — which may find an unrelated
/// `value()` helper function.
#[tokio::test]
async fn test_goto_definition_backed_enum_value_property_not_function() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///goto_enum_prop.php").unwrap();
    let text = concat!(
        "<?php\n",                                                   // 0
        "function value($value, ...$args) { return $value; }\n",     // 1
        "\n",                                                        // 2
        "class ConvertHelper {\n",                                   // 3
        "    public static function toBool(mixed $value): bool {\n", // 4
        "        if ($value instanceof \\BackedEnum) {\n",           // 5
        "            $value = $value->value;\n",                     // 6
        "        }\n",                                               // 7
        "        return (bool) $value;\n",                           // 8
        "    }\n",                                                   // 9
        "}\n",                                                       // 10
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

    // Click on "value" in `$value->value` on line 6 (character ~33)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 33,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve goto-definition for BackedEnum->value property"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // Must point into the BackedEnum stub, not the current file.
            assert!(
                location.uri.as_str().contains("phpantom-stub://"),
                "Should resolve to the BackedEnum stub, not the current file. Got uri: {}",
                location.uri
            );
            // The `$value` property is on line 5 of the stub (after two
            // method declarations with `$value` parameters on lines 3–4).
            assert_eq!(
                location.range.start.line, 5,
                "Should point to the `$value` property declaration (line 5 of stub), \
                 not a method parameter. Got line: {}",
                location.range.start.line
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Iterating over `Enum::cases()` in a foreach should resolve the value
/// variable to the enum type, not to the enclosing class.
///
/// The `UnitEnum::cases()` stub has `@return static[]`, so the return
/// type string is `static[]`.  When the foreach expression is a static
/// call like `Country::cases()`, the `static` must be replaced with the
/// owner class name (`Country`) so that the element type resolves to
/// `Country` rather than the class containing the foreach.
#[tokio::test]
async fn test_completion_foreach_enum_cases_resolves_to_enum_type() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///foreach_enum_cases.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Country: string\n",
        "{\n",
        "    case BE = 'be';\n",
        "    case NL = 'nl';\n",
        "}\n",
        "\n",
        "class Handler {\n",
        "    public function run(): void {\n",
        "        foreach (Country::cases() as $country) {\n",
        "            $country->\n",
        "        }\n",
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
                    character: 23,
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
        "Completion should return results for $country-> inside foreach over Enum::cases()"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // BackedEnum instances have `name` and `value` properties.
            assert!(
                labels.iter().any(|l| l.contains("value")),
                "$country-> should include 'value' (from BackedEnum), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.contains("name")),
                "$country-> should include 'name' (from UnitEnum), got: {:?}",
                labels
            );
            // Must NOT resolve to the enclosing class Handler.
            assert!(
                !labels.iter().any(|l| l.contains("run")),
                "$country-> should NOT include 'run' from enclosing Handler class, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Same as above but for a unit enum (no backing type).
#[tokio::test]
async fn test_completion_foreach_unit_enum_cases_resolves_to_enum_type() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///foreach_unit_enum_cases.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Suit\n",
        "{\n",
        "    case Hearts;\n",
        "    case Diamonds;\n",
        "}\n",
        "\n",
        "class Dealer {\n",
        "    public function deal(): void {\n",
        "        foreach (Suit::cases() as $suit) {\n",
        "            $suit->\n",
        "        }\n",
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
                    character: 19,
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
        "Completion should return results for $suit-> inside foreach over UnitEnum::cases()"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // UnitEnum instances have a `name` property.
            assert!(
                labels.iter().any(|l| l.contains("name")),
                "$suit-> should include 'name' (from UnitEnum), got: {:?}",
                labels
            );
            // Must NOT resolve to the enclosing class Dealer.
            assert!(
                !labels.iter().any(|l| l.contains("deal")),
                "$suit-> should NOT include 'deal' from enclosing Dealer class, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When `Enum::cases()` is assigned to an intermediate variable and then
/// iterated, the foreach value variable should still resolve to the enum type.
///
/// The assignment `$countries = Country::cases()` resolves to `Country[]`
/// via the raw type inference path.  The foreach resolution must consult
/// that assignment-derived type (not just docblock annotations) to extract
/// the element type `Country`.
#[tokio::test]
async fn test_completion_foreach_variable_assigned_from_enum_cases() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///foreach_var_enum_cases.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Country: string\n",
        "{\n",
        "    case BE = 'be';\n",
        "    case NL = 'nl';\n",
        "}\n",
        "\n",
        "class Handler {\n",
        "    public function run(): void {\n",
        "        $countries = Country::cases();\n",
        "        foreach ($countries as $country) {\n",
        "            $country->\n",
        "        }\n",
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
                    character: 23,
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
        "Completion should return results for $country-> inside foreach over variable assigned from Enum::cases()"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.contains("value")),
                "$country-> should include 'value' (from BackedEnum), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.contains("name")),
                "$country-> should include 'name' (from UnitEnum), got: {:?}",
                labels
            );
            assert!(
                !labels.iter().any(|l| l.contains("run")),
                "$country-> should NOT include 'run' from enclosing Handler class, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file PSR-4 variant: the enum is defined in a separate file and
/// iterated via `cases()` in another class.
#[tokio::test]
async fn test_completion_foreach_enum_cases_cross_file() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": {
                "App\\Enums\\": "src/Enums/"
            }
        }
    }"#;

    let enum_content = concat!(
        "<?php\n",
        "namespace App\\Enums;\n",
        "\n",
        "enum Country: string\n",
        "{\n",
        "    case BE = 'be';\n",
        "    case NL = 'nl';\n",
        "}\n",
    );

    let (backend, dir) = create_psr4_workspace_with_enum_stubs(
        composer_json,
        &[("src/Enums/Country.php", enum_content)],
    );

    let handler_uri = Url::from_file_path(dir.path().join("handler.php")).unwrap();
    let handler_text = concat!(
        "<?php\n",
        "namespace App\\Handlers;\n",
        "\n",
        "use App\\Enums\\Country;\n",
        "\n",
        "class Handler {\n",
        "    public function handle(): void {\n",
        "        foreach (Country::cases() as $country) {\n",
        "            $country->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: handler_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: handler_text.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: handler_uri.clone(),
                },
                position: Position {
                    line: 8,
                    character: 23,
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
        "Completion should return results for $country-> in cross-file foreach over Enum::cases()"
    );
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.contains("value")),
                "$country-> should include 'value' (from BackedEnum) cross-file, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.contains("name")),
                "$country-> should include 'name' (from UnitEnum) cross-file, got: {:?}",
                labels
            );
            assert!(
                !labels.iter().any(|l| l.contains("handle")),
                "$country-> should NOT include 'handle' from enclosing Handler class, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_enum_implementing_generic_interface() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///test_enum_generic_interface.php").unwrap();
    let text = r#"<?php
/** @template T */
interface Labeled {
    /** @return T */
    public function label(): mixed;
}
/** @implements Labeled<string> */
enum Color: string implements Labeled {
    case Red = 'red';
    case Blue = 'blue';
    public function label(): string { return $this->value; }
}
$c = Color::Red;
$c->
"#;

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
                    line: 13,
                    character: 4,
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
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.contains("label")),
                "$c-> should include 'label' from Labeled interface, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.contains("name")),
                "$c-> should include 'name' from UnitEnum, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}
