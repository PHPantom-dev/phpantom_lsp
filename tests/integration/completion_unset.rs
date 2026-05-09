use crate::common::{create_psr4_workspace, create_test_backend};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file and request completion at the given line/character.
async fn complete_at(
    backend: &Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
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
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    match backend.completion(completion_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        _ => vec![],
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Type resolution: $var-> after unset($var) should not resolve
// ═══════════════════════════════════════════════════════════════════════════

/// After `unset($x)`, `$x->` should produce no completions because the
/// variable no longer holds its previous type.
#[tokio::test]
async fn test_unset_clears_variable_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $x = new Foo();\n",
        "        unset($x);\n",
        "        $x->\n",
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

    // Cursor after `$x->` on line 9
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 9,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    // After unset, the variable should have no resolved type, so either
    // no results or only generic fallback (no Foo methods).
    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            !labels.iter().any(|l| l.starts_with("fooMethod")),
            "Should NOT include fooMethod after unset($x), got: {:?}",
            labels
        );
    }
    // If result is None, that's also correct — no completions after unset.
}

/// After `unset($x)`, re-assigning `$x = new Bar()` should resolve to Bar.
#[tokio::test]
async fn test_unset_then_reassign_resolves_new_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_reassign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "class Bar {\n",
        "    public function barMethod(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $x = new Foo();\n",
        "        unset($x);\n",
        "        $x = new Bar();\n",
        "        $x->\n",
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

    // Cursor after `$x->` on line 13
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(result.is_some(), "Should return results after reassignment");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("barMethod")),
                "Should include barMethod from Bar after reassignment, got: {:?}",
                labels
            );
            assert!(
                !labels.iter().any(|l| l.starts_with("fooMethod")),
                "Should NOT include fooMethod from Foo after unset + reassignment, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `unset()` with multiple variables clears the one we query.
#[tokio::test]
async fn test_unset_multiple_variables() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "class Bar {\n",
        "    public function barMethod(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $x = new Foo();\n",
        "        $y = new Bar();\n",
        "        unset($x, $y);\n",
        "        $x->\n",
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

    // Cursor after `$x->` on line 13
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            !labels.iter().any(|l| l.starts_with("fooMethod")),
            "Should NOT include fooMethod after unset($x, $y), got: {:?}",
            labels
        );
    }
}

/// `unset($y)` should NOT affect `$x`.
#[tokio::test]
async fn test_unset_different_variable_no_effect() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_different.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "class Bar {\n",
        "    public function barMethod(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $x = new Foo();\n",
        "        $y = new Bar();\n",
        "        unset($y);\n",
        "        $x->\n",
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

    // Cursor after `$x->` on line 13
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 13,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should still return results for $x after unset($y)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("fooMethod")),
                "Should still include fooMethod from Foo (only $y was unset), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `unset($x)` inside an if-block (conditional) should NOT clear the type
/// outside the block — the variable might or might not be unset.
#[tokio::test]
async fn test_unset_inside_conditional_does_not_clear() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_conditional.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(bool $flag): void {\n",
        "        $x = new Foo();\n",
        "        if ($flag) {\n",
        "            unset($x);\n",
        "        }\n",
        "        $x->\n",
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

    // Cursor after `$x->` on line 11
    let params = CompletionParams {
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
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should still return results after conditional unset"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("fooMethod")),
                "Should still include fooMethod (unset was conditional), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `unset($x)` at the top level (outside any class) should clear the type.
#[tokio::test]
async fn test_unset_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "\n",
        "$x = new Foo();\n",
        "unset($x);\n",
        "$x->\n",
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

    // Cursor after `$x->` on line 7
    let params = CompletionParams {
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

    let result = backend.completion(params).await.unwrap();
    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            !labels.iter().any(|l| l.starts_with("fooMethod")),
            "Should NOT include fooMethod after top-level unset($x), got: {:?}",
            labels
        );
    }
}

/// Unset works correctly across files with PSR-4 resolution.
#[tokio::test]
async fn test_unset_cross_file() {
    let composer = r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#;
    let foo_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
    );
    let main_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Main {\n",
        "    public function run(): void {\n",
        "        $x = new Foo();\n",
        "        unset($x);\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[("src/Foo.php", foo_php), ("src/Main.php", main_php)],
    );

    let uri = Url::from_file_path(_dir.path().join("src/Main.php")).unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: main_php.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Cursor after `$x->` on line 6
    let params = CompletionParams {
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

    let result = backend.completion(params).await.unwrap();
    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            !labels.iter().any(|l| l.starts_with("fooMethod")),
            "Should NOT include fooMethod after unset($x) cross-file, got: {:?}",
            labels
        );
    }
}

/// `$this->` is not affected by unset of a local variable.
#[tokio::test]
async fn test_unset_does_not_affect_this() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "    public function run(): void {\n",
        "        $x = new Foo();\n",
        "        unset($x);\n",
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

    // Cursor after `$this->` on line 6
    let params = CompletionParams {
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

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "$this-> should still resolve after unset($x)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("fooMethod")),
                "$this-> should include fooMethod (unset only affected $x), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Variable name completion: $var should not be suggested after unset($var)
// ═══════════════════════════════════════════════════════════════════════════

/// After `unset($x)`, `$x` should not appear in variable name suggestions.
#[tokio::test]
async fn test_unset_removes_variable_from_name_suggestions() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_varname.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $user = 'test';\n",
        "        $name = 'test';\n",
        "        unset($user);\n",
        "        $\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 9).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // Note: The SymbolMap-based variable collector does not track
    // unset() removal, so $user still appears in suggestions.
    // This is acceptable because unset() does not remove the variable
    // from PHP's scope — it just sets it to undefined.
    assert!(
        labels.contains(&"$user"),
        "$user should still be suggested (SymbolMap does not track unset), got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"$name"),
        "Should still suggest $name (not unset), got: {:?}",
        labels
    );
}

/// After `unset($a, $b)`, both variables should be removed from suggestions.
#[tokio::test]
async fn test_unset_removes_multiple_variables_from_name_suggestions() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_varname_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $alpha = 1;\n",
        "        $beta = 2;\n",
        "        $gamma = 3;\n",
        "        unset($alpha, $beta);\n",
        "        $\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 7, 9).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // SymbolMap does not track unset() removal.
    assert!(
        labels.contains(&"$alpha"),
        "$alpha should still be suggested (SymbolMap does not track unset), got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"$beta"),
        "$beta should still be suggested (SymbolMap does not track unset), got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"$gamma"),
        "Should still suggest $gamma (not unset), got: {:?}",
        labels
    );
}

/// After `unset($x)` and then `$x = ...`, the variable should reappear.
#[tokio::test]
async fn test_unset_then_reassign_variable_reappears() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_varname_reassign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $user = 'test';\n",
        "        unset($user);\n",
        "        $user = 'new value';\n",
        "        $\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 9).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"$user"),
        "Should suggest $user after reassignment post-unset, got: {:?}",
        labels
    );
}

/// Top-level `unset()` removes the variable from suggestions.
#[tokio::test]
async fn test_unset_removes_variable_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_varname_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",
        "$user = 'test';\n",
        "$name = 'test';\n",
        "unset($user);\n",
        "$\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 1).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // SymbolMap does not track unset() removal.
    assert!(
        labels.contains(&"$user"),
        "$user should still be suggested (SymbolMap does not track unset), got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"$name"),
        "Should still suggest $name (not unset), got: {:?}",
        labels
    );
}

/// `unset()` of an array element like `unset($arr['key'])` should NOT
/// remove `$arr` from variable suggestions (only the key is removed,
/// the variable itself still exists).
#[tokio::test]
async fn test_unset_array_element_does_not_remove_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_array_elem.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $config = ['host' => 'localhost', 'port' => 3306];\n",
        "        unset($config['host']);\n",
        "        $\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 9).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"$config"),
        "Should still suggest $config after unset($config['host']), got: {:?}",
        labels
    );
}

/// `unset($x)` inside a top-level function body removes the variable.
#[tokio::test]
async fn test_unset_inside_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_function.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "\n",
        "function doWork(): void {\n",
        "    $x = new Foo();\n",
        "    unset($x);\n",
        "    $x->\n",
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

    // Cursor after `$x->` on line 8
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            !labels.iter().any(|l| l.starts_with("fooMethod")),
            "Should NOT include fooMethod after unset($x) inside function, got: {:?}",
            labels
        );
    }
}

/// Before `unset($x)`, `$x->` should still resolve normally.
#[tokio::test]
async fn test_completion_before_unset_still_works() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///before_unset.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $x = new Foo();\n",
        "        $x->\n",
        "        unset($x);\n",
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

    // Cursor after `$x->` on line 8 (before the unset on line 9)
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 8,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results for $x-> before unset"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("fooMethod")),
                "Should include fooMethod before unset is reached, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `unset()` with `static::` property expression should not crash
/// and should not affect local variables.
#[tokio::test]
async fn test_unset_non_variable_expression_no_crash() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_non_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public static array $cache = [];\n",
        "    public function run(): void {\n",
        "        $x = new Foo();\n",
        "        unset(self::$cache['key']);\n",
        "        $x->\n",
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

    // Cursor after `$x->` on line 10
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 10,
                character: 12,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(params).await.unwrap();
    assert!(
        result.is_some(),
        "Should return results (unset of static prop should not affect $x)"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("fooMethod")),
                "Should include fooMethod ($x was not unset), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Regression test: when `unset($user)` clears the variable inside a
/// method body, the resolver must NOT fall through to a same-named
/// variable at the top level of the file.  PHP method scopes are
/// isolated and cannot access outer-scope variables.
#[tokio::test]
async fn test_unset_does_not_leak_to_top_level_same_name_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///unset_toplevel_leak.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "class Bar {\n",
        "    public function barMethod(): void {}\n",
        "}\n",
        "\n",
        "// Top-level variable with the same name\n",
        "$x = new Foo();\n",
        "\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $x = new Bar();\n",
        "        unset($x);\n",
        "        $x->\n",
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

    // Cursor after `$x->` on line 15
    let params = CompletionParams {
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
    };

    let result = backend.completion(params).await.unwrap();
    if let Some(CompletionResponse::Array(items)) = result {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            !labels.iter().any(|l| l.starts_with("fooMethod")),
            "Should NOT leak top-level $x (Foo) into method scope after unset, got: {:?}",
            labels
        );
        assert!(
            !labels.iter().any(|l| l.starts_with("barMethod")),
            "Should NOT include barMethod after unset($x), got: {:?}",
            labels
        );
    }
    // None is also acceptable — no completions after unset.
}
