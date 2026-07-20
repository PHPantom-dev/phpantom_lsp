//! Tests for namespace-aware function completion.
//!
//! Covers:
//! - Namespaced functions show FQN in `use function` context
//! - Namespaced functions get `use function FQN;` auto-import in inline context
//! - Global functions do NOT get auto-import
//! - Same-namespace functions do NOT get auto-import
//! - Detail shows namespace for namespaced functions
//! - Deduplication uses FQN (no short-name collisions)
//! - Short-name fallback entries are no longer inserted
//! - `filter_current_file_functions` works with FQN-based filtering

use crate::common::{
    create_psr4_workspace, create_test_backend, create_test_backend_with_function_stubs,
};
use phpantom_lsp::atom::atom;
use phpantom_lsp::php_type::PhpType;
use phpantom_lsp::types::FunctionInfo;
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

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

/// Register a namespaced helper function in the global_functions map.
fn register_namespaced_function(
    backend: &phpantom_lsp::Backend,
    fqn: &str,
    name: &str,
    namespace: &str,
    uri: &str,
) {
    {
        let mut fmap = backend.global_functions().write();
        fmap.insert(
            fqn.to_string(),
            (
                uri.to_string(),
                FunctionInfo {
                    name: atom(name),
                    name_offset: 0,
                    parameters: vec![],
                    return_type: Some(PhpType::parse("mixed")),
                    native_return_type: None,
                    description: None,
                    return_description: None,
                    links: vec![],
                    see_refs: vec![],
                    namespace: Some(namespace.to_string()),
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
}

/// Register a global (non-namespaced) function in the global_functions map.
fn register_global_function(backend: &phpantom_lsp::Backend, name: &str, uri: &str) {
    {
        let mut fmap = backend.global_functions().write();
        fmap.insert(
            name.to_string(),
            (
                uri.to_string(),
                FunctionInfo {
                    name: atom(name),
                    name_offset: 0,
                    parameters: vec![],
                    return_type: Some(PhpType::parse("string")),
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
}

// ─── `use function` context ─────────────────────────────────────────────────

/// In `use function` context, a namespaced function's insert_text should be
/// the FQN so the resulting statement reads `use function Ns\func;`.
#[tokio::test]
async fn test_use_function_namespaced_insert_text_is_fqn() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///helpers.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nuse function enum_val\n";

    let items = complete_at(&backend, &uri, text, 1, 21).await;

    let item = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION)
            && i.insert_text.as_deref() == Some("Illuminate\\Support\\enum_value;")
    });
    assert!(
        item.is_some(),
        "use function should insert the FQN. Got insert_texts: {:?}",
        items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|i| i.insert_text.as_deref())
            .collect::<Vec<_>>()
    );
}

/// In `use function` context, the label for a namespaced function should be
/// the FQN (not the short name).
#[tokio::test]
async fn test_use_function_namespaced_label_is_fqn() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///helpers.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nuse function enum_val\n";

    let items = complete_at(&backend, &uri, text, 1, 21).await;

    let item = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION)
            && i.label.contains("Illuminate\\Support\\enum_value")
    });
    assert!(
        item.is_some(),
        "use function label should contain the FQN. Labels: {:?}",
        labels(&items)
    );
}

/// In `use function` context, a global function should still use the short
/// name as the insert text.
#[tokio::test]
async fn test_use_function_global_insert_text_is_short_name() {
    let backend = create_test_backend();

    register_global_function(&backend, "my_global_func", "file:///helpers.php");

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nuse function my_global\n";

    let items = complete_at(&backend, &uri, text, 1, 22).await;

    let item = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION)
            && i.insert_text
                .as_deref()
                .is_some_and(|t| t.contains("my_global_func"))
    });
    assert!(
        item.is_some(),
        "use function for global func should insert the short name. Got: {:?}",
        items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|i| (&i.label, &i.insert_text))
            .collect::<Vec<_>>()
    );
}

/// In `use function` context, the filter_text should be the FQN so the user
/// can type either the namespace or the short name to find the function.
#[tokio::test]
async fn test_use_function_filter_text_is_fqn() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///helpers.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nuse function Illuminate\\Support\\enum\n";

    let items = complete_at(&backend, &uri, text, 1, 40).await;

    let item = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION)
            && i.filter_text
                .as_deref()
                .is_some_and(|ft| ft.contains("Illuminate\\Support\\enum_value"))
    });
    assert!(
        item.is_some(),
        "filter_text should contain the FQN. Got: {:?}",
        items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|i| (&i.label, &i.filter_text))
            .collect::<Vec<_>>()
    );
}

// ─── Inline (non-use) context ───────────────────────────────────────────────

/// In inline context, a namespaced function should get a `use function FQN;`
/// auto-import via additional_text_edits.
#[tokio::test]
async fn test_inline_namespaced_function_gets_auto_import() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///helpers.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nenum_val\n";

    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("enum_value"));
    assert!(
        item.is_some(),
        "Should find enum_value in inline completions. Labels: {:?}",
        labels(&items)
    );

    let item = item.unwrap();
    let edits = item
        .additional_text_edits
        .as_ref()
        .expect("Namespaced function should have additional_text_edits for auto-import");
    assert!(
        !edits.is_empty(),
        "Should have at least one auto-import text edit"
    );
    let edit_text = &edits[0].new_text;
    assert!(
        edit_text.contains("use function Illuminate\\Support\\enum_value;"),
        "Auto-import should insert `use function FQN;`, got: {}",
        edit_text
    );
}

/// In inline context, the insert text should be the short name (with snippet),
/// not the FQN.
#[tokio::test]
async fn test_inline_namespaced_function_insert_text_is_short_name() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///helpers.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nenum_val\n";

    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("enum_value"));
    assert!(item.is_some(), "Should find enum_value");

    let insert = item.unwrap().insert_text.as_deref().unwrap();
    assert!(
        insert.starts_with("enum_value("),
        "Insert text should start with the short name, not the FQN. Got: {}",
        insert
    );
    assert!(
        !insert.contains("Illuminate"),
        "Insert text should NOT contain the namespace. Got: {}",
        insert
    );
}

/// In inline context, a global function should NOT get auto-import.
#[tokio::test]
async fn test_inline_global_function_no_auto_import() {
    let backend = create_test_backend();

    register_global_function(&backend, "my_global_func", "file:///helpers.php");

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nmy_global\n";

    let items = complete_at(&backend, &uri, text, 1, 9).await;

    let item = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("my_global_func")
    });
    assert!(item.is_some(), "Should find my_global_func");

    assert!(
        item.unwrap().additional_text_edits.is_none(),
        "Global function should NOT have auto-import edits"
    );
}

/// In inline context, a function in the same namespace as the current file
/// should NOT get auto-import.
#[tokio::test]
async fn test_inline_same_namespace_function_no_auto_import() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "App\\Helpers\\my_helper",
        "my_helper",
        "App\\Helpers",
        "file:///helpers.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    // File declares the same namespace as the function.
    let text = "<?php\nnamespace App\\Helpers;\n\nmy_help\n";

    let items = complete_at(&backend, &uri, text, 3, 7).await;

    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("my_helper"));
    assert!(
        item.is_some(),
        "Should find my_helper. Labels: {:?}",
        labels(&items)
    );

    assert!(
        item.unwrap().additional_text_edits.is_none(),
        "Same-namespace function should NOT have auto-import edits"
    );
}

// ─── Detail field ───────────────────────────────────────────────────────────

/// In inline context, a namespaced function's detail should show the namespace.
#[tokio::test]
async fn test_inline_namespaced_function_detail_shows_namespace() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///helpers.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nenum_val\n";

    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("enum_value"));
    assert!(item.is_some(), "Should find enum_value");

    let item = item.unwrap();

    // detail now shows return type
    assert_eq!(
        item.detail.as_deref(),
        Some("mixed"),
        "Detail should show return type. Got: {:?}",
        item.detail
    );
}

/// In inline context, a global function's detail should just be "function".
#[tokio::test]
async fn test_inline_global_function_detail_is_plain() {
    let backend = create_test_backend();

    register_global_function(&backend, "my_global_func", "file:///helpers.php");

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nmy_global\n";

    let items = complete_at(&backend, &uri, text, 1, 9).await;

    let item = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("my_global_func")
    });
    assert!(item.is_some(), "Should find my_global_func");

    let item = item.unwrap();

    // detail now shows return type
    assert_eq!(
        item.detail.as_deref(),
        Some("string"),
        "Global function detail should show return type. Got: {:?}",
        item.detail
    );
}

// ─── Deduplication (FQN-based) ──────────────────────────────────────────────

/// Two functions in different namespaces with the same short name should
/// both appear in completions (no short-name collision).
#[tokio::test]
async fn test_different_namespaces_same_short_name_both_appear() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///illuminate.php",
    );
    register_namespaced_function(
        &backend,
        "Symfony\\Component\\enum_value",
        "enum_value",
        "Symfony\\Component",
        "file:///symfony.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nenum_val\n";

    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let matching: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("enum_value"))
        .collect();

    assert!(
        matching.len() >= 2,
        "Both namespaced functions should appear. Got {} matches: {:?}",
        matching.len(),
        matching
            .iter()
            .map(|i| (&i.label, &i.detail))
            .collect::<Vec<_>>()
    );
}

/// In `use function` context, two functions with the same short name in
/// different namespaces should both appear with FQN labels.
#[tokio::test]
async fn test_use_function_different_namespaces_both_appear_with_fqn() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///illuminate.php",
    );
    register_namespaced_function(
        &backend,
        "Symfony\\Component\\enum_value",
        "enum_value",
        "Symfony\\Component",
        "file:///symfony.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nuse function enum_val\n";

    let items = complete_at(&backend, &uri, text, 1, 21).await;

    let matching: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("enum_value"))
        .collect();

    assert!(
        matching.len() >= 2,
        "Both should appear in use function context. Got: {:?}",
        labels(&items)
    );

    let has_illuminate = matching
        .iter()
        .any(|i| i.label.contains("Illuminate\\Support"));
    let has_symfony = matching
        .iter()
        .any(|i| i.label.contains("Symfony\\Component"));
    assert!(
        has_illuminate && has_symfony,
        "Both FQNs should appear as labels. Labels: {:?}",
        matching.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ─── Short-name fallback removal ────────────────────────────────────────────

/// After a namespaced function is registered via file parsing, the
/// global_functions map should contain ONLY the FQN key, not a separate
/// short-name key.
#[tokio::test]
async fn test_no_short_name_fallback_in_global_functions() {
    let backend = create_test_backend();

    // Open a file that defines a namespaced function.
    let uri = Url::parse("file:///helpers.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Illuminate\\Support;\n",
        "\n",
        "function enum_value(mixed $value): mixed {\n",
        "    return $value;\n",
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

    let fmap = backend.global_functions().read();

    assert!(
        fmap.contains_key("Illuminate\\Support\\enum_value"),
        "Should have FQN key. Keys: {:?}",
        fmap.keys().collect::<Vec<_>>()
    );
    assert!(
        !fmap.contains_key("enum_value"),
        "Should NOT have short-name fallback key. Keys: {:?}",
        fmap.keys().collect::<Vec<_>>()
    );
}

// ─── Current-file filtering ─────────────────────────────────────────────────

/// In `use function` context, functions from the current file should be
/// filtered out even when they are namespaced (filter uses FQN matching).
#[tokio::test]
async fn test_use_function_filters_current_file_namespaced() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///current.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Helpers;\n",
        "\n",
        "function my_current_helper(): void {}\n",
        "\n",
        "use function my_current\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 24).await;

    let has_current = items.iter().any(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("my_current_helper")
    });
    assert!(
        !has_current,
        "Functions from the current file should be filtered out in use function context. Labels: {:?}",
        labels(&items)
    );
}

// ─── Cross-file PSR-4 namespaced functions ──────────────────────────────────

/// A namespaced function discovered from a PSR-4 autoload file should appear
/// with correct FQN and auto-import in inline completions.
#[tokio::test]
async fn test_psr4_namespaced_function_completion() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Helpers/helpers.php",
            concat!(
                "<?php\n",
                "namespace App\\Helpers;\n",
                "\n",
                "function format_price(float $amount): string {\n",
                "    return '$' . number_format($amount, 2);\n",
                "}\n",
            ),
        )],
    );

    // Open the helper file so it gets parsed.
    let helpers_path = _dir.path().join("src/Helpers/helpers.php");
    let helpers_uri = Url::from_file_path(&helpers_path).unwrap();
    let helpers_text = std::fs::read_to_string(&helpers_path).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: helpers_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: helpers_text,
            },
        })
        .await;

    // Complete in a different file.
    let test_uri = Url::parse("file:///test.php").unwrap();
    let test_text = "<?php\nformat_pri\n";

    let items = complete_at(&backend, &test_uri, test_text, 1, 10).await;

    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("format_price"));
    assert!(
        item.is_some(),
        "Should find format_price from PSR-4 helper file. Labels: {:?}",
        labels(&items)
    );

    let item = item.unwrap();

    // detail now shows return type.
    assert_eq!(
        item.detail.as_deref(),
        Some("string"),
        "Detail should show return type. Got: {:?}",
        item.detail
    );

    // Auto-import should be present.
    let edits = item
        .additional_text_edits
        .as_ref()
        .expect("Cross-file namespaced function should have auto-import");
    assert!(
        edits[0]
            .new_text
            .contains("use function App\\Helpers\\format_price;"),
        "Auto-import should use FQN. Got: {}",
        edits[0].new_text
    );
}

// ─── Matching by namespace prefix ───────────────────────────────────────────

/// Typing the namespace prefix (e.g. `Illuminate\`) should match
/// namespaced functions.
#[tokio::test]
async fn test_use_function_matches_by_namespace_prefix() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///helpers.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nuse function Illuminate\\\n";

    let items = complete_at(&backend, &uri, text, 1, 28).await;

    let item = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION)
            && i.insert_text
                .as_deref()
                .is_some_and(|t| t.contains("Illuminate\\Support\\enum_value"))
    });
    assert!(
        item.is_some(),
        "Typing namespace prefix should match namespaced functions. Got: {:?}",
        items
            .iter()
            .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
            .map(|i| (&i.label, &i.insert_text))
            .collect::<Vec<_>>()
    );
}

// ─── Stub functions with namespaces ─────────────────────────────────────────

/// Stub functions that are namespaced (e.g. those with backslashes in the
/// stub_function_index key) should show the short name in inline context
/// and the FQN in use function context.
#[tokio::test]
async fn test_stub_global_function_no_auto_import() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\narray_ma\n";

    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let item = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION)
            && i.filter_text.as_deref() == Some("array_map")
    });
    assert!(item.is_some(), "Should find array_map from stubs");

    assert!(
        item.unwrap().additional_text_edits.is_none(),
        "Global stub function should NOT have auto-import edits"
    );
}

// ─── Auto-import placement ──────────────────────────────────────────────────

/// The `use function` auto-import should be inserted at the correct
/// alphabetical position among existing use statements.
#[tokio::test]
async fn test_auto_import_alphabetical_placement() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///helpers.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Models\\User;\n",
        "use Symfony\\Component\\HttpKernel;\n",
        "\n",
        "enum_val\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;

    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("enum_value"));
    assert!(item.is_some(), "Should find enum_value");

    let edits = item
        .unwrap()
        .additional_text_edits
        .as_ref()
        .expect("Should have auto-import");

    // Function imports form their own group AFTER all class imports,
    // separated by a blank line.
    let edit = &edits[0];
    assert_eq!(
        edit.new_text, "\nuse function Illuminate\\Support\\enum_value;\n",
        "Should have blank-line separator before the first function import"
    );
    // Line 2 is `use Symfony\...` (the last class import).
    // The function import goes after it → line 3.
    assert_eq!(
        edit.range.start.line, 3,
        "Should insert after all class imports (line 3). Got line: {}",
        edit.range.start.line
    );
}

// ─── `use function` in `use function` context for namespaced function detail ────

/// In `use function` context, a namespaced user function's detail should
/// show the full signature.
#[tokio::test]
async fn test_use_function_namespaced_detail_shows_signature() {
    let backend = create_test_backend();

    {
        let mut fmap = backend.global_functions().write();
        fmap.insert(
            "Illuminate\\Support\\enum_value".to_string(),
            (
                "file:///helpers.php".to_string(),
                FunctionInfo {
                    name: atom("enum_value"),
                    name_offset: 0,
                    parameters: vec![phpantom_lsp::types::ParameterInfo {
                        name: atom("$value"),
                        is_required: true,
                        type_hint: Some(PhpType::parse("mixed")),
                        native_type_hint: Some(PhpType::parse("mixed")),
                        description: None,
                        default_value: None,
                        is_variadic: false,
                        is_reference: false,
                        closure_this_type: None,
                    }],
                    return_type: Some(PhpType::parse("mixed")),
                    native_return_type: None,
                    description: None,
                    return_description: None,
                    links: vec![],
                    see_refs: vec![],
                    namespace: Some("Illuminate\\Support".to_string()),
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

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nuse function enum_val\n";

    let items = complete_at(&backend, &uri, text, 1, 21).await;

    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("Illuminate"));
    assert!(item.is_some(), "Should find the function");

    let item = item.unwrap();

    // In use-import context, detail is None (build_use_import_item does not set it).
    assert_eq!(
        item.detail, None,
        "Use-import items should have no detail (no return type shown). Got: {:?}",
        item.detail
    );
}

// ─── User function shadows stub ─────────────────────────────────────────────

/// A user-defined function with the same FQN as a stub should shadow the
/// stub (user version wins).
#[tokio::test]
async fn test_user_function_shadows_stub_same_fqn() {
    let backend = create_test_backend_with_function_stubs();

    // Register a user-defined `array_map` that shadows the stub.
    register_global_function(&backend, "array_map", "file:///custom.php");

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\narray_ma\n";

    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let matching: Vec<_> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::FUNCTION)
                && i.filter_text.as_deref() == Some("array_map")
        })
        .collect();

    assert_eq!(
        matching.len(),
        1,
        "Should have exactly one array_map (user version shadows stub). Got: {:?}",
        matching
            .iter()
            .map(|i| (&i.label, &i.detail))
            .collect::<Vec<_>>()
    );
    // detail now shows return type.
    assert_eq!(
        matching[0].detail.as_deref(),
        Some("string"),
        "Should show return type in detail. Got: {:?}",
        matching[0].detail
    );
}

// ─── Deprecated namespaced function ─────────────────────────────────────────

/// A deprecated namespaced function should have the deprecated flag set.
#[tokio::test]
async fn test_deprecated_namespaced_function() {
    let backend = create_test_backend();

    {
        let mut fmap = backend.global_functions().write();
        fmap.insert(
            "Legacy\\old_helper".to_string(),
            (
                "file:///legacy.php".to_string(),
                FunctionInfo {
                    name: atom("old_helper"),
                    name_offset: 0,
                    parameters: vec![],
                    return_type: None,
                    native_return_type: None,
                    description: None,
                    return_description: None,
                    links: vec![],
                    see_refs: vec![],
                    namespace: Some("Legacy".to_string()),
                    conditional_return: None,
                    type_assertions: vec![],
                    deprecation_message: Some("Use newFunc() instead".into()),
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

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nold_hel\n";

    let items = complete_at(&backend, &uri, text, 1, 7).await;

    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("old_helper"));
    assert!(item.is_some(), "Should find old_helper");
    let tags = item.unwrap().tags.as_deref().unwrap_or(&[]);
    assert!(
        tags.contains(&CompletionItemTag::DEPRECATED),
        "Tags should contain DEPRECATED"
    );
}

// ─── Inline context with namespace ──────────────────────────────────────────

/// When the file has a namespace and the function is in a DIFFERENT
/// namespace, auto-import should still be generated.
#[tokio::test]
async fn test_inline_different_namespace_gets_auto_import() {
    let backend = create_test_backend();

    register_namespaced_function(
        &backend,
        "Illuminate\\Support\\enum_value",
        "enum_value",
        "Illuminate\\Support",
        "file:///helpers.php",
    );

    let uri = Url::parse("file:///test.php").unwrap();
    let text = "<?php\nnamespace App\\Services;\n\nenum_val\n";

    let items = complete_at(&backend, &uri, text, 3, 8).await;

    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::FUNCTION) && i.label.contains("enum_value"));
    assert!(item.is_some(), "Should find enum_value");

    let edits = item
        .unwrap()
        .additional_text_edits
        .as_ref()
        .expect("Different-namespace function should get auto-import");
    assert!(
        edits[0]
            .new_text
            .contains("use function Illuminate\\Support\\enum_value;"),
        "Auto-import text should be correct. Got: {}",
        edits[0].new_text
    );
}
