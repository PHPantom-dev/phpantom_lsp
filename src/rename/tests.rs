#![cfg(test)]
//! Rename tests that reach past the crate boundary.
//!
//! The rest of the suite lives in `tests/integration/rename_*.rs`.  These
//! stay here because they drive `plan_class_move`, seed the Laravel macro
//! index directly, or index a file without opening it, none of which the
//! public `Backend` API exposes.

use crate::Backend;
use crate::test_fixtures::apply_edits;
use crate::virtual_members::laravel::extract_macro_registrations;
use std::sync::atomic::Ordering;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file in the backend.
async fn open_file(backend: &Backend, uri: &Url, text: &str) {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;
}

/// Helper: send a rename request and return the workspace edit.
async fn rename(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    rename_result(backend, uri, line, character, new_name)
        .await
        .expect("rename was refused")
}

/// Like [`rename`] but keeps the refusal, so a test can assert on the
/// message the user is shown.
async fn rename_result(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    new_name: &str,
) -> std::result::Result<Option<WorkspaceEdit>, String> {
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        new_name: new_name.to_string(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    backend
        .rename(params)
        .await
        .map_err(|e| e.message.to_string())
}

fn seed_macro_index(backend: &Backend, uri: &Url, text: &str) {
    let mut index = backend.laravel_macros.write();
    index.set_file(
        uri.to_string(),
        extract_macro_registrations(text, Some(*backend.workspace.php_version.lock())),
    );
    index.rebuild();
    backend
        .laravel_has_macros
        .store(!index.is_empty(), Ordering::Relaxed);
}

fn line_char_of(haystack: &str, needle: &str) -> (u32, u32) {
    for (line_idx, line) in haystack.lines().enumerate() {
        if let Some(char_idx) = line.find(needle) {
            return (line_idx as u32, char_idx as u32);
        }
    }
    panic!("needle not found: {needle}");
}

/// Collect all text edits for a given URI from a WorkspaceEdit.
fn edits_for_uri(edit: &WorkspaceEdit, uri: &Url) -> Vec<TextEdit> {
    if let Some(changes) = edit.changes.as_ref() {
        return changes.get(uri).cloned().unwrap_or_default();
    }
    let Some(DocumentChanges::Operations(ops)) = &edit.document_changes else {
        return Vec::new();
    };
    ops.iter()
        .filter_map(|op| match op {
            DocumentChangeOperation::Edit(e) if e.text_document.uri == *uri => Some(&e.edits),
            _ => None,
        })
        .flatten()
        .map(|e| match e {
            OneOf::Left(e) => e.clone(),
            OneOf::Right(e) => e.text_edit.clone(),
        })
        .collect()
}

#[tokio::test]
async fn rename_namespace_updates_use_statements_in_indexed_unopened_file() {
    use std::path::PathBuf;

    let workspace = PathBuf::from("/tmp/test_workspace_ns_indexed_unopened");
    let _ = std::fs::create_dir_all(&workspace);

    let backend = Backend::new_test_with_workspace(workspace.clone(), Vec::new());
    let uri_a = Url::from_file_path(workspace.join("a.php")).unwrap();
    let uri_b = Url::from_file_path(workspace.join("b.php")).unwrap();

    let text_a = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "class PaymentService {}\n",
    );

    let text_b = concat!(
        "<?php\n",
        "use App\\Services\\PaymentService;\n",
        "class BillingController {\n",
        "    public function handle(PaymentService $p): void {}\n",
        "}\n",
    );

    std::fs::write(workspace.join("a.php"), text_a).unwrap();
    std::fs::write(workspace.join("b.php"), text_b).unwrap();

    open_file(&backend, &uri_a, text_a).await;

    // Index b.php without opening it. This simulates a workspace-scanned file
    // that has class metadata available but no live file_imports/symbol_map.
    backend.parse_and_cache_content(text_b, uri_b.as_str());

    let edit = rename(&backend, &uri_a, 1, 15, "Handlers").await;
    assert!(edit.is_some(), "Expected workspace edit");
    let edit = edit.unwrap();

    let edits_b = edits_for_uri(&edit, &uri_b);
    assert!(
        !edits_b.is_empty(),
        "Expected edits in indexed unopened file b"
    );

    let result_b = apply_edits(text_b, &edits_b);
    assert!(
        result_b.contains("use App\\Handlers\\PaymentService;"),
        "Use statement should be updated in indexed unopened file: {}",
        result_b
    );

    let _ = std::fs::remove_dir_all(&workspace);
}

#[tokio::test]
async fn rename_class_move_into_global_namespace_removes_the_namespace_statement() {
    // The destination has no namespace to write in place of the old
    // one, so leaving the statement behind would spell `namespace ;`.
    let backend = Backend::new_test();

    let uri = Url::parse("file:///src/Widget.php").unwrap();
    let text = concat!(
        "<?php\n",
        "\n",
        "namespace App\\Old;\n",
        "\n",
        "class Widget {}\n",
    );

    open_file(&backend, &uri, text).await;

    let ws = backend
        .plan_class_move("App\\Old\\Widget", "Widget")
        .expect("the move should be planned")
        .expect("Expected a workspace edit for the class move");

    let result = apply_edits(text, &edits_for_uri(&ws, &uri));
    assert_eq!(
        result, "<?php\n\nclass Widget {}\n",
        "The whole `namespace` statement should go; got:\n{result}"
    );
}

#[tokio::test]
async fn rename_class_move_into_global_namespace_writes_siblings_where_the_namespace_was() {
    // The removed statement's line is also where the imports the move
    // has to add would land, so both have to be one edit.
    let backend = Backend::new_test();

    let uri_decl = Url::parse("file:///src/Widget.php").unwrap();
    let uri_sibling = Url::parse("file:///src/Helper.php").unwrap();

    let text_decl = concat!(
        "<?php\n",
        "\n",
        "namespace App\\Old;\n",
        "\n",
        "class Widget {\n",
        "    public function helper(): Helper {\n",
        "        return new Helper();\n",
        "    }\n",
        "}\n",
    );
    let text_sibling = concat!(
        "<?php\n",
        "\n",
        "namespace App\\Old;\n",
        "\n",
        "class Helper {}\n",
    );

    open_file(&backend, &uri_decl, text_decl).await;
    open_file(&backend, &uri_sibling, text_sibling).await;

    let ws = backend
        .plan_class_move("App\\Old\\Widget", "Widget")
        .expect("the move should be planned")
        .expect("Expected a workspace edit for the class move");

    let result = apply_edits(text_decl, &edits_for_uri(&ws, &uri_decl));
    assert!(
        result.starts_with("<?php\n\nuse App\\Old\\Helper;\n\nclass Widget {\n"),
        "The sibling import should take the namespace statement's place; got:\n{result}"
    );
}

#[tokio::test]
async fn rename_class_move_into_global_namespace_refuses_a_brace_namespace() {
    // Removing a brace-style declaration means unwrapping the block it
    // opens, so the move says so rather than mangling the file.
    let backend = Backend::new_test();

    let uri = Url::parse("file:///src/Widget.php").unwrap();
    let text = concat!(
        "<?php\n",
        "\n",
        "namespace App\\Old {\n",
        "    class Widget {}\n",
        "}\n",
    );

    open_file(&backend, &uri, text).await;

    let error = backend
        .plan_class_move("App\\Old\\Widget", "Widget")
        .expect_err("a brace-style namespace should be refused");
    assert!(
        error.contains("brace block"),
        "The refusal should name the shape it cannot handle; got: {error}"
    );
}

#[tokio::test]
async fn rename_macro_registration_string_updates_call_sites() {
    let backend = Backend::new_test();
    let class_uri = Url::parse("file:///Widget.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let class_text = concat!("<?php\n", "namespace App\\Support;\n", "class Widget {}\n",);
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Widget;\n",
        "Widget::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Widget;\n",
        "function demo(Widget $widget): void {\n",
        "    Widget::shine();\n",
        "    $widget->shine();\n",
        "}\n",
    );

    open_file(&backend, &class_uri, class_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let (line, character) = line_char_of(provider_text, "shine");
    let edit = rename(&backend, &provider_uri, line, character, "glow").await;
    assert!(edit.is_some(), "expected macro rename edit");
    let edit = edit.unwrap();

    let provider_result = apply_edits(provider_text, &edits_for_uri(&edit, &provider_uri));
    assert!(provider_result.contains("macro('glow'"));

    let caller_result = apply_edits(caller_text, &edits_for_uri(&edit, &caller_uri));
    assert!(caller_result.contains("Widget::glow();"), "{caller_result}");
    assert!(
        caller_result.contains("$widget->glow();"),
        "{caller_result}"
    );
}

#[tokio::test]
async fn rename_macro_call_site_updates_registration_string() {
    let backend = Backend::new_test();
    let class_uri = Url::parse("file:///Widget.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let class_text = concat!("<?php\n", "namespace App\\Support;\n", "class Widget {}\n",);
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Widget;\n",
        "Widget::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Widget;\n",
        "function demo(Widget $widget): void {\n",
        "    Widget::shine();\n",
        "    $widget->shine();\n",
        "}\n",
    );

    open_file(&backend, &class_uri, class_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let edit = rename(&backend, &caller_uri, 4, 14, "glow").await;
    assert!(edit.is_some(), "expected macro rename edit");
    let edit = edit.unwrap();

    let provider_result = apply_edits(provider_text, &edits_for_uri(&edit, &provider_uri));
    assert!(
        provider_result.contains("macro('glow'"),
        "{provider_result}"
    );

    let caller_result = apply_edits(caller_text, &edits_for_uri(&edit, &caller_uri));
    assert!(caller_result.contains("Widget::glow();"), "{caller_result}");
    assert!(
        caller_result.contains("$widget->glow();"),
        "{caller_result}"
    );
}

#[tokio::test]
async fn rename_macro_from_descendant_call_updates_ancestor_and_sibling_calls() {
    let backend = Backend::new_test();
    let base_uri = Url::parse("file:///BaseCollection.php").unwrap();
    let child_uri = Url::parse("file:///EloquentCollection.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let base_text = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class BaseCollection {}\n",
    );
    let child_text = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class EloquentCollection extends BaseCollection {}\n",
    );
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\BaseCollection;\n",
        "BaseCollection::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\BaseCollection;\n",
        "use App\\Support\\EloquentCollection;\n",
        "function demo(BaseCollection $base, EloquentCollection $eloquent): void {\n",
        "    $base->shine();\n",
        "    $eloquent->shine();\n",
        "}\n",
    );

    open_file(&backend, &base_uri, base_text).await;
    open_file(&backend, &child_uri, child_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let edit = rename(&backend, &caller_uri, 6, 16, "glow").await;
    assert!(edit.is_some(), "expected descendant macro rename edit");
    let edit = edit.unwrap();

    let provider_result = apply_edits(provider_text, &edits_for_uri(&edit, &provider_uri));
    assert!(
        provider_result.contains("macro('glow'"),
        "{provider_result}"
    );

    let caller_result = apply_edits(caller_text, &edits_for_uri(&edit, &caller_uri));
    assert!(caller_result.contains("$base->glow();"), "{caller_result}");
    assert!(
        caller_result.contains("$eloquent->glow();"),
        "{caller_result}"
    );
}

#[tokio::test]
async fn rename_macro_registration_string_updates_unresolved_chain_call() {
    let backend = Backend::new_test();
    let base_uri = Url::parse("file:///BaseCollection.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let base_text = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class BaseCollection {}\n",
    );
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\BaseCollection;\n",
        "BaseCollection::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "function demo($query): void {\n",
        "    $query->pluck('name', 'id')->shine();\n",
        "}\n",
    );

    open_file(&backend, &base_uri, base_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let (line, character) = line_char_of(provider_text, "shine");
    let edit = rename(&backend, &provider_uri, line, character, "glow").await;
    assert!(edit.is_some(), "expected macro rename edit");
    let edit = edit.unwrap();

    let caller_result = apply_edits(caller_text, &edits_for_uri(&edit, &caller_uri));
    assert!(
        caller_result.contains("$query->pluck('name', 'id')->glow();"),
        "{caller_result}"
    );
}

#[tokio::test]
async fn rename_macro_chain_call_updates_registration_string() {
    let backend = Backend::new_test();
    let base_uri = Url::parse("file:///BaseCollection.php").unwrap();
    let provider_uri = Url::parse("file:///Provider.php").unwrap();
    let caller_uri = Url::parse("file:///Caller.php").unwrap();

    let base_text = concat!(
        "<?php\n",
        "namespace App\\Support;\n",
        "class BaseCollection {}\n",
    );
    let provider_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\BaseCollection;\n",
        "BaseCollection::macro('shine', function (): string { return 'ok'; });\n",
    );
    let caller_text = concat!(
        "<?php\n",
        "namespace App;\n",
        "function demo($query): void {\n",
        "    $query->pluck('name', 'id')->shine();\n",
        "}\n",
    );

    open_file(&backend, &base_uri, base_text).await;
    open_file(&backend, &provider_uri, provider_text).await;
    open_file(&backend, &caller_uri, caller_text).await;
    seed_macro_index(&backend, &provider_uri, provider_text);

    let edit = rename(&backend, &caller_uri, 3, 33, "glow").await;
    assert!(edit.is_some(), "expected macro chain rename edit");
    let edit = edit.unwrap();

    let provider_result = apply_edits(provider_text, &edits_for_uri(&edit, &provider_uri));
    assert!(
        provider_result.contains("macro('glow'"),
        "{provider_result}"
    );

    let caller_result = apply_edits(caller_text, &edits_for_uri(&edit, &caller_uri));
    assert!(
        caller_result.contains("$query->pluck('name', 'id')->glow();"),
        "{caller_result}"
    );
}
