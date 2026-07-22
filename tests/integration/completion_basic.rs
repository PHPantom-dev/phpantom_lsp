use crate::common::create_test_backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Interface Completion Tests ─────────────────────────────────────────────

#[tokio::test]
async fn test_completion_interface_type_hint_resolves_methods() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///iface.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Loggable {\n",
        "    public function log(string $message): void;\n",
        "    public function getLogLevel(): int;\n",
        "}\n",
        "class Service {\n",
        "    public function run(Loggable $logger): void {\n",
        "        $logger->\n",
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

    // Cursor right after `$logger->` on line 7
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
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
        "Completion should return results for interface-typed parameter"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                names.iter().any(|n| n.starts_with("log(")),
                "Should contain interface method 'log', got: {:?}",
                names
            );
            assert!(
                names.iter().any(|n| n.starts_with("getLogLevel(")),
                "Should contain interface method 'getLogLevel', got: {:?}",
                names
            );
        }
        _ => panic!("Expected Array response"),
    }
}

#[tokio::test]
async fn test_completion_interface_constant_via_double_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///iface_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface HasStatus {\n",
        "    const STATUS_ACTIVE = 1;\n",
        "    const STATUS_INACTIVE = 0;\n",
        "    public function getStatus(): int;\n",
        "}\n",
        "class Foo {\n",
        "    public function bar(): void {\n",
        "        HasStatus::\n",
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

    // Cursor right after `HasStatus::` on line 8
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
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
        "Completion should return results for interface constant access"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                names.contains(&"STATUS_ACTIVE"),
                "Should contain constant 'STATUS_ACTIVE', got: {:?}",
                names
            );
            assert!(
                names.contains(&"STATUS_INACTIVE"),
                "Should contain constant 'STATUS_INACTIVE', got: {:?}",
                names
            );
        }
        _ => panic!("Expected Array response"),
    }
}

// ─── Basic Completion Tests ─────────────────────────────────────────────────

#[tokio::test]
async fn test_completion_returns_none_when_nothing_matches() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\n$x = 1;\n".to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 1,
                character: 0,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_none(),
        "Completion should return None when nothing matches"
    );
}

#[tokio::test]
async fn test_completion_suggests_php_keywords() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords.php").unwrap();
    let text = concat!("<?php\n", "function demo(): void {\n", "    ret\n", "}\n",).to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `ret` on line 2.
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 2,
                character: 7,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_some(),
        "Completion should return keyword suggestions for a keyword prefix"
    );

    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert!(
        items
            .iter()
            .any(|i| i.label == "return" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Expected `return` keyword completion, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_does_not_suggest_return_at_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords_top_level.php").unwrap();
    let text = concat!("<?php\n", "ret\n").to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 1,
                character: 3,
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
        None => Vec::new(),
    };
    assert!(
        !items
            .iter()
            .any(|i| i.label == "return" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Top-level completion should not suggest `return`, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_suggests_break_inside_loop_only() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords_break.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function loopDemo(bool $cond): void {\n",
        "    while ($cond) {\n",
        "        br\n",
        "    }\n",
        "}\n",
        "function nonLoopDemo(): void {\n",
        "    br\n",
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

    let loop_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 10,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let loop_result = backend.completion(loop_params).await.unwrap();
    let loop_items = match loop_result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert!(
        loop_items
            .iter()
            .any(|i| i.label == "break" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Inside loop completion should suggest `break`, got: {:?}",
        loop_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );

    let non_loop_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let non_loop_result = backend.completion(non_loop_params).await.unwrap();
    let non_loop_items = match non_loop_result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        !non_loop_items
            .iter()
            .any(|i| i.label == "break" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Outside loop completion should not suggest `break`, got: {:?}",
        non_loop_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_suggests_continue_in_loop_not_switch() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords_continue.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function loopDemo(): void {\n",
        "    foreach ([1, 2] as $v) {\n",
        "        con\n",
        "    }\n",
        "}\n",
        "function switchDemo(): void {\n",
        "    switch (1) {\n",
        "        case 1:\n",
        "            con\n",
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

    // `continue` inside a foreach loop — should be suggested.
    let loop_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let loop_result = backend.completion(loop_params).await.unwrap();
    let loop_items = match loop_result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        loop_items
            .iter()
            .any(|i| i.label == "continue" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "`continue` should be suggested inside a loop, got: {:?}",
        loop_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );

    // `continue` inside a switch (but not a loop) — should NOT be suggested.
    let switch_params = CompletionParams {
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
    let switch_result = backend.completion(switch_params).await.unwrap();
    let switch_items = match switch_result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        !switch_items
            .iter()
            .any(|i| i.label == "continue" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "`continue` should NOT be suggested inside a switch (without a loop), got: {:?}",
        switch_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_suggests_case_default_inside_switch() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords_switch.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function switchDemo(int $x): void {\n",
        "    switch ($x) {\n",
        "        case 1:\n",
        "            break;\n",
        "        cas\n",
        "    }\n",
        "}\n",
        "function nonSwitchDemo(): void {\n",
        "    cas\n",
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

    // `case` inside a switch — should be suggested.
    let switch_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let switch_result = backend.completion(switch_params).await.unwrap();
    let switch_items = match switch_result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        switch_items
            .iter()
            .any(|i| i.label == "case" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "`case` should be suggested inside a switch, got: {:?}",
        switch_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );

    // `case` outside a switch — should NOT be suggested.
    let non_switch_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 7,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let non_switch_result = backend.completion(non_switch_params).await.unwrap();
    let non_switch_items = match non_switch_result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        !non_switch_items
            .iter()
            .any(|i| i.label == "case" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "`case` should NOT be suggested outside a switch, got: {:?}",
        non_switch_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_interface_body_keyword_restrictions() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords_interface_body.php").unwrap();
    let text = concat!("<?php\n", "interface Loggable {\n", "    pu\n", "}\n",).to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 2,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let result = backend.completion(params).await.unwrap();
    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };

    let keyword_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        keyword_labels.contains(&"public"),
        "`public` should be suggested in interface body, got: {:?}",
        keyword_labels
    );
    // Interfaces only allow `public`, `function`, and `const`.
    for excluded in &["private", "protected", "static", "abstract", "readonly"] {
        assert!(
            !keyword_labels.contains(excluded),
            "`{excluded}` should NOT be suggested in interface body, got: {:?}",
            keyword_labels
        );
    }
}

#[tokio::test]
async fn test_completion_suggests_namespace_only_at_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords_namespace.php").unwrap();
    let text = concat!(
        "<?php\n",
        "nam\n",
        "function demo(): void {\n",
        "    nam\n",
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

    let top_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 3,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let top_result = backend.completion(top_params).await.unwrap();
    let top_items = match top_result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert!(
        top_items
            .iter()
            .any(|i| i.label == "namespace" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Top-level completion should suggest `namespace`, got: {:?}",
        top_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );

    let fn_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 3,
                character: 7,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let fn_result = backend.completion(fn_params).await.unwrap();
    let fn_items = match fn_result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        !fn_items
            .iter()
            .any(|i| i.label == "namespace" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Function-scope completion should not suggest `namespace`, got: {:?}",
        fn_items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_suggests_extends_implements_only_in_declaration_header() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords_decl_header.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Child ex\n",
        "class Another extends Base im\n",
        "interface Contract im\n",
        "function demo(): void {\n",
        "    ex\n",
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

    // `class Child ex|`
    let extends_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let extends_items = match backend.completion(extends_params).await.unwrap().unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert!(
        extends_items
            .iter()
            .any(|i| i.label == "extends" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Class declaration header should suggest `extends`, got: {:?}",
        extends_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );

    // `class Another extends Base im|`
    let impl_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 29,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let impl_items = match backend.completion(impl_params).await.unwrap().unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert!(
        impl_items
            .iter()
            .any(|i| i.label == "implements" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Class declaration header should suggest `implements`, got: {:?}",
        impl_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );

    // `interface Contract im|` should NOT suggest implements.
    let iface_impl_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 21,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let iface_result = backend.completion(iface_impl_params).await.unwrap();
    let iface_items = match iface_result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        !iface_items
            .iter()
            .any(|i| i.label == "implements" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Interface declaration header should not suggest `implements`, got: {:?}",
        iface_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );

    // `function demo() { ex| }` should NOT suggest extends.
    let fn_extends_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 5,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let fn_result = backend.completion(fn_extends_params).await.unwrap();
    let fn_items = match fn_result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        !fn_items
            .iter()
            .any(|i| i.label == "extends" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Function scope should not suggest `extends`, got: {:?}",
        fn_items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_class_body_keywords_are_contextual() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords_class_body.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    pu\n",
        "    if\n",
        "    ca\n",
        "}\n",
        "enum Status {\n",
        "    ca\n",
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

    // `class User { pu| }` => should suggest `public`.
    let vis_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let vis_items = match backend.completion(vis_params).await.unwrap().unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert!(
        vis_items
            .iter()
            .any(|i| i.label == "public" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Class body should suggest visibility keyword `public`, got: {:?}",
        vis_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );

    // `class User { if| }` => should NOT suggest `if`.
    let if_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let if_items = match backend.completion(if_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        !if_items
            .iter()
            .any(|i| i.label == "if" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Class body should not suggest statement keyword `if`, got: {:?}",
        if_items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );

    // `class User { ca| }` => should NOT suggest enum `case`.
    let class_case_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let class_case_items = match backend.completion(class_case_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        !class_case_items
            .iter()
            .any(|i| i.label == "case" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Class body should not suggest enum keyword `case`, got: {:?}",
        class_case_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );

    // `enum Status { ca| }` => should suggest `case`.
    let enum_case_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 6,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let enum_case_items = match backend.completion(enum_case_params).await.unwrap().unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert!(
        enum_case_items
            .iter()
            .any(|i| i.label == "case" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Enum body should suggest `case`, got: {:?}",
        enum_case_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_after_visibility_suggests_member_keywords() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords_after_visibility.php").unwrap();
    let text = concat!("<?php\n", "class User {\n", "    public \n", "}\n",).to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `public ` on line 2.
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 2,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    assert!(
        items
            .iter()
            .any(|i| i.label == "function" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "After visibility, completion should suggest `function`, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
    assert!(
        items
            .iter()
            .any(|i| i.label == "const" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "After visibility, completion should suggest `const`, got: {:?}",
        items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_after_function_keyword_does_not_suggest_classes() {
    // Issue #249 / #126: typing a method name must not offer class names.
    let backend = create_test_backend();

    let uri = Url::parse("file:///method_name_no_classes.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Cache {}\n",
        "class Carbon {}\n",
        "class Collection {}\n",
        "class Scheduler {\n",
        "    protected function getC\n",
        "}\n",
    )
    .to_string();

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.clone(),
            },
        })
        .await;

    // Cursor after `getC` on the method name line.
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 6,
                    character: 27,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };

    let class_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !class_labels
            .iter()
            .any(|l| *l == "Cache" || *l == "Carbon" || *l == "Collection"),
        "method name position must not suggest classes, got: {:?}",
        items
            .iter()
            .map(|i| format!("{:?}:{}", i.kind, i.label))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_suggests_backed_enum_types_after_enum_colon() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///keywords_enum_backed_type.php").unwrap();
    let text = concat!("<?php\n", "enum Role: \n", "enum Status: st\n",).to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    // Cursor right after `enum Role: ` on line 1.
    let empty_prefix_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 11,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let empty_prefix_items = match backend
        .completion(empty_prefix_params)
        .await
        .unwrap()
        .unwrap()
    {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert!(
        empty_prefix_items
            .iter()
            .any(|i| i.label == "string" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Backed enum type completion should suggest `string`, got: {:?}",
        empty_prefix_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        empty_prefix_items
            .iter()
            .any(|i| i.label == "int" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Backed enum type completion should suggest `int`, got: {:?}",
        empty_prefix_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );

    // Cursor right after `enum Status: st` on line 2.
    let st_prefix_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 2,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let st_prefix_items = match backend.completion(st_prefix_params).await.unwrap().unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    assert!(
        st_prefix_items
            .iter()
            .any(|i| i.label == "string" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Backed enum type prefix `st` should suggest `string`, got: {:?}",
        st_prefix_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        !st_prefix_items
            .iter()
            .any(|i| i.label == "int" && i.kind == Some(CompletionItemKind::KEYWORD)),
        "Backed enum type prefix `st` should not suggest `int`, got: {:?}",
        st_prefix_items
            .iter()
            .map(|i| i.label.clone())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_completion_inside_class_returns_methods() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///user.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    function login() {}\n",
        "    function logout() {}\n",
        "    function test() {\n",
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

    // Cursor right after `$this->` on line 5
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
            // Should have 3 non-static methods (login, logout, test)
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

            // Check labels show full signature
            for item in &method_items {
                let label = &item.label;
                assert!(
                    label.contains("(") && label.contains(")"),
                    "Label '{}' should contain full signature with parens",
                    label
                );
            }

            // Check insert_text is a snippet with parens (no required params here)
            for item in &method_items {
                let insert = item.insert_text.as_deref().unwrap();
                let filter = item.filter_text.as_deref().unwrap();
                assert!(
                    insert.starts_with(filter) && insert.contains("()"),
                    "insert_text '{}' should be a snippet starting with '{}' and containing parens",
                    insert,
                    filter
                );
                assert_eq!(
                    item.insert_text_format,
                    Some(InsertTextFormat::SNIPPET),
                    "insert_text_format should be Snippet"
                );
            }
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_outside_class_returns_fallback() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///user.php").unwrap();
    let text = "<?php\n\nclass User {\n    function login() {}\n}\n\n$x = 1;\n".to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    // Cursor outside the class (line 6: $x = 1;)
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 6,
                character: 0,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(
        result.is_none(),
        "Cursor outside class with no operator should return None"
    );
}

#[tokio::test]
async fn test_completion_with_multiple_classes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    function doFoo() {}\n",
        "    function doBar() {}\n",
        "}\n",
        "class Bar {\n",
        "    function handleRequest() {}\n",
        "    function test() {\n",
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

    // Verify two classes were parsed
    let classes = backend
        .get_classes_for_uri(uri.as_ref())
        .expect("ast_map should have entry");
    assert_eq!(classes.len(), 2);

    // Cursor right after `$this->` on line 8 inside Bar
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
            // Bar has handleRequest and test — both non-static
            assert_eq!(method_items.len(), 2, "Bar has two methods");
            let names: Vec<&str> = method_items
                .iter()
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();
            assert!(names.contains(&"handleRequest"));
            assert!(names.contains(&"test"));
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_completion_empty_class_falls_back() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///empty.php").unwrap();
    let text = "<?php\nclass Empty {\n}\n".to_string();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text,
        },
    };
    backend.did_open(open_params).await;

    // Cursor inside the empty class body
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 1,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // Empty class has no methods or properties, so should return None
    assert!(
        result.is_none(),
        "Empty class with no members should return None"
    );
}

#[tokio::test]
async fn test_completion_no_access_operator_shows_fallback() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///all.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Svc {\n",
        "    public static function create(): self {}\n",
        "    public function run(): void {}\n",
        "    public static string $instance = '';\n",
        "    public int $count = 0;\n",
        "    const MAX = 10;\n",
        "    \n",
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

    // Cursor on blank line 7 inside the class body (no `->` or `::`)
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 7,
                character: 4,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    // Without `->` or `::`, no class members should be suggested
    assert!(result.is_none(), "No access operator should return None");
}
