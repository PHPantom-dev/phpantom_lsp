//! Tests for `workspace/didChangeWatchedFiles` handling: when PHP files
//! are created, changed, or deleted outside the editor (e.g. a git
//! checkout or `composer` run), the server must refresh its indexes so
//! completion, definition, etc. reflect the new state on disk.

use crate::common::create_psr4_workspace;
use phpantom_lsp::Backend;
use std::fs;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "name": "test/project",
    "autoload": { "psr-4": { "App\\": "app/" } }
}"#;

/// Open a consuming file and request class-name completion at a position.
async fn complete_at(
    backend: &Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
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
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    }
}

/// FQNs of completion items (stored in the `detail` field).
fn fqns(items: &[CompletionItem]) -> Vec<String> {
    items.iter().filter_map(|i| i.detail.clone()).collect()
}

/// Simulate the workspace scan by indexing a file's classes the same way
/// the server does (FQN → `file://` URI).
fn index_class(backend: &Backend, fqn: &str, path: &std::path::Path) {
    let uri = Url::from_file_path(path).unwrap().to_string();
    backend.fqn_uri_index().write().insert(fqn.to_string(), uri);
}

fn delete_event(path: &std::path::Path) -> DidChangeWatchedFilesParams {
    DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Url::from_file_path(path).unwrap(),
            typ: FileChangeType::DELETED,
        }],
    }
}

fn change_event(path: &std::path::Path) -> DidChangeWatchedFilesParams {
    DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Url::from_file_path(path).unwrap(),
            typ: FileChangeType::CHANGED,
        }],
    }
}

#[tokio::test]
async fn deleted_model_is_no_longer_suggested() {
    let model = "<?php\nnamespace App\\Models;\nclass OldModel {}\n";
    let (backend, dir) =
        create_psr4_workspace(COMPOSER_JSON, &[("app/Models/OldModel.php", model)]);
    let model_path = dir.path().join("app/Models/OldModel.php");

    // Simulate the workspace scan having indexed the model.
    index_class(&backend, "App\\Models\\OldModel", &model_path);

    let consumer = Url::parse("file:///app.php").unwrap();
    let items = complete_at(&backend, &consumer, "<?php\nnew OldMo\n", 1, 8).await;
    assert!(
        fqns(&items).contains(&"App\\Models\\OldModel".to_string()),
        "model should be suggested before deletion, got: {:?}",
        fqns(&items)
    );

    // The file is removed on disk (e.g. by a git checkout) and the editor
    // notifies the server.
    fs::remove_file(&model_path).unwrap();
    backend
        .did_change_watched_files(delete_event(&model_path))
        .await;

    assert!(
        !backend
            .fqn_uri_index()
            .read()
            .contains_key("App\\Models\\OldModel"),
        "deleted model should be purged from the FQN index"
    );

    let items_after = complete_at(&backend, &consumer, "<?php\nnew OldMo\n", 1, 8).await;
    assert!(
        !fqns(&items_after).contains(&"App\\Models\\OldModel".to_string()),
        "deleted model must not be suggested, got: {:?}",
        fqns(&items_after)
    );
}

#[tokio::test]
async fn class_removed_from_changed_file_is_no_longer_suggested() {
    // A file initially declaring two classes.
    let both = "<?php\nnamespace App\\Models;\nclass KeepMe {}\nclass RemoveMe {}\n";
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &[("app/Models/Models.php", both)]);
    let path = dir.path().join("app/Models/Models.php");

    index_class(&backend, "App\\Models\\KeepMe", &path);
    index_class(&backend, "App\\Models\\RemoveMe", &path);

    // The file is rewritten on disk to drop one class.
    fs::write(&path, "<?php\nnamespace App\\Models;\nclass KeepMe {}\n").unwrap();
    backend.did_change_watched_files(change_event(&path)).await;

    let idx = backend.fqn_uri_index().read();
    assert!(
        idx.contains_key("App\\Models\\KeepMe"),
        "surviving class should remain indexed"
    );
    assert!(
        !idx.contains_key("App\\Models\\RemoveMe"),
        "class removed from a changed file must be purged from the FQN index"
    );
}

#[tokio::test]
async fn created_file_becomes_suggestable() {
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &[]);
    let path = dir.path().join("app/Models/NewModel.php");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "<?php\nnamespace App\\Models;\nclass NewModel {}\n").unwrap();

    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Url::from_file_path(&path).unwrap(),
                typ: FileChangeType::CREATED,
            }],
        })
        .await;

    assert!(
        backend
            .fqn_uri_index()
            .read()
            .contains_key("App\\Models\\NewModel"),
        "newly created class should be indexed"
    );
}
