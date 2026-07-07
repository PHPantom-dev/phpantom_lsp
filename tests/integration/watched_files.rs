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

/// Mark a workspace file as already parsed, mirroring a lazy load via
/// `find_or_load_class` (which stores the canonical `file://` URI).
fn mark_loaded(backend: &Backend, path: &std::path::Path) {
    let uri = Url::from_file_path(path).unwrap().to_string();
    backend.parsed_uris().write().insert(uri);
}

#[tokio::test]
async fn class_removed_from_loaded_changed_file_is_no_longer_suggested() {
    // A file initially declaring two classes, which has been loaded
    // (parsed) at some point, so its details are cached.
    let both = "<?php\nnamespace App\\Models;\nclass KeepMe {}\nclass RemoveMe {}\n";
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &[("app/Models/Models.php", both)]);
    let path = dir.path().join("app/Models/Models.php");

    index_class(&backend, "App\\Models\\KeepMe", &path);
    index_class(&backend, "App\\Models\\RemoveMe", &path);
    mark_loaded(&backend, &path);

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
        "class removed from a loaded, changed file must be purged from the FQN index"
    );
}

#[tokio::test]
async fn change_to_unloaded_file_is_ignored() {
    // Editors re-report the whole workspace as "changed" when the window
    // regains focus.  A file we never loaded carries only a discovery-index
    // pointer; re-reading it from disk on every refocus is wasted work, so a
    // plain content change to an unloaded file is ignored.  Its existing
    // index entry is left untouched and is refreshed lazily on next access.
    let original = "<?php\nnamespace App\\Models;\nclass Untouched {}\n";
    let (backend, dir) =
        create_psr4_workspace(COMPOSER_JSON, &[("app/Models/Untouched.php", original)]);
    let path = dir.path().join("app/Models/Untouched.php");

    index_class(&backend, "App\\Models\\Untouched", &path);

    // The file is rewritten on disk, but it was never loaded.
    fs::write(&path, "<?php\nnamespace App\\Models;\nclass Renamed {}\n").unwrap();
    backend.did_change_watched_files(change_event(&path)).await;

    let idx = backend.fqn_uri_index().read();
    assert!(
        idx.contains_key("App\\Models\\Untouched"),
        "an unloaded file's discovery entry is kept and refreshed lazily, not on every refocus"
    );
    assert!(
        !idx.contains_key("App\\Models\\Renamed"),
        "an unloaded file is not re-scanned from disk on a content-change event"
    );
}

#[tokio::test]
async fn composer_change_purges_stale_vendor_functions() {
    // After a `composer update`, autoloaded functions/constants from the old
    // vendor tree must be removed from the indexes, not left to linger.
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &[]);
    let vendor_func = dir.path().join("vendor/old/pkg/functions.php");
    let outside_func = dir.path().join("app/helpers.php");

    // Seed a vendor-tree function and a non-vendor function.
    backend
        .autoload_function_index()
        .write()
        .insert("old_vendor_func".to_string(), vendor_func.clone());
    backend
        .autoload_function_index()
        .write()
        .insert("app_helper".to_string(), outside_func.clone());
    backend
        .autoload_constant_index()
        .write()
        .insert("OLD_VENDOR_CONST".to_string(), vendor_func.clone());

    // A `composer update` rewrites composer.json; the editor notifies us.
    backend
        .did_change_watched_files(change_event(&dir.path().join("composer.json")))
        .await;

    let fi = backend.autoload_function_index().read();
    assert!(
        !fi.contains_key("old_vendor_func"),
        "stale vendor function must be purged after composer change"
    );
    assert!(
        fi.contains_key("app_helper"),
        "non-vendor function must survive a composer change"
    );
    assert!(
        !backend
            .autoload_constant_index()
            .read()
            .contains_key("OLD_VENDOR_CONST"),
        "stale vendor constant must be purged after composer change"
    );
}

#[tokio::test]
async fn batch_of_mixed_changes_reindexes_in_one_notification() {
    // A flood of watched-file events (e.g. a branch switch or `composer
    // install`) arrives as a single notification carrying many changes of
    // different kinds.  Every change must be applied: created files indexed,
    // deleted files purged, and changed files refreshed.
    let stale = "<?php\nnamespace App\\Models;\nclass Stale {}\n";
    let edited = "<?php\nnamespace App\\Models;\nclass Edited {}\nclass DropMe {}\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("app/Models/Stale.php", stale),
            ("app/Models/Edited.php", edited),
        ],
    );

    let stale_path = dir.path().join("app/Models/Stale.php");
    let edited_path = dir.path().join("app/Models/Edited.php");
    let created_path = dir.path().join("app/Models/Fresh.php");

    index_class(&backend, "App\\Models\\Stale", &stale_path);
    index_class(&backend, "App\\Models\\Edited", &edited_path);
    index_class(&backend, "App\\Models\\DropMe", &edited_path);
    // The edited file has been loaded, so its content change is acted on.
    mark_loaded(&backend, &edited_path);

    // Apply the on-disk state the events describe.
    fs::remove_file(&stale_path).unwrap();
    fs::write(
        &edited_path,
        "<?php\nnamespace App\\Models;\nclass Edited {}\n",
    )
    .unwrap();
    fs::write(
        &created_path,
        "<?php\nnamespace App\\Models;\nclass Fresh {}\n",
    )
    .unwrap();

    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![
                FileEvent {
                    uri: Url::from_file_path(&stale_path).unwrap(),
                    typ: FileChangeType::DELETED,
                },
                FileEvent {
                    uri: Url::from_file_path(&edited_path).unwrap(),
                    typ: FileChangeType::CHANGED,
                },
                FileEvent {
                    uri: Url::from_file_path(&created_path).unwrap(),
                    typ: FileChangeType::CREATED,
                },
            ],
        })
        .await;

    let idx = backend.fqn_uri_index().read();
    assert!(
        !idx.contains_key("App\\Models\\Stale"),
        "deleted file's class must be purged"
    );
    assert!(
        idx.contains_key("App\\Models\\Edited"),
        "surviving class in a changed file must remain"
    );
    assert!(
        !idx.contains_key("App\\Models\\DropMe"),
        "class removed from a changed file must be purged"
    );
    assert!(
        idx.contains_key("App\\Models\\Fresh"),
        "newly created class must be indexed"
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
