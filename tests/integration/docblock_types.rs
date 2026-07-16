use crate::common::create_test_backend;
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helper ─────────────────────────────────────────────────────────────────

/// Extract completion filter_text names (falling back to label) from a
/// CompletionResponse.
fn completion_names(result: CompletionResponse) -> Vec<String> {
    match result {
        CompletionResponse::Array(items) => items
            .iter()
            .filter_map(|i| i.filter_text.clone().or_else(|| Some(i.label.clone())))
            .collect(),
        CompletionResponse::List(list) => list
            .items
            .iter()
            .filter_map(|i| i.filter_text.clone().or_else(|| Some(i.label.clone())))
            .collect(),
    }
}

// ─── Method @return Docblock Resolution ─────────────────────────────────────

/// Test: Method with no native return type but a `@return` docblock.
/// `$this->getSession()->regenerate()` should resolve because
/// `getSession()` has `@return Session` in its docblock.
#[tokio::test]
async fn test_completion_method_return_type_from_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class Session {\n",                                    // 1
        "    public function regenerate(): void {}\n",          // 2
        "    public function getId(): string { return ''; }\n", // 3
        "}\n",                                                  // 4
        "\n",                                                   // 5
        "class Controller {\n",                                 // 6
        "    /** @return Session */\n",                         // 7
        "    public function getSession()\n",                   // 8
        "    {\n",                                              // 9
        "        return new Session();\n",                      // 10
        "    }\n",                                              // 11
        "\n",                                                   // 12
        "    public function handle(): void {\n",               // 13
        "        $this->getSession()->\n",                      // 14
        "    }\n",                                              // 15
        "}\n",                                                  // 16
    );

    // line 14: "        $this->getSession()->"
    //           0       8              22   26
    // character 30 is right after the final ->

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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 14,
                character: 30,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "regenerate"),
        "Should offer 'regenerate' from Session class via @return docblock. Got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "getId"),
        "Should offer 'getId' from Session class via @return docblock. Got: {:?}",
        names
    );
}

/// Test: Goto definition on a member accessed through a method with only
/// a `@return` docblock (no native return type hint).
#[tokio::test]
async fn test_goto_definition_method_return_type_from_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "class Store {\n",                                                // 1
        "    public function get(string $key): mixed { return null; }\n", // 2
        "    public function put(string $key, mixed $value): void {}\n",  // 3
        "}\n",                                                            // 4
        "\n",                                                             // 5
        "class Manager {\n",                                              // 6
        "    /** @return Store */\n",                                     // 7
        "    public function driver()\n",                                 // 8
        "    {\n",                                                        // 9
        "        return new Store();\n",                                  // 10
        "    }\n",                                                        // 11
        "\n",                                                             // 12
        "    public function test(): void {\n",                           // 13
        "        $this->driver()->get('key');\n",                         // 14
        "    }\n",                                                        // 15
        "}\n",                                                            // 16
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

    // line 14: "        $this->driver()->get('key');"
    //           0       8             21  24
    // Click on "get" (character 25)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 14,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve driver()->get via @return docblock"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "get() is declared on line 2 of Store class"
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

/// Test: `@return` with fully-qualified class name (`\SessionStore`).
#[tokio::test]
async fn test_completion_method_return_type_fqn_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "class SessionStore {\n",                                         // 1
        "    public function get(string $key): mixed { return null; }\n", // 2
        "    public function put(string $key, mixed $val): void {}\n",    // 3
        "}\n",                                                            // 4
        "\n",                                                             // 5
        "class Manager {\n",                                              // 6
        "    /** @return \\SessionStore */\n",                            // 7
        "    public function driver()\n",                                 // 8
        "    {\n",                                                        // 9
        "        return new SessionStore();\n",                           // 10
        "    }\n",                                                        // 11
        "\n",                                                             // 12
        "    public function test(): void {\n",                           // 13
        "        $this->driver()->\n",                                    // 14
        "    }\n",                                                        // 15
        "}\n",                                                            // 16
    );

    // line 14: "        $this->driver()->"
    //           0       8            20 24
    // character 25 is right after the final ->

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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 14,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "get"),
        "Should offer 'get' from SessionStore via FQN @return docblock. Got: {:?}",
        names
    );
}

// ─── Property @var Docblock Resolution ──────────────────────────────────────

/// Test: Property with no native type hint but a `@var` docblock.
/// `$this->session->regenerate()` should resolve because the `$session`
/// property has `@var Session` in its docblock.
#[tokio::test]
async fn test_completion_property_type_from_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                              // 0
        "class Session {\n",                                    // 1
        "    public function regenerate(): void {}\n",          // 2
        "    public function getId(): string { return ''; }\n", // 3
        "}\n",                                                  // 4
        "\n",                                                   // 5
        "class Controller {\n",                                 // 6
        "    /** @var Session */\n",                            // 7
        "    protected $session;\n",                            // 8
        "\n",                                                   // 9
        "    public function handle(): void {\n",               // 10
        "        $this->session->\n",                           // 11
        "    }\n",                                              // 12
        "}\n",                                                  // 13
    );

    // line 11: "        $this->session->"
    //           0       8      14    2024
    // character 25 is right after the final ->

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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "regenerate"),
        "Should offer 'regenerate' from Session via @var docblock. Got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "getId"),
        "Should offer 'getId' from Session via @var docblock. Got: {:?}",
        names
    );
}

/// Test: Goto definition on a member accessed through a property with only
/// a `@var` docblock (no native type hint on the property).
#[tokio::test]
async fn test_goto_definition_property_type_from_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class Logger {\n",                                  // 1
        "    public function info(string $msg): void {}\n",  // 2
        "    public function error(string $msg): void {}\n", // 3
        "}\n",                                               // 4
        "\n",                                                // 5
        "class Service {\n",                                 // 6
        "    /** @var Logger */\n",                          // 7
        "    private $logger;\n",                            // 8
        "\n",                                                // 9
        "    public function run(): void {\n",               // 10
        "        $this->logger->info('hello');\n",           // 11
        "    }\n",                                           // 12
        "}\n",                                               // 13
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

    // line 11: "        $this->logger->info('hello');"
    //           0       8      14   19  23
    // Click on "info" (character 23)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 23,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->logger->info via @var docblock"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "info() is declared on line 2 of Logger class"
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

// ─── Docblock Overrides Native Type Hint ────────────────────────────────────

/// Test: `@var Session` overrides `object` type hint on a property.
/// The native `object` is broad enough to be refined by the docblock.
#[tokio::test]
async fn test_docblock_overrides_object_type_hint() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "class Session {\n",                           // 1
        "    public function regenerate(): void {}\n", // 2
        "}\n",                                         // 3
        "\n",                                          // 4
        "class Controller {\n",                        // 5
        "    /** @var Session */\n",                   // 6
        "    protected object $session;\n",            // 7
        "\n",                                          // 8
        "    public function handle(): void {\n",      // 9
        "        $this->session->\n",                  // 10
        "    }\n",                                     // 11
        "}\n",                                         // 12
    );

    // line 10: "        $this->session->"
    // character 25 is right after the final ->

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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: 25,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "regenerate"),
        "Should offer 'regenerate' from Session — @var Session overrides object. Got: {:?}",
        names
    );
}

/// Test: `@var Session` does NOT override `int` type hint on a property.
/// An `int` can never be an object, so the docblock is ignored.
#[tokio::test]
async fn test_docblock_does_not_override_scalar_type_hint() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "class Session {\n",                           // 1
        "    public function regenerate(): void {}\n", // 2
        "}\n",                                         // 3
        "\n",                                          // 4
        "class Controller {\n",                        // 5
        "    /** @var Session */\n",                   // 6
        "    protected int $id;\n",                    // 7
        "\n",                                          // 8
        "    public function handle(): void {\n",      // 9
        "        $this->id->\n",                       // 10
        "    }\n",                                     // 11
        "}\n",                                         // 12
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

    // line 10: "        $this->id->"
    // character 20 is right after the final ->
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    // Scalar type `int` cannot be overridden by @var Session — result
    // may be None (no completions) or an array without Session methods.
    if let Some(resp) = result {
        let names = completion_names(resp);
        assert!(
            !names.iter().any(|n| n == "regenerate"),
            "Should NOT offer 'regenerate' — @var Session can't override int. Got: {:?}",
            names
        );
    }
}

/// Test: `@var Session` does NOT override `string` type hint.
#[tokio::test]
async fn test_docblock_does_not_override_string_type_hint() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "class Session {\n",                           // 1
        "    public function regenerate(): void {}\n", // 2
        "}\n",                                         // 3
        "\n",                                          // 4
        "class Controller {\n",                        // 5
        "    /** @var Session */\n",                   // 6
        "    protected string $name;\n",               // 7
        "\n",                                          // 8
        "    public function handle(): void {\n",      // 9
        "        $this->name->\n",                     // 10
        "    }\n",                                     // 11
        "}\n",                                         // 12
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

    // line 10: "        $this->name->"
    // character 22 is right after the final ->
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    // Scalar type `string` cannot be overridden by @var Session — result
    // may be None (no completions) or an array without Session methods.
    if let Some(resp) = result {
        let names = completion_names(resp);
        assert!(
            !names.iter().any(|n| n == "regenerate"),
            "Should NOT offer 'regenerate' — @var Session can't override string. Got: {:?}",
            names
        );
    }
}

/// Test: `@return Session` overrides `mixed` return type on a method.
#[tokio::test]
async fn test_docblock_return_overrides_mixed() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "class Session {\n",                           // 1
        "    public function regenerate(): void {}\n", // 2
        "}\n",                                         // 3
        "\n",                                          // 4
        "class Factory {\n",                           // 5
        "    /** @return Session */\n",                // 6
        "    public function make(): mixed\n",         // 7
        "    {\n",                                     // 8
        "        return new Session();\n",             // 9
        "    }\n",                                     // 10
        "\n",                                          // 11
        "    public function test(): void {\n",        // 12
        "        $this->make()->\n",                   // 13
        "    }\n",                                     // 14
        "}\n",                                         // 15
    );

    // line 13: "        $this->make()->"
    //           0       8     13  17 21 23
    // character 23 is right after the final ->

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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 23,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "regenerate"),
        "Should offer 'regenerate' — @return Session overrides mixed. Got: {:?}",
        names
    );
}

/// Test: `@return Session` does NOT override `int` return type.
#[tokio::test]
async fn test_docblock_return_does_not_override_scalar() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                     // 0
        "class Session {\n",                           // 1
        "    public function regenerate(): void {}\n", // 2
        "}\n",                                         // 3
        "\n",                                          // 4
        "class Factory {\n",                           // 5
        "    /** @return Session */\n",                // 6
        "    public function getCount(): int\n",       // 7
        "    {\n",                                     // 8
        "        return 42;\n",                        // 9
        "    }\n",                                     // 10
        "\n",                                          // 11
        "    public function test(): void {\n",        // 12
        "        $this->getCount()->\n",               // 13
        "    }\n",                                     // 14
        "}\n",                                         // 15
    );

    // line 13: "        $this->getCount()->"
    // character 27 is right after the final ->

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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 27,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    // Scalar type `int` cannot be overridden by @return Session — result
    // may be None (no completions) or an array without Session methods.
    if let Some(resp) = result {
        let names = completion_names(resp);
        assert!(
            !names.iter().any(|n| n == "regenerate"),
            "Should NOT offer 'regenerate' — @return Session can't override int. Got: {:?}",
            names
        );
    }
}

// ─── Standalone Function @return Docblock ───────────────────────────────────

/// Test: Standalone function with `@return` docblock (no native type hint).
/// `session()->get()` should resolve because `session()` has
/// `@return SessionStore` in its docblock.
#[tokio::test]
async fn test_completion_function_return_type_from_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "class SessionStore {\n",                                         // 1
        "    public function get(string $key): mixed { return null; }\n", // 2
        "    public function put(string $key, mixed $val): void {}\n",    // 3
        "}\n",                                                            // 4
        "\n",                                                             // 5
        "/** @return SessionStore */\n",                                  // 6
        "function session()\n",                                           // 7
        "{\n",                                                            // 8
        "    return new SessionStore();\n",                               // 9
        "}\n",                                                            // 10
        "\n",                                                             // 11
        "class Controller {\n",                                           // 12
        "    public function handle(): void {\n",                         // 13
        "        session()->\n",                                          // 14
        "    }\n",                                                        // 15
        "}\n",                                                            // 16
    );

    // line 14: "        session()->"
    //           0       8      14 1719
    // character 19 is right after the final ->

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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 14,
                character: 19,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "get"),
        "Should offer 'get' from SessionStore via function @return docblock. Got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "put"),
        "Should offer 'put' from SessionStore via function @return docblock. Got: {:?}",
        names
    );
}

/// Test: Goto definition through a standalone function with `@return` docblock.
#[tokio::test]
async fn test_goto_definition_function_return_type_from_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // 0
        "class Application {\n",                           // 1
        "    public function abort(int $code): void {}\n", // 2
        "}\n",                                             // 3
        "\n",                                              // 4
        "/** @return Application */\n",                    // 5
        "function app()\n",                                // 6
        "{\n",                                             // 7
        "    return new Application();\n",                 // 8
        "}\n",                                             // 9
        "\n",                                              // 10
        "class Controller {\n",                            // 11
        "    public function handle(): void {\n",          // 12
        "        app()->abort(404);\n",                    // 13
        "    }\n",                                         // 14
        "}\n",                                             // 15
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

    // line 13: "        app()->abort(404);"
    //           0       8   12 14   19
    // Click on "abort" (character 15)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve app()->abort via @return docblock"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "abort() is declared on line 2"
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

// ─── Multiline Docblock ─────────────────────────────────────────────────────

/// Test: Multiline docblock with `@param` and `@return` tags.
#[tokio::test]
async fn test_completion_multiline_docblock_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                           // 0
        "class Connection {\n",                              // 1
        "    public function query(string $sql): void {}\n", // 2
        "    public function beginTransaction(): void {}\n", // 3
        "}\n",                                               // 4
        "\n",                                                // 5
        "class Database {\n",                                // 6
        "    /**\n",                                         // 7
        "     * Get the database connection.\n",             // 8
        "     *\n",                                          // 9
        "     * @param string $name The connection name\n",  // 10
        "     * @return Connection\n",                       // 11
        "     * @throws \\RuntimeException\n",               // 12
        "     */\n",                                         // 13
        "    public function connection($name = null)\n",    // 14
        "    {\n",                                           // 15
        "        return new Connection();\n",                // 16
        "    }\n",                                           // 17
        "\n",                                                // 18
        "    public function test(): void {\n",              // 19
        "        $this->connection()->\n",                   // 20
        "    }\n",                                           // 21
        "}\n",                                               // 22
    );

    // line 20: "        $this->connection()->"
    //           0       8              22  2628
    // character 28 is right after the final ->

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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 20,
                character: 29,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "query"),
        "Should offer 'query' from Connection via multiline @return docblock. Got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "beginTransaction"),
        "Should offer 'beginTransaction' from Connection. Got: {:?}",
        names
    );
}

// ─── Cross-file Docblock Resolution ─────────────────────────────────────────

/// Test: Property `@var` type that refers to a class in another file via PSR-4.
#[tokio::test]
async fn test_docblock_property_cross_file_psr4() {
    use std::fs;

    let dir = tempfile::tempdir().expect("failed to create temp dir");

    // Set up PSR-4 mapping
    fs::write(
        dir.path().join("composer.json"),
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
    )
    .unwrap();

    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    // Logger class in a separate file
    fs::write(
        src_dir.join("Logger.php"),
        concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "class Logger {\n",
            "    public function info(string $msg): void {}\n",
            "    public function error(string $msg): void {}\n",
            "}\n",
        ),
    )
    .unwrap();

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);

    // Open a file that uses @var Logger
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                             // 0
        "namespace App;\n",                    // 1
        "\n",                                  // 2
        "class Service {\n",                   // 3
        "    /** @var Logger */\n",            // 4
        "    private $logger;\n",              // 5
        "\n",                                  // 6
        "    public function run(): void {\n", // 7
        "        $this->logger->\n",           // 8
        "    }\n",                             // 9
        "}\n",                                 // 10
    );

    // line 8: "        $this->logger->"
    // character 24 is right after the final ->

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

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 8,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "info"),
        "Should offer 'info' from Logger via @var docblock + PSR-4. Got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "error"),
        "Should offer 'error' from Logger via @var docblock + PSR-4. Got: {:?}",
        names
    );
}

// ─── Guarded Function with Docblock ─────────────────────────────────────────

/// Test: Function inside `if (! function_exists(...))` guard with `@return`
/// docblock.  This combines the if-guard parsing with docblock resolution.
#[tokio::test]
async fn test_guarded_function_docblock_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                                        // 0
        "class Application {\n",                                                          // 1
        "    public function abort(int $code): void {}\n",                                // 2
        "    public function make(string $class): object { return new \\stdClass(); }\n", // 3
        "}\n",                                                                            // 4
        "\n",                                                                             // 5
        "if (! function_exists('app')) {\n",                                              // 6
        "    /**\n",                                                                      // 7
        "     * Get the app instance.\n",                                                 // 8
        "     *\n",                                                                       // 9
        "     * @return Application\n",                                                   // 10
        "     */\n",                                                                      // 11
        "    function app()\n",                                                           // 12
        "    {\n",                                                                        // 13
        "        return new Application();\n",                                            // 14
        "    }\n",                                                                        // 15
        "}\n",                                                                            // 16
        "\n",                                                                             // 17
        "class Controller {\n",                                                           // 18
        "    public function handle(): void {\n",                                         // 19
        "        app()->abort(404);\n",                                                   // 20
        "    }\n",                                                                        // 21
        "}\n",                                                                            // 22
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

    // line 20: "        app()->abort(404);"
    // Click on "abort" (character 15)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 20,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve app()->abort via @return docblock on guarded function"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "abort() is declared on line 2"
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

// ─── Docblock Overrides Class-typed Return ──────────────────────────────────

/// Test: `@return ConcreteSession` overrides `SessionInterface` return type.
/// A docblock class name should override another class name (refinement).
#[tokio::test]
async fn test_docblock_return_overrides_interface_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "interface SessionInterface {\n",                        // 1
        "    public function getId(): string;\n",                // 2
        "}\n",                                                   // 3
        "\n",                                                    // 4
        "class ConcreteSession implements SessionInterface {\n", // 5
        "    public function getId(): string { return ''; }\n",  // 6
        "    public function regenerate(): void {}\n",           // 7
        "}\n",                                                   // 8
        "\n",                                                    // 9
        "class Factory {\n",                                     // 10
        "    /** @return ConcreteSession */\n",                  // 11
        "    public function make(): SessionInterface\n",        // 12
        "    {\n",                                               // 13
        "        return new ConcreteSession();\n",               // 14
        "    }\n",                                               // 15
        "\n",                                                    // 16
        "    public function test(): void {\n",                  // 17
        "        $this->make()->\n",                             // 18
        "    }\n",                                               // 19
        "}\n",                                                   // 20
    );

    // line 18: "        $this->make()->"
    // character 23 is right after the final ->

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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 18,
                character: 23,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "regenerate"),
        "Should offer 'regenerate' from ConcreteSession — @return overrides interface. Got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "getId"),
        "Should offer 'getId' from ConcreteSession. Got: {:?}",
        names
    );
}

// ─── Parser-level Docblock Tests ────────────────────────────────────────────

/// Test: `parse_php` extracts method return types from docblocks.
#[tokio::test]
async fn test_parse_php_method_return_type_from_docblock() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Service {\n",
        "    /** @return Connection */\n",
        "    public function getConnection()\n",
        "    {\n",
        "        return null;\n",
        "    }\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let method = classes[0]
        .methods
        .iter()
        .find(|m| m.name == "getConnection")
        .expect("Should have getConnection method");

    assert_eq!(
        method.return_type_str().as_deref(),
        Some("Connection"),
        "Method should have return type from @return docblock"
    );
}

/// Test: `parse_php` extracts property types from `@var` docblocks.
#[tokio::test]
async fn test_parse_php_property_type_from_docblock() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Service {\n",
        "    /** @var Logger */\n",
        "    protected $logger;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "logger")
        .expect("Should have logger property");

    assert_eq!(
        prop.type_hint_str().as_deref(),
        Some("Logger"),
        "Property should have type from @var docblock"
    );
}

/// Test: `parse_php` — docblock `@var` overrides `object` but not `int`.
#[tokio::test]
async fn test_parse_php_docblock_override_compatibility() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "class Service {\n",
        "    /** @var Logger */\n",
        "    protected object $objectProp;\n",
        "\n",
        "    /** @var Logger */\n",
        "    protected int $intProp;\n",
        "\n",
        "    /** @var Logger */\n",
        "    protected mixed $mixedProp;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    let object_prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "objectProp")
        .expect("Should have objectProp");
    assert_eq!(
        object_prop.type_hint_str().as_deref(),
        Some("Logger"),
        "object should be overridden by @var Logger"
    );

    let int_prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "intProp")
        .expect("Should have intProp");
    assert_eq!(
        int_prop.type_hint_str().as_deref(),
        Some("int"),
        "int should NOT be overridden by @var Logger"
    );

    let mixed_prop = classes[0]
        .properties
        .iter()
        .find(|p| p.name == "mixedProp")
        .expect("Should have mixedProp");
    assert_eq!(
        mixed_prop.type_hint_str().as_deref(),
        Some("Logger"),
        "mixed should be overridden by @var Logger"
    );
}

/// Test: `parse_functions` extracts return types from docblocks on standalone
/// functions.
#[tokio::test]
async fn test_parse_functions_return_type_from_docblock() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * Get the application instance.\n",
        " *\n",
        " * @param string|null $abstract\n",
        " * @return Application\n",
        " */\n",
        "function app($abstract = null)\n",
        "{\n",
        "    return Container::getInstance();\n",
        "}\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].name, "app");
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("Application"),
        "Function should have return type from @return docblock"
    );
}

/// Test: `parse_functions` — docblock `@return` overrides `mixed` but not `int`.
#[tokio::test]
async fn test_parse_functions_docblock_override_compatibility() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/** @return Application */\n",
        "function makeApp(): mixed { return null; }\n",
        "\n",
        "/** @return Application */\n",
        "function getCount(): int { return 0; }\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(functions.len(), 2);

    let make_app = functions.iter().find(|f| f.name == "makeApp").unwrap();
    assert_eq!(
        make_app.return_type_str().as_deref(),
        Some("Application"),
        "mixed should be overridden by @return Application"
    );

    let get_count = functions.iter().find(|f| f.name == "getCount").unwrap();
    assert_eq!(
        get_count.return_type_str().as_deref(),
        Some("int"),
        "int should NOT be overridden by @return Application"
    );
}

/// Test: `@return` with nullable union like `Application|null`.
#[tokio::test]
async fn test_docblock_return_nullable_union() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/** @return Application|null */\n",
        "function maybeApp() { return null; }\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(functions.len(), 1);
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("Application|null"),
        "@return Application|null should preserve the nullable union"
    );
}

/// Test: Docblock with generic type `Collection<int, Model>` strips to `Collection`.
#[tokio::test]
async fn test_docblock_return_generic_type_stripped() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/** @return Collection<int, Model> */\n",
        "function getModels() { return []; }\n",
    );

    let functions = backend.parse_functions(php);
    assert_eq!(functions.len(), 1);
    assert_eq!(
        functions[0].return_type_str().as_deref(),
        Some("Collection<int, Model>"),
        "@return Collection<int, Model> should preserve generic parameters"
    );
}

// ─── @property Docblock Tags ────────────────────────────────────────────────

/// Test: `@property` tags are NOT parsed eagerly into `ClassInfo.properties`.
/// Instead, the raw docblock is preserved on `ClassInfo.class_docblock` and
/// properties are provided lazily by the `PHPDocProvider` via
/// `resolve_class_fully`.
#[tokio::test]
async fn test_parse_php_class_property_tags() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @property null|int                    $latest_subscription_agreement_id\n",
        " * @property UserMobileVerificationState $mobile_verification_state\n",
        " */\n",
        "class Customer {\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    // After parsing, @property tags are NOT in ClassInfo.properties.
    assert_eq!(
        classes[0].properties.len(),
        0,
        "@property tags should not be eagerly parsed into properties"
    );

    // The raw docblock is preserved for deferred parsing.
    assert!(
        classes[0].class_docblock.is_some(),
        "Raw class docblock should be preserved"
    );

    // After resolve_class_fully, virtual properties appear.
    let no_loader = |_: &str| -> Option<std::sync::Arc<phpantom_lsp::ClassInfo>> { None };
    let merged = phpantom_lsp::resolve_class_fully(&classes[0], &no_loader);
    assert_eq!(merged.properties.len(), 2);

    let id_prop = merged
        .properties
        .iter()
        .find(|p| p.name == "latest_subscription_agreement_id")
        .expect("Should have latest_subscription_agreement_id property");
    assert_eq!(
        id_prop.type_hint_str().as_deref(),
        Some("null|int"),
        "null|int should be preserved in the type hint"
    );
    assert!(!id_prop.is_static, "@property should not be static");

    let state_prop = merged
        .properties
        .iter()
        .find(|p| p.name == "mobile_verification_state")
        .expect("Should have mobile_verification_state property");
    assert_eq!(
        state_prop.type_hint_str().as_deref(),
        Some("UserMobileVerificationState")
    );
}

/// Test: `@property` with a class type enables chained completion.
/// `$customer->mobile_verification_state->` should offer members of
/// `UserMobileVerificationState`.
#[tokio::test]
async fn test_completion_via_property_tag() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                   // 0
        "class UserMobileVerificationState {\n",                     // 1
        "    public function isVerified(): bool { return true; }\n", // 2
        "    public function getCode(): string { return ''; }\n",    // 3
        "}\n",                                                       // 4
        "\n",                                                        // 5
        "/**\n",                                                     // 6
        " * @property UserMobileVerificationState $mobile_state\n",  // 7
        " */\n",                                                     // 8
        "class Customer {\n",                                        // 9
        "    public function test(): void {\n",                      // 10
        "        $this->mobile_state->\n",                           // 11
        "    }\n",                                                   // 12
        "}\n",                                                       // 13
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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 29,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "isVerified"),
        "Should offer 'isVerified' from UserMobileVerificationState via @property tag. Got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "getCode"),
        "Should offer 'getCode' from UserMobileVerificationState via @property tag. Got: {:?}",
        names
    );
}

/// Test: A real declared property takes precedence over a `@property` tag
/// with the same name.  After `resolve_class_fully`, the PHPDocProvider's
/// virtual property is suppressed by the real declaration.
#[tokio::test]
async fn test_real_property_overrides_property_tag() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @property string $name\n",
        " */\n",
        "class Customer {\n",
        "    protected int $name;\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    // After parsing, only the real declared property is present.
    assert_eq!(classes[0].properties.len(), 1);
    assert_eq!(
        classes[0].properties[0].type_hint_str().as_deref(),
        Some("int")
    );

    // After resolve_class_fully, still only one — the virtual @property
    // is suppressed because a real property with the same name exists.
    let no_loader = |_: &str| -> Option<std::sync::Arc<phpantom_lsp::ClassInfo>> { None };
    let merged = phpantom_lsp::resolve_class_fully(&classes[0], &no_loader);

    let name_props: Vec<_> = merged
        .properties
        .iter()
        .filter(|p| p.name == "name")
        .collect();
    assert_eq!(
        name_props.len(),
        1,
        "Real property should shadow the @property tag"
    );
    assert_eq!(
        name_props[0].type_hint_str().as_deref(),
        Some("int"),
        "Real declared type should win over @property type"
    );
}

/// Test: `@property-read` tags are provided lazily via `resolve_class_fully`.
#[tokio::test]
async fn test_parse_php_property_read_tag() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @property-read Session $session\n",
        " */\n",
        "class Controller {\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    // Not eagerly parsed.
    assert_eq!(classes[0].properties.len(), 0);

    // Available after resolve_class_fully.
    let no_loader = |_: &str| -> Option<std::sync::Arc<phpantom_lsp::ClassInfo>> { None };
    let merged = phpantom_lsp::resolve_class_fully(&classes[0], &no_loader);

    let prop = merged
        .properties
        .iter()
        .find(|p| p.name == "session")
        .expect("Should have session property from @property-read");
    assert_eq!(prop.type_hint_str().as_deref(), Some("Session"));
}

/// Test: Goto definition on a magic property jumps to the `@property` line
/// in the class docblock.
#[tokio::test]
async fn test_goto_definition_property_tag_jumps_to_docblock_line() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                   // 0
        "class VerificationState {\n",                               // 1
        "    public function isVerified(): bool { return true; }\n", // 2
        "}\n",                                                       // 3
        "\n",                                                        // 4
        "/**\n",                                                     // 5
        " * @property null|int             $agreement_id\n",         // 6
        " * @property VerificationState    $verification_state\n",   // 7
        " */\n",                                                     // 8
        "class Customer {\n",                                        // 9
        "    public function test(): void {\n",                      // 10
        "        $this->agreement_id;\n",                            // 11
        "    }\n",                                                   // 12
        "}\n",                                                       // 13
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

    // line 11: "        $this->agreement_id;"
    //           0       8      14    20
    // Click on "agreement_id" (character 15)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->agreement_id to @property line"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 6,
                "@property $agreement_id is declared on line 6"
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

/// Test: Goto definition on a method chained through a `@property` tag resolves
/// to the method declaration in the target class.
#[tokio::test]
async fn test_goto_definition_chained_via_property_tag() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                   // 0
        "class VerificationState {\n",                               // 1
        "    public function isVerified(): bool { return true; }\n", // 2
        "}\n",                                                       // 3
        "\n",                                                        // 4
        "/**\n",                                                     // 5
        " * @property VerificationState $verification_state\n",      // 6
        " */\n",                                                     // 7
        "class Customer {\n",                                        // 8
        "    public function test(): void {\n",                      // 9
        "        $this->verification_state->isVerified();\n",        // 10
        "    }\n",                                                   // 11
        "}\n",                                                       // 12
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

    // line 10: "        $this->verification_state->isVerified();"
    //           0       8      14                 34  38
    // Click on "isVerified" (character 35)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: 35,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->verification_state->isVerified() via @property tag"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "isVerified() is declared on line 2 of VerificationState"
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

/// Test: Goto definition on a `@property-read` property jumps to the docblock line.
#[tokio::test]
async fn test_goto_definition_property_read_tag() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                              // 0
        "/**\n",                                // 1
        " * @property-read string $name\n",     // 2
        " */\n",                                // 3
        "class Model {\n",                      // 4
        "    public function test(): void {\n", // 5
        "        $this->name;\n",               // 6
        "    }\n",                              // 7
        "}\n",                                  // 8
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

    // line 6: "        $this->name;"
    // Click on "name" (character 15)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->name to @property-read line"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "@property-read $name is declared on line 2"
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

// ─── @method Docblock Tags ──────────────────────────────────────────────────

/// Test: `@method` tags are NOT parsed eagerly into `ClassInfo.methods`.
/// Instead, the raw docblock is preserved on `ClassInfo.class_docblock` and
/// methods are provided lazily by the `PHPDocProvider` via
/// `resolve_class_fully`.
#[tokio::test]
async fn test_parse_php_class_method_tags() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @method \\Mockery\\MockInterface mock(string $abstract, callable():mixed $mockDefinition = null)\n",
        " * @method assertDatabaseHas(string $table, array<string, mixed> $data, string $connection = null)\n",
        " * @method static Decimal getAmountUntilBonusCashIsTriggered()\n",
        " */\n",
        "class Cart {\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    // After parsing, @method tags are NOT in ClassInfo.methods.
    assert_eq!(
        classes[0].methods.len(),
        0,
        "@method tags should not be eagerly parsed into methods"
    );

    // The raw docblock is preserved for deferred parsing.
    assert!(
        classes[0].class_docblock.is_some(),
        "Raw class docblock should be preserved"
    );

    // After resolve_class_fully, virtual methods appear.
    let no_loader = |_: &str| -> Option<std::sync::Arc<phpantom_lsp::ClassInfo>> { None };
    let merged = phpantom_lsp::resolve_class_fully(&classes[0], &no_loader);
    assert_eq!(merged.methods.len(), 3);

    let mock_method = merged
        .methods
        .iter()
        .find(|m| m.name == "mock")
        .expect("Should have mock method from @method tag");
    assert_eq!(
        mock_method.return_type_str().as_deref(),
        Some("\\Mockery\\MockInterface"),
        "FQN return type should preserve leading backslash"
    );
    assert!(!mock_method.is_static, "mock should not be static");
    assert_eq!(mock_method.parameters.len(), 2);

    let assert_method = merged
        .methods
        .iter()
        .find(|m| m.name == "assertDatabaseHas")
        .expect("Should have assertDatabaseHas method from @method tag");
    assert!(
        assert_method.return_type.is_none(),
        "assertDatabaseHas has no return type"
    );
    assert_eq!(assert_method.parameters.len(), 3);
    assert!(!assert_method.parameters[2].is_required);

    let static_method = merged
        .methods
        .iter()
        .find(|m| m.name == "getAmountUntilBonusCashIsTriggered")
        .expect("Should have getAmountUntilBonusCashIsTriggered method from @method tag");
    assert_eq!(static_method.return_type_str().as_deref(), Some("Decimal"));
    assert!(static_method.is_static);
    assert!(static_method.parameters.is_empty());
}

/// Test: `@method` with a class return type enables chained completion.
/// `$cart->mock("Foo")->` should offer members of `MockInterface`.
#[tokio::test]
async fn test_completion_via_method_tag() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                   // 0
        "class MockInterface {\n",                                   // 1
        "    public function shouldReceive(string $name): self {\n", // 2
        "        return $this;\n",                                   // 3
        "    }\n",                                                   // 4
        "    public function andReturn(mixed $value): self {\n",     // 5
        "        return $this;\n",                                   // 6
        "    }\n",                                                   // 7
        "}\n",                                                       // 8
        "\n",                                                        // 9
        "/**\n",                                                     // 10
        " * @method MockInterface mock(string $abstract)\n",         // 11
        " */\n",                                                     // 12
        "class TestCase {\n",                                        // 13
        "    public function test(): void {\n",                      // 14
        "        $this->mock('Foo')->\n",                            // 15
        "    }\n",                                                   // 16
        "}\n",                                                       // 17
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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 15,
                character: 29,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "shouldReceive"),
        "Should offer 'shouldReceive' from MockInterface via @method return type. Got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "andReturn"),
        "Should offer 'andReturn' from MockInterface via @method return type. Got: {:?}",
        names
    );
}

/// Test: `@method` tags appear in completion for the class itself.
/// `$this->` inside a class with `@method` tags should offer those methods.
#[tokio::test]
async fn test_completion_method_tag_on_this() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "/**\n",                                   // 1
        " * @method string getName()\n",           // 2
        " * @method void setName(string $name)\n", // 3
        " */\n",                                   // 4
        "class Model {\n",                         // 5
        "    public function test(): void {\n",    // 6
        "        $this->\n",                       // 7
        "    }\n",                                 // 8
        "}\n",                                     // 9
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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "getName"),
        "Should offer 'getName' from @method tag on $this->. Got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "setName"),
        "Should offer 'setName' from @method tag on $this->. Got: {:?}",
        names
    );
}

/// Test: `@method static` tags appear in `ClassName::` completion.
#[tokio::test]
async fn test_completion_static_method_tag_via_double_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                          // 0
        "/**\n",                                                            // 1
        " * @method static Decimal getAmountUntilBonusCashIsTriggered()\n", // 2
        " */\n",                                                            // 3
        "class Cart {\n",                                                   // 4
        "}\n",                                                              // 5
        "\n",                                                               // 6
        "function test() {\n",                                              // 7
        "    Cart::\n",                                                     // 8
        "}\n",                                                              // 9
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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 8,
                character: 10,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names
            .iter()
            .any(|n| n == "getAmountUntilBonusCashIsTriggered"),
        "Should offer static @method via `Cart::`. Got: {:?}",
        names
    );
}

/// Test: A real declared method takes precedence over a `@method` tag
/// with the same name.  After `resolve_class_fully`, the PHPDocProvider's
/// virtual method is suppressed by the real declaration.
#[tokio::test]
async fn test_real_method_overrides_method_tag() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @method string getName()\n",
        " */\n",
        "class Model {\n",
        "    public function getName(): int { return 42; }\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);

    // After parsing, only the real declared method is present.
    assert_eq!(classes[0].methods.len(), 1);
    assert_eq!(
        classes[0].methods[0].return_type_str().as_deref(),
        Some("int")
    );

    // After resolve_class_fully, still only one — the virtual @method
    // is suppressed because a real method with the same name exists.
    let no_loader = |_: &str| -> Option<std::sync::Arc<phpantom_lsp::ClassInfo>> { None };
    let merged = phpantom_lsp::resolve_class_fully(&classes[0], &no_loader);

    let name_methods: Vec<_> = merged
        .methods
        .iter()
        .filter(|m| m.name == "getName")
        .collect();
    assert_eq!(
        name_methods.len(),
        1,
        "Real method should prevent duplicate from @method tag"
    );

    // The real declaration should win (return type int, not string).
    assert_eq!(name_methods[0].return_type_str().as_deref(), Some("int"));
}

/// Test: Goto definition on a magic method jumps to the `@method` line
/// in the class docblock.
#[tokio::test]
async fn test_goto_definition_method_tag_jumps_to_docblock_line() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                 // 0
        "/**\n",                                   // 1
        " * @method string getName()\n",           // 2
        " * @method void setName(string $name)\n", // 3
        " */\n",                                   // 4
        "class Model {\n",                         // 5
        "    public function test(): void {\n",    // 6
        "        $this->getName();\n",             // 7
        "    }\n",                                 // 8
        "}\n",                                     // 9
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

    // line 7: "        $this->getName();"
    //          0       8      15
    // Click on "getName" (character 15)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->getName() to @method line"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "@method getName is declared on line 2"
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

/// Test: Goto definition on a chained method from a `@method` return type
/// resolves to the method declaration in the target class.
#[tokio::test]
async fn test_goto_definition_chained_via_method_tag() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                   // 0
        "class MockInterface {\n",                                   // 1
        "    public function shouldReceive(string $name): self {\n", // 2
        "        return $this;\n",                                   // 3
        "    }\n",                                                   // 4
        "}\n",                                                       // 5
        "\n",                                                        // 6
        "/**\n",                                                     // 7
        " * @method MockInterface mock(string $abstract)\n",         // 8
        " */\n",                                                     // 9
        "class TestCase {\n",                                        // 10
        "    public function test(): void {\n",                      // 11
        "        $this->mock('Foo')->shouldReceive('bar');\n",       // 12
        "    }\n",                                                   // 13
        "}\n",                                                       // 14
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

    // line 12: "        $this->mock('Foo')->shouldReceive('bar');"
    //           0       8      14          25   30  34
    // Click on "shouldReceive" (character 29)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 29,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->mock('Foo')->shouldReceive() via @method return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "shouldReceive() is declared on line 2 of MockInterface"
            );
        }
        other => panic!("Expected Scalar, got: {:?}", other),
    }
}

/// Test: `@method` tags on traits are provided lazily via `resolve_class_fully`.
#[tokio::test]
async fn test_parse_php_trait_method_tags() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @method string greet()\n",
        " */\n",
        "trait Greeter {\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(
        classes[0].methods.len(),
        0,
        "@method not eagerly parsed on traits"
    );

    let no_loader = |_: &str| -> Option<std::sync::Arc<phpantom_lsp::ClassInfo>> { None };
    let merged = phpantom_lsp::resolve_class_fully(&classes[0], &no_loader);
    assert_eq!(merged.methods.len(), 1);
    assert_eq!(merged.methods[0].name, "greet");
    assert_eq!(
        merged.methods[0].return_type_str().as_deref(),
        Some("string")
    );
}

/// Test: `@method` tags on interfaces are provided lazily via `resolve_class_fully`.
#[tokio::test]
async fn test_parse_php_interface_method_tags() {
    let backend = create_test_backend();
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @method static self create(array $attributes)\n",
        " */\n",
        "interface Factory {\n",
        "}\n",
    );

    let classes = backend.parse_php(php);
    assert_eq!(classes.len(), 1);
    assert_eq!(
        classes[0].methods.len(),
        0,
        "@method not eagerly parsed on interfaces"
    );

    let no_loader = |_: &str| -> Option<std::sync::Arc<phpantom_lsp::ClassInfo>> { None };
    let merged = phpantom_lsp::resolve_class_fully(&classes[0], &no_loader);
    assert_eq!(merged.methods.len(), 1);
    assert_eq!(merged.methods[0].name, "create");
    assert!(merged.methods[0].is_static);
    assert_eq!(merged.methods[0].return_type_str().as_deref(), Some("self"));
    assert_eq!(merged.methods[0].parameters.len(), 1);
    assert_eq!(merged.methods[0].parameters[0].name, "$attributes");
}

/// Test: An inline `@var \Redis` with a leading backslash resolves to the
/// global class, bypassing a same-short-name import.
///
/// When `use App\Facades\Redis;` imports a facade named `Redis`, the
/// leading backslash in `@var \Redis` must force resolution to the global
/// `Redis` class instead of the imported facade.
#[tokio::test]
async fn test_var_leading_backslash_bypasses_same_short_name_import() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                            // 0
        "namespace App\\Facades {\n",                         // 1
        "    class Redis {\n",                                // 2
        "        public function facadeOnly(): void {}\n",    // 3
        "    }\n",                                            // 4
        "}\n",                                                // 5
        "namespace {\n",                                      // 6
        "    class Redis {\n",                                // 7
        "        public function select(int $db): void {}\n", // 8
        "    }\n",                                            // 9
        "}\n",                                                // 10
        "namespace App {\n",                                  // 11
        "    use App\\Facades\\Redis;\n",                     // 12
        "    function run(): void {\n",                       // 13
        "        /** @var \\Redis */\n",                      // 14
        "        $client = something();\n",                   // 15
        "        $client->\n",                                // 16
        "    }\n",                                            // 17
        "}\n",                                                // 18
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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 16,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap().unwrap();
    let names = completion_names(result);
    assert!(
        names.iter().any(|n| n == "select"),
        "\\Redis should resolve to the global class (offering 'select'), not the facade. Got: {:?}",
        names
    );
    assert!(
        !names.iter().any(|n| n == "facadeOnly"),
        "\\Redis must not resolve to the imported facade. Got: {:?}",
        names
    );
}
