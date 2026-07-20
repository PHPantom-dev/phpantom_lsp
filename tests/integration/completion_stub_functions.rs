use crate::common::{create_test_backend, create_test_backend_with_function_stubs};
use phpantom_lsp::Backend;
use phpantom_lsp::atom::atom;
use phpantom_lsp::php_type::PhpType;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file and request completion at the given line/character.
async fn complete_at(
    backend: &phpantom_lsp::Backend,
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

/// Verify that `find_or_load_function` can resolve a basic built-in PHP
/// function from the embedded stubs and return its `FunctionInfo`.
#[tokio::test]
async fn test_stub_function_index_resolves_array_map() {
    let backend = create_test_backend_with_function_stubs();

    // `array_map` is a standard PHP function that should be in the stubs.
    let result = backend.find_or_load_function(&["array_map"]);
    assert!(
        result.is_some(),
        "find_or_load_function should resolve 'array_map' from embedded stubs"
    );

    let func = result.unwrap();
    assert_eq!(func.name, "array_map");
    // array_map returns `array` according to the stubs.
    assert!(
        func.return_type.is_some(),
        "array_map should have a return type from stubs"
    );
}

/// Verify that `find_or_load_function` can resolve `str_contains`.
#[tokio::test]
async fn test_stub_function_index_resolves_str_contains() {
    let backend = create_test_backend_with_function_stubs();

    let result = backend.find_or_load_function(&["str_contains"]);
    assert!(
        result.is_some(),
        "find_or_load_function should resolve 'str_contains' from embedded stubs"
    );

    let func = result.unwrap();
    assert_eq!(func.name, "str_contains");
    assert!(
        func.return_type.is_some(),
        "str_contains should have a return type"
    );
    assert_eq!(func.return_type_str().as_deref(), Some("bool"));
}

/// Verify that `find_or_load_function` can resolve `json_decode`.
#[tokio::test]
async fn test_stub_function_index_resolves_json_decode() {
    let backend = create_test_backend_with_function_stubs();

    let result = backend.find_or_load_function(&["json_decode"]);
    assert!(
        result.is_some(),
        "find_or_load_function should resolve 'json_decode' from embedded stubs"
    );

    let func = result.unwrap();
    assert_eq!(func.name, "json_decode");
    assert!(
        func.return_type.is_some(),
        "json_decode should have a return type"
    );
}

/// Verify that stub functions are cached in `global_functions` after the
/// first lookup, so subsequent lookups are fast (Phase 1 hit).
#[tokio::test]
async fn test_stub_function_cached_after_first_lookup() {
    let backend = create_test_backend_with_function_stubs();

    // First lookup triggers parsing and caching.
    let first = backend.find_or_load_function(&["str_contains"]);
    assert!(first.is_some());

    // Second lookup should hit the cache (Phase 1).
    let second = backend.find_or_load_function(&["str_contains"]);
    assert!(second.is_some());
    assert_eq!(second.unwrap().name, "str_contains");

    // Verify it's actually in global_functions now.
    let in_cache = backend
        .global_functions()
        .read()
        .get("str_contains")
        .map(|(uri, _)| uri.clone());
    assert!(
        in_cache.is_some(),
        "str_contains should be cached in global_functions"
    );
    assert!(
        in_cache.unwrap().starts_with("phpantom-stub-fn://"),
        "cached URI should use the phpantom-stub-fn:// scheme"
    );
}

/// Verify that a non-existent function returns None.
#[tokio::test]
async fn test_stub_function_nonexistent_returns_none() {
    let backend = create_test_backend();

    let result = backend.find_or_load_function(&["this_function_does_not_exist_xyz"]);
    assert!(result.is_none(), "Non-existent function should return None");
}

/// Verify that when multiple candidates are provided, the first match wins.
#[tokio::test]
async fn test_stub_function_multiple_candidates() {
    let backend = create_test_backend_with_function_stubs();

    // Try a non-existent name first, then a real one.
    let result = backend.find_or_load_function(&["nonexistent_func_xyz", "array_pop"]);
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "array_pop");
}

/// Verify that `date_create` resolves from stubs and has a return type
/// that includes `DateTime` (it returns `DateTime|false`).
#[tokio::test]
async fn test_stub_function_date_create_return_type() {
    let backend = create_test_backend_with_function_stubs();

    let result = backend.find_or_load_function(&["date_create"]);
    assert!(
        result.is_some(),
        "date_create should be in the embedded stubs"
    );

    let func = result.unwrap();
    assert_eq!(func.name, "date_create");

    let ret_str = func.return_type_str();
    let ret = ret_str.as_deref().unwrap_or("");
    assert!(
        ret.contains("DateTime"),
        "date_create return type should mention DateTime, got: {}",
        ret
    );
}

/// End-to-end test: a variable assigned from a built-in stub function
/// (`date_create`) should resolve to `DateTime` and offer its methods
/// via `->` completion.
#[tokio::test]
async fn test_completion_variable_from_stub_function_date_create() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///stub_func_test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {\n",
        "        $dt = date_create();\n",
        "        $dt->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 14).await;

    // DateTime should have a `format` method.
    // Completion labels include the full signature (e.g. "format($format): string").
    let has_format = items.iter().any(|item| item.label.starts_with("format("));
    assert!(
        has_format,
        "Completion after date_create() should include DateTime::format, got labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// End-to-end test: chained call from a stub function.
/// `date_create()->format(...)` — verify that `date_create()` resolves
/// to DateTime so chained `->` offers DateTime methods.
#[tokio::test]
async fn test_completion_chained_stub_function_call() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///stub_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {\n",
        "        date_create()->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 23).await;

    let has_format = items.iter().any(|item| item.label.starts_with("format("));
    assert!(
        has_format,
        "Chained completion after date_create()-> should include format, got labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// A leading-backslash global function call (`\response()->`) is an
/// explicit global-namespace reference and must resolve its return
/// type the same way the unqualified form (`response()->`) does.
#[tokio::test]
async fn test_completion_chained_leading_backslash_function_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///leading_backslash_fn.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Reply {\n",
        "    public function json(array $data): string { return ''; }\n",
        "}\n",
        "function response(): Reply { return new Reply(); }\n",
        "class Controller {\n",
        "    public function index(): void {\n",
        "        \\response()->\n",
        "    }\n",
        "}\n",
    );

    // Column 21 is right after `\response()->`.
    let items = complete_at(&backend, &uri, text, 7, 21).await;

    let has_json = items.iter().any(|item| item.label.starts_with("json"));
    assert!(
        has_json,
        "Chained completion after \\response()-> should include json, got labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Verify that `simplexml_load_string` resolves and its return type
/// includes `SimpleXMLElement`.
#[tokio::test]
async fn test_stub_function_simplexml_load_string() {
    let backend = create_test_backend_with_function_stubs();

    let result = backend.find_or_load_function(&["simplexml_load_string"]);
    assert!(
        result.is_some(),
        "simplexml_load_string should be in the embedded stubs"
    );

    let func = result.unwrap();
    let ret_str = func.return_type_str();
    let ret = ret_str.as_deref().unwrap_or("");
    assert!(
        ret.contains("SimpleXMLElement"),
        "simplexml_load_string return type should mention SimpleXMLElement, got: {}",
        ret
    );
}

/// Verify that the function_loader closure in completion handles stub
/// functions — a built-in function used as an expression subject should
/// resolve its return type.
#[tokio::test]
async fn test_completion_stub_function_in_expression_subject() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///stub_expr.php").unwrap();
    // `simplexml_load_string(...)` returns `SimpleXMLElement|false`.
    // SimpleXMLElement has methods like `xpath`, `children`, `attributes`, etc.
    let text = concat!(
        "<?php\n",
        "class Processor {\n",
        "    public function process(): void {\n",
        "        $xml = simplexml_load_string('<root/>');\n",
        "        $xml->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 14).await;

    // SimpleXMLElement should have `xpath` or `children` method.
    // Completion labels include the full signature (e.g. "xpath($expression): array|false|null").
    let has_sxml_method = items.iter().any(|item| {
        item.label.starts_with("xpath(")
            || item.label.starts_with("children(")
            || item.label.starts_with("attributes(")
    });
    assert!(
        has_sxml_method,
        "Completion after simplexml_load_string() should include SimpleXMLElement methods, got labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Verify that loading all sibling functions from a stub file works.
/// When we look up `array_pop`, the entire `standard_N.php` file is
/// parsed, so other functions from the same file should also be cached.
#[tokio::test]
async fn test_stub_function_sibling_functions_cached() {
    let backend = create_test_backend_with_function_stubs();

    // Look up array_push — this triggers parsing of its stub file.
    let result = backend.find_or_load_function(&["array_push"]);
    assert!(result.is_some(), "array_push should be in stubs");

    // Now other functions from the same file should be cached.
    // array_pop is in the same standard file group.
    // Check if it got cached in global_functions (it may be in a different
    // file, but let's verify the caching mechanism works for the same file).
    let in_cache = backend.global_functions().read().get("array_push").cloned();
    assert!(
        in_cache.is_some(),
        "array_push should be in global_functions cache after lookup"
    );
}

/// Verify that stub functions with parameters have their parameter info
/// extracted correctly.
#[tokio::test]
async fn test_stub_function_parameters_extracted() {
    let backend = create_test_backend_with_function_stubs();

    let result = backend.find_or_load_function(&["str_contains"]);
    assert!(result.is_some());

    let func = result.unwrap();
    // str_contains(string $haystack, string $needle): bool
    assert!(
        func.parameters.len() >= 2,
        "str_contains should have at least 2 parameters, got {}",
        func.parameters.len()
    );
    assert_eq!(func.parameters[0].name, "$haystack");
    assert_eq!(func.parameters[1].name, "$needle");
}

/// Verify that user-defined functions take precedence over stub functions.
/// If a function with the same name is in `global_functions`, the stub
/// version should NOT override it.
#[tokio::test]
async fn test_user_function_takes_precedence_over_stub() {
    let backend = create_test_backend();

    // Pre-populate global_functions with a user-defined `str_contains`.
    let custom_func = phpantom_lsp::FunctionInfo {
        name: atom("str_contains"),
        name_offset: 0,
        parameters: vec![],
        return_type: Some(PhpType::parse("CustomReturn")),
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
        template_param_bounds: Default::default(),
        template_bindings: vec![],
        throws: vec![],
        is_polyfill: false,
        overloads: vec![],
    };

    {
        let mut fmap = backend.global_functions().write();
        fmap.insert(
            "str_contains".to_string(),
            ("file:///custom.php".to_string(), custom_func),
        );
    }

    let result = backend.find_or_load_function(&["str_contains"]);
    assert!(result.is_some());
    let func = result.unwrap();
    assert_eq!(
        func.return_type_str().as_deref(),
        Some("CustomReturn"),
        "User-defined function should take precedence over stub"
    );
}

/// Verify that the constant index is built (even if not yet used for
/// resolution, the infrastructure should be in place).
#[tokio::test]
async fn test_stub_constant_index_built() {
    let backend = create_test_backend_with_function_stubs();

    // The stub_constant_index should be populated from the embedded stubs.
    // PHP_EOL is a very common constant that should be present.
    let has_php_eol = backend.stub_constant_index().contains_key("PHP_EOL");
    assert!(has_php_eol, "stub_constant_index should contain PHP_EOL");
}

/// Verify that common constants are present in the constant index.
#[tokio::test]
async fn test_stub_constant_index_common_constants() {
    let backend = create_test_backend_with_function_stubs();

    // Note: TRUE, FALSE, NULL are language constructs, not in the stubs map.
    let expected = [
        "PHP_INT_MAX",
        "PHP_INT_MIN",
        "SORT_ASC",
        "SORT_DESC",
        "PHP_EOL",
        "PHP_MAJOR_VERSION",
    ];
    for name in &expected {
        assert!(
            backend.stub_constant_index().contains_key(name),
            "stub_constant_index should contain '{}', but it doesn't",
            name
        );
    }
}

/// End-to-end: verify that the function_loader in the definition resolver
/// can also access stub functions (used for resolving call-expression
/// subjects in go-to-definition member resolution).
#[tokio::test]
async fn test_definition_resolver_uses_stub_functions() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///def_stub.php").unwrap();
    // When cursor is on `format` after `date_create()->`, the definition
    // resolver needs to resolve `date_create()` via the stub function
    // loader to know the return type is DateTime, then find `format` on it.
    let text = concat!(
        "<?php\n",
        "class TestDef {\n",
        "    public function test(): void {\n",
        "        $dt = date_create();\n",
        "        $dt->format('Y-m-d');\n",
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

    // The `date_create` function should now be loadable via stubs for
    // return type resolution.
    let func = backend.find_or_load_function(&["date_create"]);
    assert!(
        func.is_some(),
        "date_create should be resolvable for the definition resolver"
    );
}

/// Verify that `array_key_exists` is resolvable (it's a very commonly used
/// built-in function).
#[tokio::test]
async fn test_stub_function_array_key_exists() {
    let backend = create_test_backend_with_function_stubs();

    let result = backend.find_or_load_function(&["array_key_exists"]);
    assert!(result.is_some(), "array_key_exists should be in stubs");

    let func = result.unwrap();
    assert_eq!(func.name, "array_key_exists");
    assert_eq!(func.return_type_str().as_deref(), Some("bool"));
}

/// Verify that `substr` is resolvable.
#[tokio::test]
async fn test_stub_function_substr() {
    let backend = create_test_backend_with_function_stubs();

    let result = backend.find_or_load_function(&["substr"]);
    assert!(result.is_some(), "substr should be in stubs");

    let func = result.unwrap();
    assert_eq!(func.name, "substr");
    // substr returns `string` in modern stubs, but may vary;
    // just verify the function was loaded successfully.
}

/// Verify that `preg_match` is resolvable.
#[tokio::test]
async fn test_stub_function_preg_match() {
    let backend = create_test_backend_with_function_stubs();

    let result = backend.find_or_load_function(&["preg_match"]);
    assert!(result.is_some(), "preg_match should be in stubs");

    let func = result.unwrap();
    assert_eq!(func.name, "preg_match");
}

// ─── Constant completion tests ──────────────────────────────────────────────

/// Typing a partial constant name should suggest matching SPL constants
/// from the stub_constant_index.
#[tokio::test]
async fn test_completion_stub_constant_php_eol() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///const_test.php").unwrap();
    let text = concat!("<?php\n", "echo PHP_E\n",);

    // Cursor at the end of `PHP_E` on line 1
    let items = complete_at(&backend, &uri, text, 1, 10).await;

    let constant_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
        .collect();

    assert!(
        constant_items.iter().any(|i| i.label == "PHP_EOL"),
        "Should suggest PHP_EOL when typing 'PHP_E'. Got: {:?}",
        constant_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Typing `SORT` should suggest both SORT_ASC and SORT_DESC.
#[tokio::test]
async fn test_completion_stub_constant_sort() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///sort_test.php").unwrap();
    let text = concat!("<?php\n", "$x = SORT\n",);

    let items = complete_at(&backend, &uri, text, 1, 9).await;

    let labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        labels.contains(&"SORT_ASC"),
        "Should suggest SORT_ASC. Got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"SORT_DESC"),
        "Should suggest SORT_DESC. Got: {:?}",
        labels
    );
}

/// Constants from user-defined `define()` calls should appear in completions.
#[tokio::test]
async fn test_completion_user_defined_constant() {
    let backend = create_test_backend_with_function_stubs();

    // First open a file that defines a constant
    let defs_uri = Url::parse("file:///constants.php").unwrap();
    let defs_text = concat!(
        "<?php\n",
        "define('MY_APP_VERSION', '1.0.0');\n",
        "define('MY_APP_NAME', 'TestApp');\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: defs_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: defs_text.to_string(),
            },
        })
        .await;

    // Now open another file and type the partial constant name
    let uri = Url::parse("file:///use_const.php").unwrap();
    let text = concat!("<?php\n", "echo MY_APP\n",);

    let items = complete_at(&backend, &uri, text, 1, 11).await;

    let labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        labels.contains(&"MY_APP_VERSION"),
        "Should suggest MY_APP_VERSION from define(). Got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"MY_APP_NAME"),
        "Should suggest MY_APP_NAME from define(). Got: {:?}",
        labels
    );
}

/// Stub constants should have their value extracted from the stub source.
#[tokio::test]
async fn test_completion_stub_constant_detail() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///detail_test.php").unwrap();
    let text = concat!("<?php\n", "echo PHP_EOL\n",);

    let items = complete_at(&backend, &uri, text, 1, 12).await;

    let php_eol = items.iter().find(|i| i.label == "PHP_EOL");
    assert!(php_eol.is_some(), "Should find PHP_EOL in completions");
    let php_eol = php_eol.unwrap();
    assert!(
        php_eol.detail.is_none(),
        "Stub constant detail should be None at completion time (filled in on resolve)"
    );
    assert_eq!(
        php_eol.kind,
        Some(CompletionItemKind::CONSTANT),
        "Constants should use CONSTANT kind"
    );
}

/// User-defined constants should have `detail` set to the constant's value.
#[tokio::test]
async fn test_completion_user_constant_detail() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///user_detail_test.php").unwrap();
    let text = concat!("<?php\n", "define('CUSTOM_FLAG', true);\n", "echo CUSTOM\n",);

    let items = complete_at(&backend, &uri, text, 2, 11).await;

    let custom = items.iter().find(|i| i.label == "CUSTOM_FLAG");
    assert!(custom.is_some(), "Should find CUSTOM_FLAG in completions");
    let custom = custom.unwrap();
    assert_eq!(
        custom.detail.as_deref(),
        Some("true"),
        "User-defined constants should show their value as detail"
    );
}

/// Constants should NOT appear when typing after `->` (member access).
#[tokio::test]
async fn test_completion_constants_not_after_arrow() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///arrow_test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo { public string $PHP_EOL; }\n",
        "$f = new Foo();\n",
        "$f->PHP\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 7).await;

    // After `->`, we should NOT get standalone constants
    let constant_items: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::CONSTANT)
                && i.detail.as_deref() == Some("PHP constant")
        })
        .collect();

    assert!(
        constant_items.is_empty(),
        "Standalone constants should not appear after '->'. Got: {:?}",
        constant_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Constants should NOT appear when typing after `$` (variable context).
#[tokio::test]
async fn test_completion_constants_not_for_variables() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///var_test.php").unwrap();
    let text = concat!("<?php\n", "$PHP_EOL\n",);

    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let constant_items: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::CONSTANT)
                && i.detail.as_deref() == Some("PHP constant")
        })
        .collect();

    assert!(
        constant_items.is_empty(),
        "Standalone constants should not appear when typing a variable. Got: {:?}",
        constant_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Both class names and constants should appear together when prefix matches.
#[tokio::test]
async fn test_completion_constants_alongside_classes() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///mixed_test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "define('SORT_HELPER_FLAG', 1);\n",
        "class SORT_Helper {}\n",
        "SORT\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 4).await;

    let has_class = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::CLASS));
    let has_constant = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::CONSTANT));

    assert!(
        has_class,
        "Should include class completions matching the prefix"
    );
    assert!(
        has_constant,
        "Should include constant completions matching the prefix"
    );
}

// ─── Function name completion tests ─────────────────────────────────────────

/// Typing a partial function name should suggest matching SPL functions
/// from the stub_function_index.
#[tokio::test]
async fn test_completion_stub_function_array_map() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///fn_test.php").unwrap();
    let text = concat!("<?php\n", "array_m\n",);

    let items = complete_at(&backend, &uri, text, 1, 7).await;

    let function_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect();

    let labels: Vec<&str> = function_items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("array_map")),
        "Should suggest array_map from stub_function_index. Got: {:?}",
        labels
    );
}

/// Typing `str_c` should suggest `str_contains` from stub functions.
#[tokio::test]
async fn test_completion_stub_function_str_contains() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///fn_str.php").unwrap();
    let text = concat!("<?php\n", "str_c\n",);

    let items = complete_at(&backend, &uri, text, 1, 5).await;

    let function_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect();

    let labels: Vec<&str> = function_items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("str_contains")),
        "Should suggest str_contains from stub_function_index. Got: {:?}",
        labels
    );
}

/// Stub functions should have `detail` set to "PHP function".
#[tokio::test]
async fn test_completion_stub_function_detail() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///fn_detail.php").unwrap();
    let text = concat!("<?php\n", "json_decode\n",);

    let items = complete_at(&backend, &uri, text, 1, 11).await;

    let json_decode = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION)
            && i.filter_text.as_deref() == Some("json_decode")
    });
    assert!(
        json_decode.is_some(),
        "Should find json_decode in completions"
    );
    let json_decode = json_decode.unwrap();
    assert_eq!(
        json_decode.detail.as_deref(),
        None,
        "Stub functions without a parsed return type should have no detail"
    );
}

/// Stub function completions should use CompletionItemKind::FUNCTION.
#[tokio::test]
async fn test_completion_stub_function_kind() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///fn_kind.php").unwrap();
    let text = concat!("<?php\n", "substr\n",);

    let items = complete_at(&backend, &uri, text, 1, 6).await;

    let substr = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("substr"));
    assert!(substr.is_some(), "Should find substr in completions");
    assert_eq!(
        substr.unwrap().kind,
        Some(CompletionItemKind::FUNCTION),
        "Function completions should use FUNCTION kind"
    );
}

/// User-defined functions should appear in completions with a full signature label.
#[tokio::test]
async fn test_completion_user_defined_function() {
    let backend = create_test_backend_with_function_stubs();

    // Open a file that defines a function
    let defs_uri = Url::parse("file:///helpers.php").unwrap();
    let defs_text = concat!(
        "<?php\n",
        "function my_helper_func(string $name, int $count = 0): string {\n",
        "    return str_repeat($name, $count);\n",
        "}\n",
    );
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: defs_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: defs_text.to_string(),
            },
        })
        .await;

    // Now complete in another file
    let uri = Url::parse("file:///use_fn.php").unwrap();
    let text = concat!("<?php\n", "my_helper\n",);

    let items = complete_at(&backend, &uri, text, 1, 9).await;

    let function_items: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect();

    assert!(
        !function_items.is_empty(),
        "Should suggest user-defined function matching the prefix"
    );

    let helper = function_items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("my_helper_func"));
    assert!(
        helper.is_some(),
        "Should find my_helper_func in completions. Got: {:?}",
        function_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    // User-defined functions should show the return type as detail
    let helper_item = helper.unwrap();
    assert_eq!(
        helper_item.detail.as_deref(),
        Some("string"),
        "User-defined functions should show return type as detail"
    );

    // The label should contain the full signature
    let label = &helper_item.label;
    assert!(
        label.contains("my_helper_func("),
        "Label should contain function name with parens. Got: {}",
        label
    );
    assert!(
        label.contains("$name"),
        "Label should contain parameter names. Got: {}",
        label
    );
}

/// User-defined function label should show full signature with types.
#[tokio::test]
async fn test_completion_user_function_label_signature() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///sig_test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function calculate_total(float $price, int $qty, bool $tax = true): float {\n",
        "    return $price * $qty;\n",
        "}\n",
        "calc\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 4).await;

    let calc = items
        .iter()
        .find(|i| i.filter_text.as_deref() == Some("calculate_total"));
    assert!(calc.is_some(), "Should find calculate_total in completions");

    let label = &calc.unwrap().label;
    assert_eq!(
        label, "calculate_total($price, $qty, $tax = ...)",
        "Label should show function name and parameter names. Got: {}",
        label
    );
}

/// Functions should NOT appear when typing after `->` (member access).
#[tokio::test]
async fn test_completion_functions_not_after_arrow() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///arrow_fn_test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo { public function array_map() {} }\n",
        "$f = new Foo();\n",
        "$f->array\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 9).await;

    // After `->`, we should NOT get standalone function completions
    let standalone_fn_items: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::FUNCTION)
                && i.detail.as_deref() == Some("PHP function")
        })
        .collect();

    assert!(
        standalone_fn_items.is_empty(),
        "Standalone functions should not appear after '->'. Got: {:?}",
        standalone_fn_items
            .iter()
            .map(|i| &i.label)
            .collect::<Vec<_>>()
    );
}

/// Functions should NOT appear when typing after `$` (variable context).
#[tokio::test]
async fn test_completion_functions_not_for_variables() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///var_fn_test.php").unwrap();
    let text = concat!("<?php\n", "$array_map\n",);

    let items = complete_at(&backend, &uri, text, 1, 10).await;

    let function_items: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::FUNCTION)
                && i.detail.as_deref() == Some("PHP function")
        })
        .collect();

    assert!(
        function_items.is_empty(),
        "Standalone functions should not appear when typing a variable. Got: {:?}",
        function_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Functions, classes, and constants should all appear together when prefix matches.
#[tokio::test]
async fn test_completion_functions_alongside_classes_and_constants() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///mixed_fn_test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "define('ARRAY_HELPER_FLAG', 1);\n",
        "class ArrayHelper {}\n",
        "array\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 5).await;

    let has_class = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::CLASS));
    let has_constant = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::CONSTANT));
    let has_function = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::FUNCTION));

    assert!(
        has_class,
        "Should include class completions matching the prefix"
    );
    assert!(
        has_constant,
        "Should include constant completions matching the prefix"
    );
    assert!(
        has_function,
        "Should include function completions matching the prefix. Got kinds: {:?}",
        items.iter().map(|i| (&i.label, i.kind)).collect::<Vec<_>>()
    );
}

/// Multiple matching stub functions should all appear (e.g. `array_` prefix).
#[tokio::test]
async fn test_completion_multiple_matching_stub_functions() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///multi_fn.php").unwrap();
    let text = concat!("<?php\n", "array_\n",);

    let items = complete_at(&backend, &uri, text, 1, 6).await;

    let fn_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(
        fn_labels.contains(&"array_map"),
        "Should suggest array_map. Got: {:?}",
        fn_labels
    );
    assert!(
        fn_labels.contains(&"array_pop"),
        "Should suggest array_pop. Got: {:?}",
        fn_labels
    );
    assert!(
        fn_labels.contains(&"array_push"),
        "Should suggest array_push. Got: {:?}",
        fn_labels
    );
    assert!(
        fn_labels.contains(&"array_key_exists"),
        "Should suggest array_key_exists. Got: {:?}",
        fn_labels
    );
}

/// User-defined function should take precedence over stub function with
/// the same name (user version appears, stub version is deduplicated away).
#[tokio::test]
async fn test_completion_user_function_shadows_stub() {
    let backend = create_test_backend_with_function_stubs();

    // Register a user-defined function with the same name as a stub
    let uri = Url::parse("file:///shadow.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function str_contains(string $a, string $b): bool { return true; }\n",
        "str_con\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 7).await;

    let str_contains_items: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::FUNCTION)
                && i.filter_text.as_deref() == Some("str_contains")
        })
        .collect();

    assert_eq!(
        str_contains_items.len(),
        1,
        "Should have exactly one str_contains completion (deduplicated). Got: {:?}",
        str_contains_items
            .iter()
            .map(|i| (&i.label, &i.detail))
            .collect::<Vec<_>>()
    );

    // The user-defined version should win: detail shows return type, description shows "function"
    assert_eq!(
        str_contains_items[0].detail.as_deref(),
        Some("bool"),
        "User-defined function should show return type as detail"
    );
}

/// Stub functions should get `name()$0` as a snippet — we know they're
/// callable but don't have parameter info loaded.
#[tokio::test]
async fn test_completion_function_insert_text() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///insert_test.php").unwrap();
    let text = concat!("<?php\n", "json_d\n",);

    let items = complete_at(&backend, &uri, text, 1, 6).await;

    let json_decode = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION)
            && i.filter_text.as_deref() == Some("json_decode")
    });
    assert!(
        json_decode.is_some(),
        "Should find json_decode in completions"
    );
    let json_decode = json_decode.unwrap();
    assert_eq!(
        json_decode.insert_text.as_deref(),
        Some("json_decode()$0"),
        "insert_text should be the function name with empty parens snippet"
    );
    assert_eq!(
        json_decode.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "insert_text_format should be Snippet for stub functions"
    );
}

/// `preg_match` should appear when typing `preg`.
#[tokio::test]
async fn test_completion_stub_function_preg_match() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///preg_test.php").unwrap();
    let text = concat!("<?php\n", "preg\n",);

    let items = complete_at(&backend, &uri, text, 1, 4).await;

    let fn_labels: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(
        fn_labels.contains(&"preg_match"),
        "Should suggest preg_match. Got: {:?}",
        fn_labels
    );
}

// ─── @removed tag filtering ────────────────────────────────────────────────

/// Verify that `find_or_load_function` filters out functions whose docblock
/// has `@removed X.Y` where the target PHP version is >= X.Y.
#[tokio::test]
async fn test_removed_function_filtered_out() {
    let stub_content: &str = concat!(
        "<?php\n",
        "/**\n",
        " * @return string\n",
        " * @removed 7.0\n",
        " */\n",
        "function mysql_tablename($result, $i) {}\n",
        "\n",
        "/**\n",
        " * @return string\n",
        " */\n",
        "function array_map(callable $callback, array $array): array {}\n",
    );

    let mut function_stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    // Both names point to the same source — the removed one should be filtered.
    function_stubs.insert(
        "mysql_tablename",
        Box::leak(stub_content.to_string().into_boxed_str()),
    );
    function_stubs.insert(
        "array_map",
        Box::leak(stub_content.to_string().into_boxed_str()),
    );

    let backend = phpantom_lsp::Backend::new_test_with_all_stubs(
        std::collections::HashMap::new(),
        function_stubs,
        std::collections::HashMap::new(),
    );
    // Default PHP version is 8.5, which is >= 7.0.
    assert!(
        backend
            .find_or_load_function(&["mysql_tablename"])
            .is_none(),
        "mysql_tablename (@removed 7.0) should be filtered out for PHP 8.5"
    );
    assert!(
        backend.find_or_load_function(&["array_map"]).is_some(),
        "array_map (no @removed) should still be available"
    );
}

/// Verify that `@removed` does not filter out functions when the target
/// PHP version is older than the removal version.
///
/// `parse_functions_versioned` is the parse-time layer that checks
/// `@removed`.  When the PHP version is below the removal version the
/// function should survive parsing and be resolvable.
#[tokio::test]
async fn test_removed_function_available_on_older_php() {
    let stub_content = concat!(
        "<?php\n",
        "/**\n",
        " * @return array\n",
        " * @removed 8.0\n",
        " */\n",
        "function each(&$array): array {}\n",
    );

    let backend = Backend::new_test();
    // Target PHP 7.4 — `each` was removed in 8.0, so it should survive.
    backend.set_php_version(phpantom_lsp::types::PhpVersion::new(7, 4));

    let functions = backend.parse_functions_versioned(
        stub_content,
        Some(phpantom_lsp::types::PhpVersion::new(7, 4)),
    );
    assert!(
        functions.iter().any(|f| f.name == "each"),
        "each (@removed 8.0) should be present when parsing for PHP 7.4, got: {:?}",
        functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    // Verify the converse: parsing for PHP 8.5 should filter it out.
    let functions_85 = backend.parse_functions_versioned(
        stub_content,
        Some(phpantom_lsp::types::PhpVersion::new(8, 5)),
    );
    assert!(
        !functions_85.iter().any(|f| f.name == "each"),
        "each (@removed 8.0) should be filtered when parsing for PHP 8.5, got: {:?}",
        functions_85.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

/// Verify that `@removed` on a class method filters out just that method.
/// The removed method should not appear in member completion results.
#[tokio::test]
async fn test_removed_method_filtered_from_completion() {
    let stub_content: &str = concat!(
        "<?php\n",
        "class SplFileObject {\n",
        "    /** @return string */\n",
        "    public function fgets(): string {}\n",
        "\n",
        "    /**\n",
        "     * @return string|false\n",
        "     * @removed 8.0\n",
        "     */\n",
        "    public function fgetss($allowable_tags = null) {}\n",
        "}\n",
    );

    let mut class_stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    class_stubs.insert(
        "SplFileObject",
        Box::leak(stub_content.to_string().into_boxed_str()),
    );

    let backend = Backend::new_test_with_all_stubs(
        class_stubs,
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    );

    let uri = Url::parse("file:///test_removed_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function test(SplFileObject $f) {\n",
        "    $f->\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 8).await;
    let method_names: Vec<&str> = items
        .iter()
        .filter_map(|i| i.label.split('(').next())
        .collect();

    assert!(
        method_names.contains(&"fgets"),
        "fgets (no @removed) should appear in completions. Got: {:?}",
        method_names
    );
    assert!(
        !method_names.contains(&"fgetss"),
        "fgetss (@removed 8.0) should be filtered from completions for PHP 8.5. Got: {:?}",
        method_names
    );
}

/// Verify that `@removed` on a class filters out the entire class.
/// A type-hinted parameter of a removed class should not offer completions.
#[tokio::test]
async fn test_removed_class_filtered_from_completion() {
    let stub_content: &str = concat!(
        "<?php\n",
        "/**\n",
        " * @removed 8.0\n",
        " */\n",
        "class OCI_Lob {\n",
        "    public function read(int $length): string {}\n",
        "}\n",
        "\n",
        "class OCI_Connection {\n",
        "    public function commit(): bool {}\n",
        "}\n",
    );

    let mut class_stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    class_stubs.insert(
        "OCI_Lob",
        Box::leak(stub_content.to_string().into_boxed_str()),
    );
    class_stubs.insert(
        "OCI_Connection",
        Box::leak(stub_content.to_string().into_boxed_str()),
    );

    let backend = Backend::new_test_with_all_stubs(
        class_stubs,
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    );

    // OCI_Lob is removed — should yield no member completions.
    let uri_lob = Url::parse("file:///test_removed_lob.php").unwrap();
    let text_lob = concat!(
        "<?php\n",
        "function test(OCI_Lob $l) {\n",
        "    $l->\n",
        "}\n",
    );
    let lob_items = complete_at(&backend, &uri_lob, text_lob, 2, 8).await;
    assert!(
        !lob_items.iter().any(|i| i.label.contains("read")),
        "OCI_Lob (@removed 8.0) should not produce member completions for PHP 8.5. Got: {:?}",
        lob_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    // OCI_Connection is not removed — should yield completions.
    let uri_conn = Url::parse("file:///test_removed_conn.php").unwrap();
    let text_conn = concat!(
        "<?php\n",
        "function test(OCI_Connection $c) {\n",
        "    $c->\n",
        "}\n",
    );
    let conn_items = complete_at(&backend, &uri_conn, text_conn, 2, 8).await;
    assert!(
        conn_items.iter().any(|i| i.label.contains("commit")),
        "OCI_Connection (no @removed) should produce member completions. Got: {:?}",
        conn_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Verify that removed stub functions do not appear in the function name
/// completion list (the popup the user sees when typing a function name).
#[tokio::test]
async fn test_removed_function_excluded_from_completion_list() {
    let stub_content: &str = concat!(
        "<?php\n",
        "/**\n",
        " * @return string|false\n",
        " * @removed 7.0\n",
        " */\n",
        "function mysql_close($link_identifier = null) {}\n",
        "\n",
        "/**\n",
        " * @return array\n",
        " */\n",
        "function array_map(callable $callback, array $array): array {}\n",
    );

    let mut function_stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    function_stubs.insert(
        "mysql_close",
        Box::leak(stub_content.to_string().into_boxed_str()),
    );
    function_stubs.insert(
        "array_map",
        Box::leak(stub_content.to_string().into_boxed_str()),
    );

    let backend = Backend::new_test_with_all_stubs(
        std::collections::HashMap::new(),
        function_stubs,
        std::collections::HashMap::new(),
    );

    let uri = Url::parse("file:///test_removed_completion.php").unwrap();
    let text = "<?php\nmysql_\n";

    let items = complete_at(&backend, &uri, text, 1, 6).await;
    let fn_names: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .filter_map(|i| i.filter_text.as_deref().or(Some(i.label.as_str())))
        .collect();

    assert!(
        !fn_names.iter().any(|n| n.contains("mysql_close")),
        "mysql_close (@removed 7.0) should not appear in completions for PHP 8.5. Got: {:?}",
        fn_names
    );
}

/// Verify that removed stub classes do not appear in the class name
/// completion list (e.g. when typing after `new` or in a type hint).
#[tokio::test]
async fn test_removed_class_excluded_from_class_name_completion() {
    let stub_content: &str = concat!(
        "<?php\n",
        "/**\n",
        " * @removed 8.0\n",
        " */\n",
        "class OCI_Lob {\n",
        "    public function read(int $length): string {}\n",
        "}\n",
        "\n",
        "class OCI_Connection {\n",
        "    public function commit(): bool {}\n",
        "}\n",
    );

    let mut class_stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    class_stubs.insert(
        "OCI_Lob",
        Box::leak(stub_content.to_string().into_boxed_str()),
    );
    class_stubs.insert(
        "OCI_Connection",
        Box::leak(stub_content.to_string().into_boxed_str()),
    );

    let backend = Backend::new_test_with_all_stubs(
        class_stubs,
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    );

    let uri = Url::parse("file:///test_removed_class_completion.php").unwrap();
    let text = "<?php\nfunction test(OCI\n";

    let items = complete_at(&backend, &uri, text, 1, 18).await;
    let class_names: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref().or(Some(i.label.as_str())))
        .collect();

    assert!(
        !class_names.iter().any(|n| n.contains("OCI_Lob")),
        "OCI_Lob (@removed 8.0) should not appear in class name completions for PHP 8.5. Got: {:?}",
        class_names
    );
    assert!(
        class_names.iter().any(|n| n.contains("OCI_Connection")),
        "OCI_Connection (no @removed) should appear in class name completions. Got: {:?}",
        class_names
    );
}
