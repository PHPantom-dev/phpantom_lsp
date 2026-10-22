//! Settings the editor forwards instead of `.phpantom.toml`: the file
//! filters a client passes in `initializationOptions` and pushes again
//! through `workspace/didChangeConfiguration`.

use crate::common::initialize_with_options;
use phpantom_lsp::Backend;
use std::fs;
use std::path::Path;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// A workspace holding a class under `generated/` and one in a `.module`
/// file, which the default filters index and skip respectively.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in [
        ("generated/Hidden.php", "<?php\nclass Hidden {}\n"),
        ("src/a.module", "<?php\nclass Module {}\n"),
    ] {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

fn is_indexed(backend: &Backend, fqn: &str) -> bool {
    backend.fqn_uri_index().read().contains_key(fqn)
}

/// Write a class file and report it the way the client's watcher would.
async fn create_on_disk(backend: &Backend, path: &Path, class: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("<?php\nclass {class} {{}}\n")).unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Url::from_file_path(path).unwrap(),
                typ: FileChangeType::CREATED,
            }],
        })
        .await;
}

async fn push_settings(backend: &Backend, settings: serde_json::Value) {
    backend
        .did_change_configuration(DidChangeConfigurationParams { settings })
        .await;
}

/// The filters have to be live before anything scans, so the first
/// discovery pass already honours them instead of indexing excluded
/// trees and dropping them afterwards.
#[tokio::test]
async fn initialize_applies_client_supplied_file_filters() {
    let dir = workspace();
    let backend = Backend::new_test();
    initialize_with_options(
        &backend,
        dir.path(),
        Some(serde_json::json!({
            "indexing": { "exclude": ["generated"], "extensions": ["module"] }
        })),
    )
    .await;

    assert!(
        !is_indexed(&backend, "Hidden"),
        "an excluded tree is not walked"
    );
    assert!(
        is_indexed(&backend, "Module"),
        "an extra extension is PHP source"
    );
}

/// Most clients send nothing, and one that does may send a shape meant
/// for something else entirely. Neither may switch filtering on.
#[tokio::test]
async fn initialize_without_client_filters_leaves_discovery_unfiltered() {
    for options in [None, Some(serde_json::json!({ "unrelated": true }))] {
        let dir = workspace();
        let backend = Backend::new_test();
        initialize_with_options(&backend, dir.path(), options.clone()).await;

        assert!(is_indexed(&backend, "Hidden"), "for {options:?}");
        assert!(!is_indexed(&backend, "Module"), "for {options:?}");
    }
}

/// Editing the editor's own settings mid-session has to take effect
/// without a restart, the same way a live `.phpantom.toml` edit does.
#[tokio::test]
async fn did_change_configuration_recompiles_the_filters() {
    let dir = workspace();
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;

    push_settings(
        &backend,
        serde_json::json!({ "phpantom": { "indexing": { "exclude": ["generated"] } } }),
    )
    .await;
    let late = dir.path().join("generated/Late.php");
    create_on_disk(&backend, &late, "Late").await;
    assert!(
        !is_indexed(&backend, "Late"),
        "a file created under a newly excluded folder is not indexed"
    );

    // Removing the entry again has to restore the unfiltered walk, not
    // merely stop adding to the exclude list.
    push_settings(
        &backend,
        serde_json::json!({ "phpantom": { "indexing": {} } }),
    )
    .await;
    create_on_disk(&backend, &late, "Late").await;
    assert!(
        is_indexed(&backend, "Late"),
        "the folder is indexed again once the exclude is gone"
    );
}

/// Recompiling the filters only governs the next scan. The notification
/// also has to reconcile the index that was built under the old ones, or
/// a class in a folder the user just hid keeps answering completion and
/// workspace symbol search until restart.
#[tokio::test]
async fn did_change_configuration_evicts_newly_excluded_classes() {
    let dir = workspace();
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    assert!(is_indexed(&backend, "Hidden"));

    push_settings(
        &backend,
        serde_json::json!({ "phpantom": { "indexing": { "exclude": ["generated"] } } }),
    )
    .await;

    assert!(
        !is_indexed(&backend, "Hidden"),
        "a class under a newly excluded folder must leave the index"
    );
}

/// VS Code's client syncs the whole `phpantom` settings section on every
/// change to any key in it. That notification says nothing about file
/// filters, so it must leave the ones the extension forwarded at startup
/// alone rather than reading as "cleared".
#[tokio::test]
async fn a_settings_push_without_filters_leaves_them_in_force() {
    let dir = workspace();
    let backend = Backend::new_test();
    initialize_with_options(
        &backend,
        dir.path(),
        Some(serde_json::json!({ "indexing": { "exclude": ["generated"] } })),
    )
    .await;

    push_settings(
        &backend,
        serde_json::json!({ "phpantom": { "trace": { "server": "verbose" } } }),
    )
    .await;
    create_on_disk(&backend, &dir.path().join("generated/Late.php"), "Late").await;

    assert!(
        !is_indexed(&backend, "Late"),
        "an unrelated settings push must not discard the client's filters"
    );
}
