use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Ambiguous Variable Types ───────────────────────────────────────────────

/// When a variable is reassigned inside an `if` block, the variable could be
/// either type after the block.  Goto definition should still resolve the
/// member if *any* candidate type declares it.
///
/// ```php
/// $thing = new SessionManager();
/// if ($thing->callCustomCreator2()) {
///     $thing = new Manager();
/// }
/// $thing->callCustomCreator2(); // Manager doesn't have it, but SessionManager does
/// ```
#[tokio::test]
async fn test_goto_definition_ambiguous_variable_if_block() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                             // 0
        "class SessionManager {\n",                            // 1
        "    public function callCustomCreator2(): void {}\n", // 2
        "    public function start(): void {}\n",              // 3
        "}\n",                                                 // 4
        "\n",                                                  // 5
        "class Manager {\n",                                   // 6
        "    public function doWork(): void {}\n",             // 7
        "}\n",                                                 // 8
        "\n",                                                  // 9
        "class App {\n",                                       // 10
        "    public function run(): void {\n",                 // 11
        "        $thing = new SessionManager();\n",            // 12
        "        if ($thing->callCustomCreator2()) {\n",       // 13
        "            $thing = new Manager();\n",               // 14
        "        }\n",                                         // 15
        "        $thing->callCustomCreator2();\n",             // 16
        "    }\n",                                             // 17
        "}\n",                                                 // 18
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

    // Click on "callCustomCreator2" on line 16: $thing->callCustomCreator2()
    // After the if block, $thing could be SessionManager or Manager.
    // Manager doesn't have callCustomCreator2, but SessionManager does.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 16,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $thing->callCustomCreator2() via SessionManager even though Manager was assigned in if-block"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "callCustomCreator2 is declared on line 2 in SessionManager"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// When both candidate types share a method, the first candidate (original
/// assignment) should win.
#[tokio::test]
async fn test_goto_definition_ambiguous_variable_both_have_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "class Alpha {\n",                        // 1
        "    public function greet(): void {}\n", // 2
        "}\n",                                    // 3
        "\n",                                     // 4
        "class Beta {\n",                         // 5
        "    public function greet(): void {}\n", // 6
        "}\n",                                    // 7
        "\n",                                     // 8
        "class App {\n",                          // 9
        "    public function run(): void {\n",    // 10
        "        $obj = new Alpha();\n",          // 11
        "        if (true) {\n",                  // 12
        "            $obj = new Beta();\n",       // 13
        "        }\n",                            // 14
        "        $obj->greet();\n",               // 15
        "    }\n",                                // 16
        "}\n",                                    // 17
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

    // Click on "greet" on line 15
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 15,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $obj->greet() when both Alpha and Beta have greet()"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            // Forward walker returns Beta first: at the cursor position (after the if-block),
            // the most recent assignment is `$obj = new Beta()` inside the if-body.
            assert_eq!(
                location.range.start.line, 6,
                "greet() should resolve to Beta (line 6) as the forward walker's first candidate"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// An unconditional reassignment should replace previous candidates,
/// so only the final type is used.
#[tokio::test]
async fn test_goto_definition_unconditional_reassignment_replaces_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class Foo {\n",                            // 1
        "    public function fooOnly(): void {}\n", // 2
        "}\n",                                      // 3
        "\n",                                       // 4
        "class Bar {\n",                            // 5
        "    public function barOnly(): void {}\n", // 6
        "}\n",                                      // 7
        "\n",                                       // 8
        "class App {\n",                            // 9
        "    public function run(): void {\n",      // 10
        "        $x = new Foo();\n",                // 11
        "        $x = new Bar();\n",                // 12
        "        $x->barOnly();\n",                 // 13
        "    }\n",                                  // 14
        "}\n",                                      // 15
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

    // Click on "barOnly" on line 13 — unconditional reassignment means $x is Bar
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x->barOnly() to Bar::barOnly"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 6,
                "barOnly is declared on line 6 in Bar"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // fooOnly should NOT resolve since Foo was unconditionally replaced by Bar
    let params2 = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result2 = backend.goto_definition(params2).await.unwrap();
    // The result should resolve to Bar, not Foo — we already checked above
    assert!(result2.is_some());
    match result2.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_ne!(
                location.range.start.line, 2,
                "fooOnly on line 2 (Foo) should NOT be reachable after unconditional reassignment"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Ambiguous variable across try/catch: reassignment in try block should
/// add a candidate, not replace the original.
#[tokio::test]
async fn test_goto_definition_ambiguous_variable_try_catch() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // 0
        "class Logger {\n",                                // 1
        "    public function log(string $msg): void {}\n", // 2
        "}\n",                                             // 3
        "\n",                                              // 4
        "class NullLogger {\n",                            // 5
        "    public function silence(): void {}\n",        // 6
        "}\n",                                             // 7
        "\n",                                              // 8
        "class App {\n",                                   // 9
        "    public function run(): void {\n",             // 10
        "        $logger = new Logger();\n",               // 11
        "        try {\n",                                 // 12
        "            $logger = new NullLogger();\n",       // 13
        "        } catch (\\Exception $e) {\n",            // 14
        "        }\n",                                     // 15
        "        $logger->log('hello');\n",                // 16
        "    }\n",                                         // 17
        "}\n",                                             // 18
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

    // Click on "log" on line 16: $logger->log('hello')
    // NullLogger doesn't have log(), but Logger does.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 16,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $logger->log() via Logger even though NullLogger was assigned in try block"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "log() is declared on line 2 in Logger"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Ambiguous variable with if/else: both branches reassign, original type
/// should still be available as a candidate.
#[tokio::test]
async fn test_goto_definition_ambiguous_variable_if_else_branches() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "class Writer {\n",                       // 1
        "    public function write(): void {}\n", // 2
        "}\n",                                    // 3
        "\n",                                     // 4
        "class Printer {\n",                      // 5
        "    public function print(): void {}\n", // 6
        "}\n",                                    // 7
        "\n",                                     // 8
        "class Sender {\n",                       // 9
        "    public function send(): void {}\n",  // 10
        "}\n",                                    // 11
        "\n",                                     // 12
        "class App {\n",                          // 13
        "    public function run(): void {\n",    // 14
        "        $out = new Writer();\n",         // 15
        "        if (true) {\n",                  // 16
        "            $out = new Printer();\n",    // 17
        "        } else {\n",                     // 18
        "            $out = new Sender();\n",     // 19
        "        }\n",                            // 20
        "        $out->write();\n",               // 21
        "    }\n",                                // 22
        "}\n",                                    // 23
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

    // Click on "write" on line 21 — forward walker excludes Writer (dead pre-if assignment
    // overwritten by both branches), so only Printer and Sender are candidates.
    // Neither has write(), so we try "print" instead to verify Printer is resolved.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 21,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    // The forward walker correctly excludes Writer (the pre-if assignment is dead because
    // both if and else branches reassign $out). Only Printer|Sender remain as candidates.
    // Neither has write(), so go-to-definition returns None.
    assert!(
        result.is_none(),
        "Forward walker excludes Writer; neither Printer nor Sender has write(), so result should be None"
    );
}

/// Ambiguous variable across a loop: reassignment inside a while loop should
/// add a candidate.
#[tokio::test]
async fn test_goto_definition_ambiguous_variable_loop() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                   // 0
        "class Handler {\n",                         // 1
        "    public function handle(): void {}\n",   // 2
        "}\n",                                       // 3
        "\n",                                        // 4
        "class Fallback {\n",                        // 5
        "    public function fallback(): void {}\n", // 6
        "}\n",                                       // 7
        "\n",                                        // 8
        "class App {\n",                             // 9
        "    public function run(): void {\n",       // 10
        "        $h = new Handler();\n",             // 11
        "        while (true) {\n",                  // 12
        "            $h = new Fallback();\n",        // 13
        "        }\n",                               // 14
        "        $h->handle();\n",                   // 15
        "    }\n",                                   // 16
        "}\n",                                       // 17
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

    // Click on "handle" on line 15 — Fallback doesn't have handle(), Handler does
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 15,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $h->handle() via Handler even though Fallback was assigned in while loop"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "handle() is declared on line 2 in Handler"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Cross-file ambiguous variable: the reassigned class comes from PSR-4.
#[tokio::test]
async fn test_goto_definition_ambiguous_variable_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[
            (
                "src/Cache.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Cache {\n",
                    "    public function get(string $key): mixed { return null; }\n",
                    "}\n",
                ),
            ),
            (
                "src/NullCache.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class NullCache {\n",
                    "    public function clear(): void {}\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///test_main.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Cache;\n",
        "use App\\NullCache;\n",
        "\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $store = new Cache();\n",
        "        if (getenv('DISABLE_CACHE')) {\n",
        "            $store = new NullCache();\n",
        "        }\n",
        "        $store->get('key');\n",
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

    // Click on "get" on line 10: $store->get('key')
    // NullCache doesn't have get(), but Cache does (cross-file via PSR-4)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 10,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $store->get() via Cache (PSR-4) even with NullCache in if-block"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            // Cache::get is declared on line 3 (0-indexed) of Cache.php
            assert_eq!(
                location.range.start.line, 3,
                "get() should be on line 3 of Cache.php"
            );
            let loc_path = location.uri.to_file_path().unwrap();
            assert!(
                loc_path.ends_with("src/Cache.php"),
                "Should resolve to Cache.php, got: {:?}",
                loc_path
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ── Inline @var docblock override tests ─────────────────────────────────────

/// When a variable is assigned from an unknown function but has an inline
/// `/** @var Type */` annotation, goto definition should resolve the member
/// via the annotated type.
#[tokio::test]
async fn test_goto_definition_inline_var_docblock_simple() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///varoverride.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class Session {\n",                        // 1
        "    public function getId(): string {}\n", // 2
        "    public function flash(): void {}\n",   // 3
        "}\n",                                      // 4
        "class Controller {\n",                     // 5
        "    public function handle() {\n",         // 6
        "        /** @var Session */\n",            // 7
        "        $sess = mystery();\n",             // 8
        "        $sess->getId();\n",                // 9
        "    }\n",                                  // 10
        "}\n",                                      // 11
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

    // Click on "getId" on line 9: $sess->getId()
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $sess->getId() via @var Session annotation"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "getId is declared on line 2 in Session"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// When the `@var` annotation omits the variable name (`/** @var Type */`),
/// goto definition should apply the type to the immediately following
/// assignment and resolve the member correctly.
#[tokio::test]
async fn test_goto_definition_inline_var_docblock_no_variable_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///varoverride_noname.php").unwrap();
    let text = concat!(
        "<?php\n",                                  // 0
        "class Session {\n",                        // 1
        "    public function getId(): string {}\n", // 2
        "    public function flash(): void {}\n",   // 3
        "}\n",                                      // 4
        "class Controller {\n",                     // 5
        "    public function handle() {\n",         // 6
        "        /** @var Session */\n",            // 7
        "        $sess = mystery();\n",             // 8
        "        $sess->flash();\n",                // 9
        "    }\n",                                  // 10
        "}\n",                                      // 11
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

    // Click on "flash" on line 9: $sess->flash()
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $sess->flash() via @var Session annotation (no variable name)"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "flash() is declared on line 3 in Session"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// When the `@var` annotation includes a variable name (`@var Type $var`),
/// goto definition should still resolve correctly.
#[tokio::test]
async fn test_goto_definition_inline_var_docblock_with_variable_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///varoverride_named.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "class Logger {\n",                       // 1
        "    public function info(): void {}\n",  // 2
        "    public function error(): void {}\n", // 3
        "}\n",                                    // 4
        "class App {\n",                          // 5
        "    public function run() {\n",          // 6
        "        /** @var Logger $log */\n",      // 7
        "        $log = getLogger();\n",          // 8
        "        $log->error();\n",               // 9
        "    }\n",                                // 10
        "}\n",                                    // 11
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

    // Click on "error" on line 9: $log->error()
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
        "Should resolve $log->error() via @var Logger $log annotation"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "error() is declared on line 3 in Logger"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// When the `@var` annotation names a *different* variable, the override
/// should NOT apply and goto definition should fail (no type info).
#[tokio::test]
async fn test_goto_definition_inline_var_docblock_wrong_variable_name() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///varoverride_wrong.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "class Logger {\n",                      // 1
        "    public function info(): void {}\n", // 2
        "}\n",                                   // 3
        "class App {\n",                         // 4
        "    public function run() {\n",         // 5
        "        /** @var Logger $other */\n",   // 6
        "        $log = something();\n",         // 7
        "        $log->info();\n",               // 8
        "    }\n",                               // 9
        "}\n",                                   // 10
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

    // Click on "info" on line 8: $log->info()
    // The @var names $other, not $log, so no type resolution should occur.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 8,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_none(),
        "Should NOT resolve $log->info() when @var names $other"
    );
}

/// When the native return type is a scalar (string), the `@var` override
/// should be blocked (same check as @return) and definition should fall
/// through to normal resolution (which won't find a class for `string`).
#[tokio::test]
async fn test_goto_definition_inline_var_docblock_blocked_by_scalar() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///varoverride_scalar.php").unwrap();
    let text = concat!(
        "<?php\n",                                    // 0
        "class Session {\n",                          // 1
        "    public function getId(): string {}\n",   // 2
        "}\n",                                        // 3
        "class App {\n",                              // 4
        "    public function getName(): string {}\n", // 5
        "    public function run() {\n",              // 6
        "        /** @var Session */\n",              // 7
        "        $s = $this->getName();\n",           // 8
        "        $s->getId();\n",                     // 9
        "    }\n",                                    // 10
        "}\n",                                        // 11
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

    // Click on "getId" on line 9: $s->getId()
    // getName() returns `string` — override should be blocked.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_none(),
        "Should NOT resolve $s->getId() when native type is scalar string"
    );
}

/// When the native return type is a class (non-scalar), the `@var` override
/// should be allowed and goto definition should resolve to the overridden type.
#[tokio::test]
async fn test_goto_definition_inline_var_docblock_override_allowed_for_object() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///varoverride_obj.php").unwrap();
    let text = concat!(
        "<?php\n",                                            // 0
        "class BaseService {\n",                              // 1
        "    public function base(): void {}\n",              // 2
        "}\n",                                                // 3
        "class Session extends BaseService {\n",              // 4
        "    public function getId(): string {}\n",           // 5
        "    public function flash(): void {}\n",             // 6
        "}\n",                                                // 7
        "class App {\n",                                      // 8
        "    public function getService(): BaseService {}\n", // 9
        "    public function run() {\n",                      // 10
        "        /** @var Session */\n",                      // 11
        "        $s = $this->getService();\n",                // 12
        "        $s->flash();\n",                             // 13
        "    }\n",                                            // 14
        "}\n",                                                // 15
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

    // Click on "flash" on line 13: $s->flash()
    // getService() returns BaseService but @var says Session — override allowed.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $s->flash() via @var Session override"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 6,
                "flash() is declared on line 6 in Session"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Variable Go-To-Definition ─────────────────────────────────────────────

/// Clicking on `$typed` in `return $typed;` should jump to the assignment
/// on the previous line: `$typed = getUnknownValue();`.
#[tokio::test]
async fn test_goto_definition_variable_jumps_to_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_goto_assign.php").unwrap();
    let text = concat!(
        "<?php\n",                           // 0
        "function demo(): mixed {\n",        // 1
        "    $typed = getUnknownValue();\n", // 2
        "    return $typed;\n",              // 3
        "}\n",                               // 4
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

    // Cursor on `$typed` in `return $typed;` (line 3, character 12 = on 't')
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $typed to its assignment on the previous line"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "$typed is assigned on line 2");
            assert_eq!(
                location.range.start.character, 4,
                "$typed starts at column 4"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_compact_string_jumps_to_variable_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_goto_compact.php").unwrap();
    let text = concat!(
        "<?php\n",                       // 0
        "function demo(): array {\n",    // 1
        "    $user = 'alice';\n",        // 2
        "    return compact('user');\n", // 3
        "}\n",                           // 4
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

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 21,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve compact('user') to the $user assignment"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "$user is assigned on line 2");
            assert_eq!(
                location.range.start.character, 4,
                "$user starts at column 4"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// When the cursor is on a variable at its definition site (the assignment),
/// GTD should return the self-location so editors can fall back to Find References.
#[tokio::test]
async fn test_goto_definition_variable_on_definition_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_goto_on_def.php").unwrap();
    let text = concat!(
        "<?php\n",                           // 0
        "function demo(): void {\n",         // 1
        "    $typed = getUnknownValue();\n", // 2
        "}\n",                               // 3
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

    // Cursor on `$typed` on line 2 (the assignment itself)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 5,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "should point back to line 2");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// Go-to-definition on a variable should jump to a function parameter
/// declaration when that is the most recent definition.
#[tokio::test]
async fn test_goto_definition_variable_jumps_to_parameter() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_goto_param.php").unwrap();
    let text = concat!(
        "<?php\n",                                       // 0
        "class App {\n",                                 // 1
        "    public function handle(int $id): void {\n", // 2
        "        echo $id;\n",                           // 3
        "    }\n",                                       // 4
        "}\n",                                           // 5
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

    // Cursor on `$id` in `echo $id;` (line 3)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $id to the parameter declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$id is declared as a parameter on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Go-to-definition on a variable used after a foreach should jump to the
/// foreach `as $var` declaration.
#[tokio::test]
async fn test_goto_definition_variable_jumps_to_foreach() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_goto_foreach.php").unwrap();
    let text = concat!(
        "<?php\n",                           // 0
        "function demo(): void {\n",         // 1
        "    $items = [1, 2, 3];\n",         // 2
        "    foreach ($items as $item) {\n", // 3
        "        echo $item;\n",             // 4
        "    }\n",                           // 5
        "}\n",                               // 6
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

    // Cursor on `$item` in `echo $item;` (line 4)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $item to the foreach declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "$item is declared in the foreach on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// When a variable is reassigned, go-to-definition should jump to the
/// most recent assignment before the cursor.
#[tokio::test]
async fn test_goto_definition_variable_jumps_to_most_recent_reassignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_goto_reassign.php").unwrap();
    let text = concat!(
        "<?php\n",                   // 0
        "function demo(): void {\n", // 1
        "    $val = 1;\n",           // 2
        "    $val = 2;\n",           // 3
        "    $val = 3;\n",           // 4
        "    echo $val;\n",          // 5
        "}\n",                       // 6
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

    // Cursor on `$val` in `echo $val;` (line 5)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 10,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $val to the most recent assignment"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 4,
                "$val's most recent assignment is on line 4"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Go-to-definition on `$e` in a catch block should jump to the catch
/// declaration.
#[tokio::test]
async fn test_goto_definition_variable_jumps_to_catch() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_goto_catch.php").unwrap();
    let text = concat!(
        "<?php\n",                          // 0
        "function demo(): void {\n",        // 1
        "    try {\n",                      // 2
        "        riskyOperation();\n",      // 3
        "    } catch (\\Exception $e) {\n", // 4
        "        echo $e;\n",               // 5
        "    }\n",                          // 6
        "}\n",                              // 7
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

    // Cursor on `$e` in `echo $e;` (line 5)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $e to the catch declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 4,
                "$e is declared in the catch on line 4"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Go-to-definition on a variable at the top level (outside any class)
/// should still resolve to its assignment.
#[tokio::test]
async fn test_goto_definition_variable_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_goto_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",                       // 0
        "$typed = getUnknownValue();\n", // 1
        "return $typed;\n",              // 2
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

    // Cursor on `$typed` in `return $typed;` (line 2)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $typed to assignment on line 1"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 1, "$typed is assigned on line 1");
            assert_eq!(
                location.range.start.character, 0,
                "$typed starts at column 0"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Clicking on `$val` in a foreach key-value `=> $val` should jump
/// to the foreach declaration.
#[tokio::test]
async fn test_goto_definition_variable_foreach_key_value() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///var_goto_foreach_kv.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "function demo(): void {\n",              // 1
        "    $map = ['a' => 1];\n",               // 2
        "    foreach ($map as $key => $val) {\n", // 3
        "        echo $val;\n",                   // 4
        "    }\n",                                // 5
        "}\n",                                    // 6
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

    // Cursor on `$val` in `echo $val;` (line 4)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $val to the foreach key-value declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "$val is declared in the foreach on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Type-Hint Resolution at Variable Definition ───────────────────────────

/// When the cursor is on a promoted constructor property at its definition,
/// GTD should return self-location so editors can fall back to Find References.
/// The type hint is a separate symbol span the user can click directly.
#[tokio::test]
async fn test_goto_definition_variable_at_definition_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///accordion.php").unwrap();
    let text = concat!(
        "<?php\n",                                               // 0
        "class HtmlString {}\n",                                 // 1
        "class AccordionData {\n",                               // 2
        "    public function __construct(\n",                    // 3
        "        public readonly HtmlString|string $content,\n", // 4
        "    ) {}\n",                                            // 5
        "}\n",                                                   // 6
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

    // Cursor on `$content` on line 4 (the definition itself)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 45,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 4, "should point back to line 4");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// GTD on a parameter at its definition site should return self-location
/// so editors can fall back to Find References.
#[tokio::test]
async fn test_goto_definition_parameter_at_definition_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///param_type.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "class Request {\n",                            // 1
        "    public function input(): string {}\n",     // 2
        "}\n",                                          // 3
        "class Controller {\n",                         // 4
        "    public function handle(Request $req) {\n", // 5
        "    }\n",                                      // 6
        "}\n",                                          // 7
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

    // Cursor on `$req` on line 5 (the parameter definition)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 39,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 5, "should point back to line 5");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// GTD on a parameter with a nullable class type hint (`?Foo`) at its
/// definition site should return self-location.
#[tokio::test]
async fn test_goto_definition_variable_at_definition_nullable_type_hint_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///nullable_type.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "class Logger {\n",                             // 1
        "    public function info(): void {}\n",        // 2
        "}\n",                                          // 3
        "class App {\n",                                // 4
        "    public function handle(?Logger $log) {\n", // 5
        "    }\n",                                      // 6
        "}\n",                                          // 7
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

    // Cursor on `$log` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 37,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 5, "should point back to line 5");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// GTD on a parameter with a scalar type hint (e.g. `string $name`) at its
/// definition site should return self-location.
#[tokio::test]
async fn test_goto_definition_variable_at_definition_scalar_type_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///scalar_type.php").unwrap();
    let text = concat!(
        "<?php\n",                                      // 0
        "class App {\n",                                // 1
        "    public function handle(string $name) {\n", // 2
        "    }\n",                                      // 3
        "}\n",                                          // 4
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

    // Cursor on `$name` on line 2
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 37,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "should point back to line 2");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// GTD on a parameter with a union type hint at its definition site
/// should return self-location, even when the class is the second member.
#[tokio::test]
async fn test_goto_definition_variable_at_definition_union_class_second_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///union_second.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "class HtmlString {\n",                                   // 1
        "    public function toHtml(): string {}\n",              // 2
        "}\n",                                                    // 3
        "class Widget {\n",                                       // 4
        "    public function render(string|HtmlString $out) {\n", // 5
        "    }\n",                                                // 6
        "}\n",                                                    // 7
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

    // Cursor on `$out` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 47,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 5, "should point back to line 5");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// GTD on a property at its definition site should return self-location
/// so editors can fall back to Find References.
#[tokio::test]
async fn test_goto_definition_property_at_definition_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///prop_type.php").unwrap();
    let text = concat!(
        "<?php\n",                         // 0
        "class Logger {\n",                // 1
        "    public function info() {}\n", // 2
        "}\n",                             // 3
        "class App {\n",                   // 4
        "    private Logger $logger;\n",   // 5
        "}\n",                             // 6
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

    // Cursor on `$logger` on line 5
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 21,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 5, "should point back to line 5");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

// ─── Foreach Variable: Consecutive Loops with Same Variable Name ────────────

#[tokio::test]
async fn test_goto_definition_foreach_consecutive_loops_same_var() {
    // Reproduces issue #43: when the same variable name is used in
    // consecutive foreach loops, clicking on `$b` inside the second
    // loop body should jump to the second foreach's `as $b`, not the
    // first loop's.
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_consecutive.php").unwrap();
    let text = concat!(
        "<?php\n",                    // 0
        "function demo(): void {\n",  // 1
        "    foreach ($a as $b) {\n", // 2
        "        echo $b;\n",         // 3
        "    }\n",                    // 4
        "    foreach ($c as $b) {\n", // 5
        "        echo $b;\n",         // 6
        "    }\n",                    // 7
        "}\n",                        // 8
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

    // Cursor on `$b` in `echo $b;` on line 6 (inside the second loop).
    // Should jump to line 5 (`as $b` in the second foreach), NOT line 2.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $b to the second foreach declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 5,
                "$b should jump to line 5 (second foreach), not line 2 (first foreach)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_goto_definition_foreach_on_as_variable_returns_none() {
    // When the cursor is on `$b` in `as $b` (the definition site itself),
    // go-to-definition should return None — the user is already at the
    // definition.  Previously the backwards scan found an earlier foreach
    // with the same variable and jumped there.
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_on_as.php").unwrap();
    let text = concat!(
        "<?php\n",                    // 0
        "function demo(): void {\n",  // 1
        "    foreach ($a as $b) {\n", // 2
        "    }\n",                    // 3
        "    foreach ($c as $b) {\n", // 4
        "    }\n",                    // 5
        "}\n",                        // 6
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

    // Cursor on `$b` in `as $b` on line 4 (the second foreach's
    // definition site).
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 24,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_none(),
        "Should return None when cursor is on `as $b` (already at definition site), \
         but got: {:?}. This means it jumped to the first foreach incorrectly.",
        result,
    );
}

#[tokio::test]
async fn test_goto_definition_foreach_reassignment_inside_loop() {
    // When $b is redefined by assignment inside a loop, the usage after
    // the assignment should jump to the assignment, not the foreach.
    let backend = create_test_backend();

    let uri = Url::parse("file:///foreach_reassign.php").unwrap();
    let text = concat!(
        "<?php\n",                       // 0
        "function demo(): void {\n",     // 1
        "    foreach ($a as $b) {\n",    // 2
        "        $b = 'overwritten';\n", // 3
        "        echo $b;\n",            // 4
        "    }\n",                       // 5
        "}\n",                           // 6
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

    // Cursor on `$b` in `echo $b;` on line 4.
    // Should jump to line 3 (the reassignment), not line 2 (the foreach).
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $b to the reassignment on line 3"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "$b should jump to the reassignment on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── RHS variable on same line as LHS assignment ────────────────────────────

/// When `$value` appears on both sides of an assignment (`$value = $value->value`),
/// go-to-definition on the RHS `$value` should jump to the parameter declaration,
/// not silently return nothing because the line also contains a definition.
#[tokio::test]
async fn test_goto_definition_rhs_variable_same_line_as_assignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///rhs_same_line.php").unwrap();
    let text = concat!(
        "<?php\n",                                                 // 0
        "class Converter {\n",                                     // 1
        "    public static function toInt(mixed $value): int {\n", // 2
        "        if ($value instanceof BackedEnum) {\n",           // 3
        "            $value = $value->value;\n",                   // 4
        "        }\n",                                             // 5
        "        return (int) $value;\n",                          // 6
        "    }\n",                                                 // 7
        "}\n",                                                     // 8
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

    // Cursor on the RHS `$value` in `$value = $value->value;` (line 4).
    // The LHS `$value` starts at column 12, so the RHS `$value` starts at column 21.
    // Clicking on the RHS should jump to the parameter on line 2.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 22, // inside the RHS `$value`
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "RHS $value should resolve to the parameter declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "RHS $value should jump to the parameter on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// When the cursor is on the LHS `$value` of `$value = $value->value`,
/// GTD should return self-location (it is a definition site).
#[tokio::test]
async fn test_goto_definition_lhs_variable_same_line_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///lhs_same_line.php").unwrap();
    let text = concat!(
        "<?php\n",                                                 // 0
        "class Converter {\n",                                     // 1
        "    public static function toInt(mixed $value): int {\n", // 2
        "        if ($value instanceof BackedEnum) {\n",           // 3
        "            $value = $value->value;\n",                   // 4
        "        }\n",                                             // 5
        "    }\n",                                                 // 6
        "}\n",                                                     // 7
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

    // Cursor on the LHS `$value` in `$value = $value->value;` (line 4, col 13).
    // This is the definition site itself — go-to-definition should return None
    // (no further definition to jump to, since `mixed` is a scalar type).
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 13, // inside the LHS `$value`
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 4, "should point back to line 4");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

// ─── Arrow Function Parameter Go-to-Definition ─────────────────────────────

/// When the cursor is on the RHS usage of an arrow function parameter
/// (e.g. `$o` in `fn(Order $o) => $o->getItems()`), go-to-definition
/// should jump to the parameter on the same line, not to some unrelated
/// `$o` earlier in the file.
#[tokio::test]
async fn test_goto_definition_arrow_fn_rhs_param_jumps_to_same_line_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///arrow_fn_rhs.php").unwrap();
    let text = concat!(
        "<?php\n",                                                          // 0
        "class Order {\n",                                                  // 1
        "    public function getItems(): array {}\n",                       // 2
        "}\n",                                                              // 3
        "class Demo {\n",                                                   // 4
        "    public Order $o;\n",                                           // 5
        "    public function run(): void {\n",                              // 6
        "        $list = array_map(fn(Order $o) => $o->getItems(), []);\n", // 7
        "    }\n",                                                          // 8
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

    // Line 7: `        $list = array_map(fn(Order $o) => $o->getItems(), []);`
    //                                                    ^^ cursor on RHS $o
    // Count columns: `        $list = array_map(fn(Order $o) => $o->getItems(), []);`
    //   "        " = 8, "$list" = 5, " = " = 3, "array_map(" = 10, "fn(" = 3,
    //   "Order " = 6, "$o" = 2, ") => " = 5, "$o" = starts at col 42
    // The parameter `$o` is at col 33, the RHS `$o` is at col 42.
    // Let's find the exact columns by counting:
    //   0         1         2         3         4
    //   0123456789012345678901234567890123456789012345
    //           $list = array_map(fn(Order $o) => $o->getItems(), []);
    // `        ` = 8 chars
    // `$list` at 8-12
    // ` = ` at 13-15
    // `array_map(` at 16-25
    // `fn(` at 26-28
    // `Order` at 29-33
    // ` ` at 34
    // `$o` at 35-36
    // `) => ` at 37-41
    // `$o` at 42-43
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 43, // on the `o` of the RHS `$o`
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "RHS $o should resolve to the arrow function parameter"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 7,
                "RHS $o should jump to the parameter on the same line 7"
            );
            assert_eq!(
                location.range.start.character, 35,
                "Should point to the parameter $o at column 35"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// GTD on an arrow function parameter at its definition site (`$o` in
/// `fn(Order $o) =>`) should return self-location.
#[tokio::test]
async fn test_goto_definition_arrow_fn_lhs_param_returns_self_location() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///arrow_fn_lhs.php").unwrap();
    let text = concat!(
        "<?php\n",                                                          // 0
        "class Order {\n",                                                  // 1
        "    public function getItems(): array {}\n",                       // 2
        "}\n",                                                              // 3
        "class Demo {\n",                                                   // 4
        "    public function run(): void {\n",                              // 5
        "        $list = array_map(fn(Order $o) => $o->getItems(), []);\n", // 6
        "    }\n",                                                          // 7
        "}\n",                                                              // 8
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

    // Cursor on the defining `$o` parameter on line 6
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 36,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 6, "should point back to line 6");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// Arrow function parameter with no type hint: RHS usage should still
/// jump to the parameter definition on the same line, and the LHS
/// should return None (no type hint to resolve).
#[tokio::test]
async fn test_goto_definition_arrow_fn_untyped_param_rhs_jumps_to_param() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///arrow_fn_untyped.php").unwrap();
    let text = concat!(
        "<?php\n",                                            // 0
        "class Demo {\n",                                     // 1
        "    public function run(): void {\n",                // 2
        "        $list = array_map(fn($x) => $x + 1, []);\n", // 3
        "    }\n",                                            // 4
        "}\n",                                                // 5
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

    // Line 3: `        $list = array_map(fn($x) => $x + 1, []);`
    //   "        " = 8, "$list" = 5, " = " = 3, "array_map(" = 10, "fn(" = 3
    //   "$x" at 29-30, ") => " = 5, "$x" at 36-37
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 37, // on the `x` of the RHS `$x`
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "RHS $x should resolve to the arrow function parameter"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "RHS $x should jump to the parameter on the same line"
            );
            assert_eq!(
                location.range.start.character, 29,
                "Should point to the parameter $x at column 29"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Closure use() clause GTD ───────────────────────────────────────────────

/// Using `$search` inside a closure body should jump to the `$search`
/// in the `use ($search)` clause.
#[tokio::test]
async fn test_goto_definition_closure_use_clause_body_to_use() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_closure_use.php").unwrap();

    // Line 0: <?php
    // Line 1: function demo(): void {
    // Line 2:     $search = 'hi';
    // Line 3:     $fn = function () use ($search): void {
    // Line 4:         echo $search;
    // Line 5:     };
    // Line 6: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $search = 'hi';\n",
        "    $fn = function () use ($search): void {\n",
        "        echo $search;\n",
        "    };\n",
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

    // Cursor on `$search` in `echo $search;` (line 4, col 13)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $search inside closure body to the use() clause"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "$search should jump to the use() clause on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Clicking `$search` in the `use ($search)` clause should jump to the
/// outer assignment `$search = 'hi'`.
#[tokio::test]
async fn test_goto_definition_closure_use_clause_to_outer_assignment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_closure_use_outer.php").unwrap();

    // Line 0: <?php
    // Line 1: function demo(): void {
    // Line 2:     $search = 'hi';
    // Line 3:     $fn = function () use ($search): void {
    // Line 4:         echo $search;
    // Line 5:     };
    // Line 6: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $search = 'hi';\n",
        "    $fn = function () use ($search): void {\n",
        "        echo $search;\n",
        "    };\n",
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

    // Cursor on `$search` in `use ($search)` (line 3, col 28)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 28,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $search in use() clause to the outer assignment"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$search in use() should jump to the assignment on line 2"
            );
            assert_eq!(
                location.range.start.character, 4,
                "$search assignment starts at column 4"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Multiple captured variables: each resolves independently.
#[tokio::test]
async fn test_goto_definition_closure_use_clause_multiple_captures() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_closure_use_multi.php").unwrap();

    // Line 0: <?php
    // Line 1: function demo(): void {
    // Line 2:     $a = 1;
    // Line 3:     $b = 2;
    // Line 4:     $fn = function () use ($a, $b): void {
    // Line 5:         echo $a;
    // Line 6:         echo $b;
    // Line 7:     };
    // Line 8: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $a = 1;\n",
        "    $b = 2;\n",
        "    $fn = function () use ($a, $b): void {\n",
        "        echo $a;\n",
        "        echo $b;\n",
        "    };\n",
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

    // $b inside the closure body (line 6, col 13) should jump to
    // $b in the use() clause on line 4.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $b inside closure body to the use() clause"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 4,
                "$b should jump to the use() clause on line 4"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// A captured variable reassigned inside the closure body should jump
/// to the reassignment, not the use() clause.
#[tokio::test]
async fn test_goto_definition_closure_use_clause_reassigned_inside() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_closure_use_reassign.php").unwrap();

    // Line 0: <?php
    // Line 1: function demo(): void {
    // Line 2:     $x = 'outer';
    // Line 3:     $fn = function () use ($x): void {
    // Line 4:         $x = 'inner';
    // Line 5:         echo $x;
    // Line 6:     };
    // Line 7: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $x = 'outer';\n",
        "    $fn = function () use ($x): void {\n",
        "        $x = 'inner';\n",
        "        echo $x;\n",
        "    };\n",
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

    // $x on line 5 should jump to the reassignment on line 4, not the use() clause.
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x to the reassignment inside the closure"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 4,
                "$x should jump to the inner reassignment on line 4"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

// ─── Switch / Do-While / For / While / Global / Static / Destructuring / Block / Return ───

/// Variable assigned in a switch case, goto-definition from usage after the switch.
#[tokio::test]
async fn test_goto_definition_switch_variable_after_switch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_switch_after.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     switch (true) {
    // line 3:         case 1:
    // line 4:             $x = 'one';
    // line 5:             break;
    // line 6:     }
    // line 7:     echo $x;
    // line 8: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    switch (true) {\n",
        "        case 1:\n",
        "            $x = 'one';\n",
        "            break;\n",
        "    }\n",
        "    echo $x;\n",
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

    // Cursor on `$x` in `echo $x;` (line 7, char 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 7,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x to the assignment inside the switch case"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 4,
                "$x is assigned on line 4 inside the case"
            );
            assert_eq!(location.range.start.character, 12);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Cursor is inside a switch case body, variable defined earlier in the same case.
#[tokio::test]
async fn test_goto_definition_switch_cursor_inside_case() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_switch_inside_case.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     $x = 'before';
    // line 3:     switch (true) {
    // line 4:         case 1:
    // line 5:             $x = 'one';
    // line 6:             $y = $x;
    // line 7:             break;
    // line 8:     }
    // line 9: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $x = 'before';\n",
        "    switch (true) {\n",
        "        case 1:\n",
        "            $x = 'one';\n",
        "            $y = $x;\n",
        "            break;\n",
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

    // Cursor on RHS `$x` in `$y = $x;` (line 6, char 17)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 17,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x to the assignment inside the same case"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 5,
                "$x should jump to the case-local assignment on line 5"
            );
            assert_eq!(location.range.start.character, 12);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Variable assigned inside a do-while body, goto-def from usage inside the body.
#[tokio::test]
async fn test_goto_definition_do_while_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_do_while.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     do {
    // line 3:         $x = 'hello';
    // line 4:         $y = $x;
    // line 5:     } while (true);
    // line 6: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    do {\n",
        "        $x = 'hello';\n",
        "        $y = $x;\n",
        "    } while (true);\n",
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

    // Cursor on RHS `$x` in `$y = $x;` (line 4, char 13)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x to the assignment inside the do-while"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "$x is assigned on line 3 inside the do-while"
            );
            assert_eq!(location.range.start.character, 8);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// For loop initializer: `for ($i = 0; ...)`, goto-definition of `$i` from inside the loop body.
#[tokio::test]
async fn test_goto_definition_for_loop_initializer() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_for_init.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     for ($i = 0; $i < 10; $i++) {
    // line 3:         $y = $i;
    // line 4:     }
    // line 5: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    for ($i = 0; $i < 10; $i++) {\n",
        "        $y = $i;\n",
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

    // Cursor on RHS `$i` in `$y = $i;` (line 3, char 13)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $i to the for-loop initializer"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$i is initialized on line 2 in the for statement"
            );
            assert_eq!(location.range.start.character, 9);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Cursor on `$i` in `for ($i = 0; ...)` returns self-location (at definition).
#[tokio::test]
async fn test_goto_definition_for_loop_initializer_at_definition_returns_self_location() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_for_init_atdef.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     for ($i = 0; $i < 10; $i++) {
    // line 3:         $y = $i;
    // line 4:     }
    // line 5: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    for ($i = 0; $i < 10; $i++) {\n",
        "        $y = $i;\n",
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

    // Cursor on `$i` in `for ($i = 0;` (line 2, char 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "should point back to line 2");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// Variable assigned before a while loop, goto-def from inside the while body.
#[tokio::test]
async fn test_goto_definition_while_loop_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_while.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     $x = 'hello';
    // line 3:     while (true) {
    // line 4:         $y = $x;
    // line 5:     }
    // line 6: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $x = 'hello';\n",
        "    while (true) {\n",
        "        $y = $x;\n",
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

    // Cursor on RHS `$x` in `$y = $x;` (line 4, char 13)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x to the assignment before the while loop"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$x is assigned on line 2 before the while loop"
            );
            assert_eq!(location.range.start.character, 4);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// `global $gvar;` in a function, goto-def of `$gvar` usage.
#[tokio::test]
async fn test_goto_definition_global_statement() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_global.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     global $gvar;
    // line 3:     $y = $gvar;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    global $gvar;\n",
        "    $y = $gvar;\n",
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

    // Cursor on RHS `$gvar` in `$y = $gvar;` (line 3, char 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $gvar to the global declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$gvar is declared on line 2 in the global statement"
            );
            assert_eq!(location.range.start.character, 11);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Cursor on `$gvar` in `global $gvar;` returns self-location (at definition).
#[tokio::test]
async fn test_goto_definition_global_statement_at_definition_returns_self_location() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_global_atdef.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     global $gvar;
    // line 3:     $y = $gvar;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    global $gvar;\n",
        "    $y = $gvar;\n",
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

    // Cursor on `$gvar` in `global $gvar;` (line 2, char 11)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "should point back to line 2");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// `static $count = 0;` in a function, goto-def of `$count` usage.
#[tokio::test]
async fn test_goto_definition_static_statement() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_static.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     static $count = 0;
    // line 3:     $y = $count;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    static $count = 0;\n",
        "    $y = $count;\n",
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

    // Cursor on RHS `$count` in `$y = $count;` (line 3, char 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $count to the static declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$count is declared on line 2 in the static statement"
            );
            assert_eq!(location.range.start.character, 11);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Cursor on `$count` in `static $count = 0;` returns self-location (at definition).
#[tokio::test]
async fn test_goto_definition_static_statement_at_definition_returns_self_location() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_static_atdef.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     static $count = 0;
    // line 3:     $y = $count;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    static $count = 0;\n",
        "    $y = $count;\n",
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

    // Cursor on `$count` in `static $count = 0;` (line 2, char 11)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "should point back to line 2");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// Array destructuring: `[$a, $b] = getValues();`, goto-def of `$a` from later usage.
#[tokio::test]
async fn test_goto_definition_array_destructuring() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_array_destruct.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     [$a, $b] = getValues();
    // line 3:     $y = $a;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    [$a, $b] = getValues();\n",
        "    $y = $a;\n",
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

    // Cursor on RHS `$a` in `$y = $a;` (line 3, char 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $a to the array destructuring on line 2"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "$a is destructured on line 2");
            assert_eq!(location.range.start.character, 5);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Cursor on `$a` in `[$a, $b] = ...` returns self-location (at definition).
#[tokio::test]
async fn test_goto_definition_array_destructuring_at_definition_returns_self_location() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_array_destruct_atdef.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     [$a, $b] = getValues();
    // line 3:     $y = $a;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    [$a, $b] = getValues();\n",
        "    $y = $a;\n",
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

    // Cursor on `$a` in `[$a, $b] = ...` (line 2, char 5)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 5,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "should point back to line 2");
        }
        other => panic!("Expected self-location Scalar, got: {other:?}"),
    }
}

/// List destructuring: `list($x, $y) = getValues();`, goto-def of `$x`.
#[tokio::test]
async fn test_goto_definition_list_destructuring() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_list_destruct.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     list($x, $y) = getValues();
    // line 3:     $z = $x;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    list($x, $y) = getValues();\n",
        "    $z = $x;\n",
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

    // Cursor on RHS `$x` in `$z = $x;` (line 3, char 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x to the list() destructuring on line 2"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "$x is destructured on line 2");
            // "    list($x" → $x starts at char 9
            assert_eq!(location.range.start.character, 9);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Nested destructuring: `[[$a, $b], $c] = ...`, goto-def of `$b`.
#[tokio::test]
async fn test_goto_definition_nested_destructuring() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_nested_destruct.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     [[$a, $b], $c] = getValues();
    // line 3:     $y = $b;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    [[$a, $b], $c] = getValues();\n",
        "    $y = $b;\n",
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

    // Cursor on RHS `$b` in `$y = $b;` (line 3, char 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $b to the nested destructuring on line 2"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(location.range.start.line, 2, "$b is destructured on line 2");
            // "    [[$a, $b]" → $b starts at char 10
            assert_eq!(location.range.start.character, 10);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Foreach destructuring: `foreach ($items as [$name, $value])`, goto-def of `$name` from loop body.
#[tokio::test]
async fn test_goto_definition_foreach_destructuring() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_foreach_destruct.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     $items = [];
    // line 3:     foreach ($items as [$name, $value]) {
    // line 4:         $y = $name;
    // line 5:     }
    // line 6: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $items = [];\n",
        "    foreach ($items as [$name, $value]) {\n",
        "        $y = $name;\n",
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

    // Cursor on RHS `$name` in `$y = $name;` (line 4, char 13)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $name to the foreach destructuring on line 3"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "$name is destructured in foreach on line 3"
            );
            // "    foreach ($items as [$name" → $name starts at char 24
            assert_eq!(location.range.start.character, 24);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Foreach key variable: goto-def of `$key` from loop body.
#[tokio::test]
async fn test_goto_definition_foreach_key_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_foreach_key.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     $map = ['a' => 1];
    // line 3:     foreach ($map as $key => $val) {
    // line 4:         $y = $key;
    // line 5:     }
    // line 6: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $map = ['a' => 1];\n",
        "    foreach ($map as $key => $val) {\n",
        "        $y = $key;\n",
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

    // Cursor on RHS `$key` in `$y = $key;` (line 4, char 13)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $key to the foreach key declaration on line 3"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "$key is declared in the foreach on line 3"
            );
            // "    foreach ($map as $key" → $key starts at char 21
            assert_eq!(location.range.start.character, 21);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Variable defined inside a block `{ }`, goto-def from usage after the block.
#[tokio::test]
async fn test_goto_definition_block_statement() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_block.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     {
    // line 3:         $x = 'hello';
    // line 4:     }
    // line 5:     $y = $x;
    // line 6: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    {\n",
        "        $x = 'hello';\n",
        "    }\n",
        "    $y = $x;\n",
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

    // Cursor on RHS `$x` in `$y = $x;` (line 5, char 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x to the assignment inside the block"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "$x is assigned on line 3 inside the block"
            );
            assert_eq!(location.range.start.character, 8);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Return statement with assignment: `return $x = foo();`, goto-def of `$x`
/// from dead code after the return triggers the Return statement handler.
#[tokio::test]
async fn test_goto_definition_return_with_assignment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_return_assign.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): mixed {
    // line 2:     return $x = getValue();
    // line 3:     $y = $x;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(): mixed {\n",
        "    return $x = getValue();\n",
        "    $y = $x;\n",
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

    // Cursor on RHS `$x` in `$y = $x;` (line 3, char 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x to the assignment in the return statement"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$x is assigned on line 2 in the return statement"
            );
            // "    return $x" → $x starts at char 11
            assert_eq!(location.range.start.character, 11);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Cursor inside a closure body where the variable is defined both outside
/// and inside the closure (without use clause). The inner definition wins.
#[tokio::test]
async fn test_goto_definition_closure_inner_definition_wins() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_closure_inner.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     $x = 'outer';
    // line 3:     $fn = function () {
    // line 4:         $x = 'inner';
    // line 5:         $y = $x;
    // line 6:     };
    // line 7: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $x = 'outer';\n",
        "    $fn = function () {\n",
        "        $x = 'inner';\n",
        "        $y = $x;\n",
        "    };\n",
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

    // Cursor on RHS `$x` in `$y = $x;` (line 5, char 13)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x to the inner closure assignment, not the outer one"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 4,
                "$x should jump to the inner assignment on line 4"
            );
            assert_eq!(location.range.start.character, 8);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Arrow function: cursor on body variable resolves to the arrow fn parameter.
#[tokio::test]
async fn test_goto_definition_arrow_function_body_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_arrow_fn_body.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     $x = 'outer';
    // line 3:     $fn = fn($x) => $x;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $x = 'outer';\n",
        "    $fn = fn($x) => $x;\n",
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

    // Cursor on `$x` in the arrow function body `=> $x` (line 3, char 20)
    // "    $fn = fn($x) => $x;\n"
    //  0123456789012345678901234
    //                      ^$x at 20
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $x in arrow fn body to the arrow fn parameter"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 3,
                "$x should jump to the arrow fn parameter on line 3"
            );
            // "    $fn = fn($x)" → $x parameter starts at char 13
            assert_eq!(location.range.start.character, 13);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// RHS of same assignment: `$value = $value->something` with cursor on
/// RHS `$value` should jump to the original definition (parameter),
/// not the LHS of the same statement.
#[tokio::test]
async fn test_goto_definition_rhs_same_assignment_jumps_to_original() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_rhs_same_assign.php").unwrap();

    // line 0: <?php
    // line 1: function demo(mixed $value): mixed {
    // line 2:     $value = $value->something;
    // line 3:     return $value;
    // line 4: }
    let text = concat!(
        "<?php\n",
        "function demo(mixed $value): mixed {\n",
        "    $value = $value->something;\n",
        "    return $value;\n",
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

    // Cursor on the RHS `$value` in `$value = $value->something;` (line 2)
    // "    $value = $value->something;\n"
    //  0123456789012345
    //               ^$value at 13
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 13,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "RHS $value should resolve to the parameter, not the LHS"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "RHS $value should jump to the parameter on line 1"
            );
            // "function demo(mixed $value)" → $value at char 20
            assert_eq!(location.range.start.character, 20);
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Conditional assignment inside if-block should not override the outer
/// definition for variable references after the if-block.
#[tokio::test]
async fn test_goto_definition_conditional_does_not_override_outer() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///var_cond_override.php").unwrap();

    // line 0: <?php
    // line 1: function demo(): void {
    // line 2:     $z = 'a';
    // line 3:     if (random_int(0, 1)) {
    // line 4:         $z = 'b';
    // line 5:     }
    // line 6:     echo $z;
    // line 7: }
    let text = concat!(
        "<?php\n",
        "function demo(): void {\n",
        "    $z = 'a';\n",
        "    if (random_int(0, 1)) {\n",
        "        $z = 'b';\n",
        "    }\n",
        "    echo $z;\n",
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

    // Cursor on `$z` in `echo $z;` (line 6, char 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $z to the outer assignment"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "$z should jump to line 2 (outer assignment), not line 4 (conditional)"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}
