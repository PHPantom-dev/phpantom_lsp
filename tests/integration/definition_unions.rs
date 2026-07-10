use crate::common::{create_psr4_workspace, create_test_backend};
use phpantom_lsp::atom::atom;
use phpantom_lsp::php_type::PhpType;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Union Return Type Resolution ───────────────────────────────────────────

/// When a function returns a union type (`B|C`), goto definition should
/// resolve the member if any part of the union declares it.
///
/// ```php
/// function a(): B|C { ... }
/// $a = a();
/// $a->onlyOnB(); // B has it, C doesn't — should still resolve
/// ```
#[tokio::test]
async fn test_goto_definition_union_return_type_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "class B {\n",                                            // 1
        "    public function onlyOnB(): void {}\n",               // 2
        "}\n",                                                    // 3
        "\n",                                                     // 4
        "class C {\n",                                            // 5
        "    public function onlyOnC(): void {}\n",               // 6
        "}\n",                                                    // 7
        "\n",                                                     // 8
        "class App {\n",                                          // 9
        "    public function getBC(): B|C { return new B(); }\n", // 10
        "\n",                                                     // 11
        "    public function run(): void {\n",                    // 12
        "        $a = $this->getBC();\n",                         // 13
        "        $a->onlyOnB();\n",                               // 14
        "        $a->onlyOnC();\n",                               // 15
        "    }\n",                                                // 16
        "}\n",                                                    // 17
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

    // Click on "onlyOnB" on line 14 — B has it, C doesn't
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 14,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $a->onlyOnB() via union return type B|C"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "onlyOnB is declared on line 2 in class B"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // Click on "onlyOnC" on line 15 — C has it, B doesn't
    let params2 = GotoDefinitionParams {
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

    let result2 = backend.goto_definition(params2).await.unwrap();
    assert!(
        result2.is_some(),
        "Should resolve $a->onlyOnC() via union return type B|C"
    );

    match result2.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 6,
                "onlyOnC is declared on line 6 in class C"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Union return type via a standalone function assigned to a variable:
/// `$x = someFunc();` where `someFunc(): A|B`.
#[tokio::test]
async fn test_goto_definition_union_return_type_standalone_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                               // 0
        "class Dog {\n",                         // 1
        "    public function bark(): void {}\n", // 2
        "}\n",                                   // 3
        "\n",                                    // 4
        "class Cat {\n",                         // 5
        "    public function meow(): void {}\n", // 6
        "}\n",                                   // 7
        "\n",                                    // 8
        "function getAnimal(): Dog|Cat {\n",     // 9
        "    return new Dog();\n",               // 10
        "}\n",                                   // 11
        "\n",                                    // 12
        "class App {\n",                         // 13
        "    public function run(): void {\n",   // 14
        "        $pet = getAnimal();\n",         // 15
        "        $pet->bark();\n",               // 16
        "        $pet->meow();\n",               // 17
        "    }\n",                               // 18
        "}\n",                                   // 19
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

    // Register the standalone function in global_functions so the resolver
    // can look up its return type.
    {
        let mut fmap = backend.global_functions().write();
        fmap.insert(
            "getAnimal".to_string(),
            (
                uri.to_string(),
                phpantom_lsp::FunctionInfo {
                    name: atom("getAnimal"),
                    name_offset: 0,
                    parameters: vec![],
                    return_type: Some(PhpType::parse("Dog|Cat")),
                    native_return_type: None,
                    description: None,
                    return_description: None,
                    links: vec![],
                    see_refs: vec![],
                    namespace: None,
                    conditional_return: None,
                    type_assertions: vec![],
                    deprecation_message: None,
                    deprecated_replacement: None,
                    template_params: vec![],
                    template_bindings: vec![],
                    template_param_bounds: Default::default(),
                    throws: vec![],
                    is_polyfill: false,
                    overloads: vec![],
                },
            ),
        );
    }

    // Click on "bark" on line 16 — Dog has it
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 16,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $pet->bark() via Dog|Cat union return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "bark is declared on line 2 in Dog"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // Click on "meow" on line 17 — Cat has it
    let params2 = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 17,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result2 = backend.goto_definition(params2).await.unwrap();
    assert!(
        result2.is_some(),
        "Should resolve $pet->meow() via Dog|Cat union return type"
    );

    match result2.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 6,
                "meow is declared on line 6 in Cat"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Nullable union type (`?Foo` is equivalent to `Foo|null`): should still
/// resolve the class part.
#[tokio::test]
async fn test_goto_definition_nullable_union_return_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                        // 0
        "class Formatter {\n",                                            // 1
        "    public function format(string $s): string { return $s; }\n", // 2
        "}\n",                                                            // 3
        "\n",                                                             // 4
        "class App {\n",                                                  // 5
        "    public function getFormatter(): ?Formatter {\n",             // 6
        "        return new Formatter();\n",                              // 7
        "    }\n",                                                        // 8
        "\n",                                                             // 9
        "    public function run(): void {\n",                            // 10
        "        $f = $this->getFormatter();\n",                          // 11
        "        $f->format('hello');\n",                                 // 12
        "    }\n",                                                        // 13
        "}\n",                                                            // 14
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

    // Click on "format" on line 12
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 16,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $f->format() via ?Formatter nullable return type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "format is declared on line 2 in Formatter"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Union type on a property: `public A|B $prop;`
#[tokio::test]
async fn test_goto_definition_union_property_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                // 0
        "class Engine {\n",                       // 1
        "    public function start(): void {}\n", // 2
        "}\n",                                    // 3
        "\n",                                     // 4
        "class Motor {\n",                        // 5
        "    public function rev(): void {}\n",   // 6
        "}\n",                                    // 7
        "\n",                                     // 8
        "class Car {\n",                          // 9
        "    public Engine|Motor $powerUnit;\n",  // 10
        "\n",                                     // 11
        "    public function run(): void {\n",    // 12
        "        $this->powerUnit->start();\n",   // 13
        "        $this->powerUnit->rev();\n",     // 14
        "    }\n",                                // 15
        "}\n",                                    // 16
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

    // Click on "start" on line 13 — Engine has it
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 13,
                character: 30,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $this->powerUnit->start() via Engine|Motor union property type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "start is declared on line 2 in Engine"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // Click on "rev" on line 14 — Motor has it
    let params2 = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 14,
                character: 27,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result2 = backend.goto_definition(params2).await.unwrap();
    assert!(
        result2.is_some(),
        "Should resolve $this->powerUnit->rev() via Engine|Motor union property type"
    );

    match result2.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 6,
                "rev is declared on line 6 in Motor"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Union type in a parameter type hint: `function run(A|B $x) { $x->method(); }`
#[tokio::test]
async fn test_goto_definition_union_parameter_type_hint() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                     // 0
        "class Reader {\n",                                            // 1
        "    public function read(): void {}\n",                       // 2
        "}\n",                                                         // 3
        "\n",                                                          // 4
        "class Stream {\n",                                            // 5
        "    public function consume(): void {}\n",                    // 6
        "}\n",                                                         // 7
        "\n",                                                          // 8
        "class App {\n",                                               // 9
        "    public function process(Reader|Stream $input): void {\n", // 10
        "        $input->read();\n",                                   // 11
        "        $input->consume();\n",                                // 12
        "    }\n",                                                     // 13
        "}\n",                                                         // 14
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

    // Click on "read" on line 11 — Reader has it
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $input->read() via Reader|Stream union param type"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "read is declared on line 2 in Reader"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // Click on "consume" on line 12 — Stream has it
    let params2 = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 18,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result2 = backend.goto_definition(params2).await.unwrap();
    assert!(
        result2.is_some(),
        "Should resolve $input->consume() via Reader|Stream union param type"
    );

    match result2.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 6,
                "consume is declared on line 6 in Stream"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Union return type with scalar parts: `string|Foo` — the scalar `string`
/// should be ignored and `Foo` should resolve.
#[tokio::test]
async fn test_goto_definition_union_with_scalar_parts() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",                                                // 0
        "class Result {\n",                                       // 1
        "    public function unwrap(): mixed { return null; }\n", // 2
        "}\n",                                                    // 3
        "\n",                                                     // 4
        "class App {\n",                                          // 5
        "    public function fetch(): string|Result {\n",         // 6
        "        return new Result();\n",                         // 7
        "    }\n",                                                // 8
        "\n",                                                     // 9
        "    public function run(): void {\n",                    // 10
        "        $r = $this->fetch();\n",                         // 11
        "        $r->unwrap();\n",                                // 12
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

    // Click on "unwrap" on line 12 — `string` part is ignored, Result has it
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $r->unwrap() via string|Result, ignoring scalar part"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 2,
                "unwrap is declared on line 2 in Result"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Cross-file union return type: parts of the union come from PSR-4.
#[tokio::test]
async fn test_goto_definition_union_return_type_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[
            (
                "src/Encoder.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Encoder {\n",
                    "    public function encode(string $data): string { return $data; }\n",
                    "}\n",
                ),
            ),
            (
                "src/Decoder.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Decoder {\n",
                    "    public function decode(string $data): string { return $data; }\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::parse("file:///test_main.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Encoder;\n",
        "use App\\Decoder;\n",
        "\n",
        "class Codec {\n",
        "    public function getCodec(): Encoder|Decoder {\n",
        "        return new Encoder();\n",
        "    }\n",
        "\n",
        "    public function run(): void {\n",
        "        $c = $this->getCodec();\n",
        "        $c->encode('data');\n",
        "        $c->decode('data');\n",
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

    // Click on "encode" on line 11 — Encoder has it (cross-file)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 11,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend.goto_definition(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should resolve $c->encode() via Encoder|Decoder union return type (cross-file)"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                location.range.start.line, 3,
                "encode is on line 3 of Encoder.php"
            );
            let loc_path = location.uri.to_file_path().unwrap();
            assert!(
                loc_path.ends_with("src/Encoder.php"),
                "Should resolve to Encoder.php, got: {:?}",
                loc_path
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // Click on "decode" on line 12 — Decoder has it (cross-file)
    let params2 = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 12,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result2 = backend.goto_definition(params2).await.unwrap();
    assert!(
        result2.is_some(),
        "Should resolve $c->decode() via Encoder|Decoder union return type (cross-file)"
    );

    match result2.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(
                location.range.start.line, 3,
                "decode is on line 3 of Decoder.php"
            );
            let loc_path = location.uri.to_file_path().unwrap();
            assert!(
                loc_path.ends_with("src/Decoder.php"),
                "Should resolve to Decoder.php, got: {:?}",
                loc_path
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}
