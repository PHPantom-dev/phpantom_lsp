//! Integration tests for non-Composer function and constant discovery.
//!
//! These tests verify that the autoload function and constant indices
//! (populated by the workspace scanner for non-Composer projects) feed
//! into completion, go-to-definition, and hover correctly.

use crate::common::create_test_backend;
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

// ═══════════════════════════════════════════════════════════════════════════
// Function completion from autoload index
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn autoload_function_index_appears_in_completion() {
    let backend = create_test_backend();

    // Simulate a workspace scan that discovered a function in another file
    // without doing a full AST parse.
    let dir = tempfile::tempdir().unwrap();
    let helper_path = dir.path().join("helpers.php");
    std::fs::write(
        &helper_path,
        r#"<?php
function formatDate(string $date): string {
    return date('Y-m-d', strtotime($date));
}
"#,
    )
    .unwrap();

    // Populate the autoload function index (simulating what
    // scan_workspace_fallback_full does during initialization).
    {
        let mut idx = backend.autoload_function_index().write();
        idx.insert("formatDate".to_string(), helper_path.clone());
    }

    // Open a file that calls the function.
    let uri = Url::parse("file:///test_caller.php").unwrap();
    let src = "<?php\nformat";

    let items = complete_at(&backend, &uri, src, 1, 6).await;
    let names: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(
        names.iter().any(|n| n.contains("formatDate")),
        "autoload function should appear in completions, got: {:?}",
        names
    );
}

#[tokio::test]
async fn autoload_function_index_deduplicates_with_global_functions() {
    let backend = create_test_backend();

    let dir = tempfile::tempdir().unwrap();
    let helper_path = dir.path().join("helpers.php");
    std::fs::write(&helper_path, "<?php\nfunction myFunc(): void {}").unwrap();

    // The function exists in both global_functions (already parsed) and
    // the autoload index.  It should appear only once.
    let uri_helpers = format!("file://{}", helper_path.display());
    backend.update_ast(&uri_helpers, "<?php\nfunction myFunc(): void {}");

    {
        let mut idx = backend.autoload_function_index().write();
        idx.insert("myFunc".to_string(), helper_path);
    }

    let uri = Url::parse("file:///test_dedup.php").unwrap();
    let src = "<?php\nmyFu";

    let items = complete_at(&backend, &uri, src, 1, 4).await;
    let count = items.iter().filter(|i| i.label.contains("myFunc")).count();
    assert_eq!(
        count, 1,
        "function should appear exactly once, got {} occurrences",
        count
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Constant completion from autoload index
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn autoload_constant_index_appears_in_completion() {
    let backend = create_test_backend();

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.php");
    std::fs::write(
        &config_path,
        "<?php\ndefine('APP_VERSION', '1.0.0');\nconst DEBUG_MODE = true;",
    )
    .unwrap();

    {
        let mut idx = backend.autoload_constant_index().write();
        idx.insert("APP_VERSION".to_string(), config_path.clone());
        idx.insert("DEBUG_MODE".to_string(), config_path);
    }

    let uri = Url::parse("file:///test_const.php").unwrap();
    let src = "<?php\nAPP_";

    let items = complete_at(&backend, &uri, src, 1, 4).await;
    let names: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(
        names.iter().any(|n| n.contains("APP_VERSION")),
        "autoload constant should appear in completions, got: {:?}",
        names
    );
}

#[tokio::test]
async fn autoload_constant_index_deduplicates_with_global_defines() {
    let backend = create_test_backend();

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.php");
    std::fs::write(&config_path, "<?php\ndefine('MY_CONST', 42);").unwrap();

    // Parse the file so it's in global_defines.
    let uri_config = format!("file://{}", config_path.display());
    backend.update_ast(&uri_config, "<?php\ndefine('MY_CONST', 42);");

    // Also add to autoload index.
    {
        let mut idx = backend.autoload_constant_index().write();
        idx.insert("MY_CONST".to_string(), config_path);
    }

    let uri = Url::parse("file:///test_dedup_const.php").unwrap();
    let src = "<?php\nMY_CO";

    let items = complete_at(&backend, &uri, src, 1, 5).await;
    let count = items
        .iter()
        .filter(|i| i.label.contains("MY_CONST"))
        .count();
    assert_eq!(
        count, 1,
        "constant should appear exactly once, got {} occurrences",
        count
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Lazy function resolution from autoload index
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn autoload_function_index_resolves_return_type() {
    let backend = create_test_backend();

    // Write a PHP file with a function that returns a known type.
    let dir = tempfile::tempdir().unwrap();
    let helper_path = dir.path().join("helpers.php");
    std::fs::write(
        &helper_path,
        r#"<?php
class Result {
    public string $value;
}

function getResult(): Result {
    return new Result();
}
"#,
    )
    .unwrap();

    // Only the autoload index knows about this function — it hasn't
    // been parsed yet.
    {
        let mut idx = backend.autoload_function_index().write();
        idx.insert("getResult".to_string(), helper_path);
    }

    // Resolve the function — this should trigger lazy parsing.
    let info = backend.find_or_load_function(&["getResult"]);
    assert!(
        info.is_some(),
        "function should be resolved from autoload index"
    );
    let info = info.unwrap();
    assert_eq!(
        info.return_type_str().as_deref(),
        Some("Result"),
        "return type should be resolved after lazy parse"
    );

    // After lazy parse, the function should be cached in global_functions.
    let cached = backend.global_functions().read().contains_key("getResult");
    assert!(
        cached,
        "function should be cached in global_functions after lazy parse"
    );
}

#[test]
fn autoload_function_index_does_not_override_already_parsed() {
    let backend = create_test_backend();

    // Parse a file with a function first.
    let uri = "file:///already_parsed.php";
    let src = "<?php\nfunction earlyFunc(): string { return 'hello'; }";
    backend.update_ast(uri, src);

    // Then add an autoload index entry pointing to a different file
    // (simulating a stale index or duplicate).
    let dir = tempfile::tempdir().unwrap();
    let other_path = dir.path().join("other.php");
    std::fs::write(
        &other_path,
        "<?php\nfunction earlyFunc(): int { return 42; }",
    )
    .unwrap();

    {
        let mut idx = backend.autoload_function_index().write();
        idx.insert("earlyFunc".to_string(), other_path);
    }

    // The already-parsed version should win.
    let info = backend.find_or_load_function(&["earlyFunc"]);
    assert!(info.is_some());
    assert_eq!(
        info.unwrap().return_type_str().as_deref(),
        Some("string"),
        "already-parsed function should take priority over autoload index"
    );
}

#[test]
fn autoload_function_lazy_parse_also_discovers_classes() {
    let backend = create_test_backend();

    // Write a file that contains both a function and a class.
    let dir = tempfile::tempdir().unwrap();
    let helper_path = dir.path().join("helpers.php");
    std::fs::write(
        &helper_path,
        r#"<?php
class HelperResult {
    public int $code;
}

function getHelperResult(): HelperResult {
    return new HelperResult();
}
"#,
    )
    .unwrap();

    {
        let mut idx = backend.autoload_function_index().write();
        idx.insert("getHelperResult".to_string(), helper_path);
    }

    // Resolving the function should also make the class available.
    let info = backend.find_or_load_function(&["getHelperResult"]);
    assert!(info.is_some(), "function should resolve");

    // The class defined in the same file should now be discoverable
    // via the class index (populated by update_ast).
    let ci = backend.fqn_uri_index().read();
    let has_class = ci.keys().any(|k| k.contains("HelperResult"));
    assert!(
        has_class,
        "class from the same file should be available after lazy parse, class_index keys: {:?}",
        ci.keys().collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Namespaced function completion from autoload index
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn namespaced_function_completion_from_autoload_index() {
    let backend = create_test_backend();

    let dir = tempfile::tempdir().unwrap();
    let helper_path = dir.path().join("helpers.php");
    std::fs::write(
        &helper_path,
        "<?php\nnamespace App\\Helpers;\nfunction formatMoney(float $amount): string { return ''; }",
    )
    .unwrap();

    {
        let mut idx = backend.autoload_function_index().write();
        idx.insert("App\\Helpers\\formatMoney".to_string(), helper_path);
    }

    let uri = Url::parse("file:///test_ns_func.php").unwrap();
    let src = "<?php\nformatMon";

    let items = complete_at(&backend, &uri, src, 1, 9).await;
    let names: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    assert!(
        names.iter().any(|n| n.contains("formatMoney")),
        "namespaced autoload function should appear in completions, got: {:?}",
        names
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Workspace scanner integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn scan_workspace_fallback_full_discovers_all_symbol_types() {
    use phpantom_lsp::classmap_scanner::scan_workspace_fallback_full;

    let dir = tempfile::tempdir().unwrap();

    // Create a multi-file non-Composer project.
    std::fs::write(
        dir.path().join("User.php"),
        "<?php\nnamespace App;\nclass User {}",
    )
    .unwrap();

    std::fs::write(
        dir.path().join("helpers.php"),
        "<?php\nfunction formatDate(): string { return ''; }\ndefine('APP_NAME', 'Test');\nconst DEBUG = true;\n",
    )
    .unwrap();

    let sub = dir.path().join("lib");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join("math.php"),
        "<?php\nnamespace Lib;\nfunction add(int $a, int $b): int { return $a + $b; }",
    )
    .unwrap();

    let skip = std::collections::HashSet::new();
    let result = scan_workspace_fallback_full(dir.path(), &skip);

    // Classes
    assert!(
        result.classmap.contains_key("App\\User"),
        "should find namespaced class: {:?}",
        result.classmap.keys().collect::<Vec<_>>()
    );

    // Functions
    assert!(
        result.function_index.contains_key("formatDate"),
        "should find global function: {:?}",
        result.function_index.keys().collect::<Vec<_>>()
    );
    assert!(
        result.function_index.contains_key("Lib\\add"),
        "should find namespaced function: {:?}",
        result.function_index.keys().collect::<Vec<_>>()
    );

    // Constants
    assert!(
        result.constant_index.contains_key("APP_NAME"),
        "should find define() constant: {:?}",
        result.constant_index.keys().collect::<Vec<_>>()
    );
    assert!(
        result.constant_index.contains_key("DEBUG"),
        "should find top-level const: {:?}",
        result.constant_index.keys().collect::<Vec<_>>()
    );
}

#[test]
fn scan_workspace_fallback_full_excludes_class_methods_and_constants() {
    use phpantom_lsp::classmap_scanner::scan_workspace_fallback_full;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Service.php"),
        "<?php\nclass Service {\n    const VERSION = '1.0';\n    public function handle(): void {}\n    private function helper(): void {}\n}\n",
    )
    .unwrap();

    let skip = std::collections::HashSet::new();
    let result = scan_workspace_fallback_full(dir.path(), &skip);

    assert!(
        result.classmap.contains_key("Service"),
        "class should be found"
    );
    assert!(
        result.function_index.is_empty(),
        "class methods should not appear as functions: {:?}",
        result.function_index
    );
    assert!(
        result.constant_index.is_empty(),
        "class constants should not appear as top-level constants: {:?}",
        result.constant_index
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Scanner edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn find_symbols_arrow_function_not_captured() {
    use phpantom_lsp::classmap_scanner::find_symbols;

    let content = b"<?php\n$fn = fn(int $x): int => $x * 2;\nfunction realFunc(): void {}\n";
    let result = find_symbols(content);
    assert_eq!(result.functions, vec!["realFunc"]);
}

#[test]
fn find_symbols_nested_function_not_captured() {
    use phpantom_lsp::classmap_scanner::find_symbols;

    // Functions defined inside other functions are not top-level.
    let content = b"<?php\nfunction outer(): void {\n    function inner(): void {}\n}\n";
    let result = find_symbols(content);
    // outer is at depth 0 (top-level), inner is at depth 1 (inside outer's body).
    assert_eq!(
        result.functions,
        vec!["outer"],
        "only top-level function should be captured"
    );
}

#[test]
fn find_symbols_define_with_namespace_constant() {
    use phpantom_lsp::classmap_scanner::find_symbols;

    // define() with a namespaced constant name.
    let content = b"<?php\ndefine('App\\Config\\DB_HOST', 'localhost');\n";
    let result = find_symbols(content);
    assert_eq!(result.constants, vec!["App\\Config\\DB_HOST"]);
}

#[test]
fn find_symbols_multiple_namespaces_semicolon_form() {
    use phpantom_lsp::classmap_scanner::find_symbols;

    let content = b"<?php\nnamespace First;\nfunction firstFunc(): void {}\nconst FIRST_CONST = 1;\n\nnamespace Second;\nfunction secondFunc(): void {}\nconst SECOND_CONST = 2;\n";
    let result = find_symbols(content);
    assert_eq!(
        result.functions,
        vec!["First\\firstFunc", "Second\\secondFunc"]
    );
    assert_eq!(
        result.constants,
        vec!["First\\FIRST_CONST", "Second\\SECOND_CONST"]
    );
}

#[test]
fn find_symbols_global_namespace_block() {
    use phpantom_lsp::classmap_scanner::find_symbols;

    let content = b"<?php\nnamespace App {\n    function appFunc(): void {}\n    const APP_VER = '1.0';\n}\nnamespace {\n    function globalFunc(): void {}\n    const GLOBAL_CONST = true;\n}\n";
    let result = find_symbols(content);
    assert!(
        result.functions.contains(&"App\\appFunc".to_string()),
        "should find namespaced function: {:?}",
        result.functions
    );
    assert!(
        result.functions.contains(&"globalFunc".to_string()),
        "should find global namespace function: {:?}",
        result.functions
    );
    assert!(
        result.constants.contains(&"App\\APP_VER".to_string()),
        "should find namespaced const: {:?}",
        result.constants
    );
    assert!(
        result.constants.contains(&"GLOBAL_CONST".to_string()),
        "should find global namespace const: {:?}",
        result.constants
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Eager autoload-file preloading
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn preload_autoload_files_resolves_function_exists_guarded_helper() {
    let backend = create_test_backend();

    // A helper guarded by `function_exists` lives at brace depth > 0, so
    // the byte-level scanner never records it in the autoload function
    // index.  Eager preloading runs a full parse, which finds it.
    let dir = tempfile::tempdir().unwrap();
    let helper_path = dir.path().join("helpers.php");
    std::fs::write(
        &helper_path,
        r#"<?php
if (! function_exists('guardedHelper')) {
    function guardedHelper(): string {
        return 'ok';
    }
}
"#,
    )
    .unwrap();

    // The function is unknown before preloading (the byte scan missed it
    // and nothing has been parsed).
    assert!(
        backend.find_or_load_function(&["guardedHelper"]).is_none(),
        "guarded helper should not be resolvable before preloading"
    );

    backend.preload_autoload_files(&[helper_path]);

    let info = backend.find_or_load_function(&["guardedHelper"]);
    assert!(
        info.is_some(),
        "guarded helper should resolve after eager preloading"
    );
    assert_eq!(
        info.unwrap().return_type_str().as_deref(),
        Some("string"),
        "preloaded helper should carry its full signature"
    );
    assert!(
        backend
            .global_functions()
            .read()
            .contains_key("guardedHelper"),
        "preloaded helper should be cached in global_functions"
    );
}

#[test]
fn preload_autoload_files_skips_already_parsed() {
    let backend = create_test_backend();

    // Parse a file up front with one signature.
    let dir = tempfile::tempdir().unwrap();
    let helper_path = dir.path().join("helpers.php");
    let uri = format!("file://{}", helper_path.display());
    backend.update_ast(&uri, "<?php\nfunction stableFunc(): string { return ''; }");

    // The on-disk content differs (a stale copy).  Preloading must not
    // re-parse it, so the already-parsed signature is preserved.
    std::fs::write(
        &helper_path,
        "<?php\nfunction stableFunc(): int { return 0; }",
    )
    .unwrap();

    backend.preload_autoload_files(&[helper_path]);

    let info = backend.find_or_load_function(&["stableFunc"]);
    assert_eq!(
        info.and_then(|i| i.return_type_str().map(|s| s.to_string())),
        Some("string".to_string()),
        "already-parsed file must not be re-parsed by preloading"
    );
}
