//! Regression tests for gating Laravel string-key hover, go-to-definition,
//! find-references, and rename on `is_laravel`.
//!
//! `SymbolKind::LaravelStringKey` spans are extracted by pure name match
//! (`config()`, `route()`, `view()`, `__()`, `trans()`, …) with no project
//! context, so a non-Laravel project that happens to declare its own
//! `config()` function (common in home-grown micro-frameworks) must not get
//! fabricated "Config key" hovers, go-to-definition jumps, or find-references
//! results against a `config/*.php` file that happens to sit on disk.

use crate::common::{
    completion_response, create_psr4_workspace, goto_definition_at, hover_response, open_php,
    position_after, references_response,
};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON_NON_LARAVEL: &str = r#"{
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const COMPOSER_JSON_LARAVEL: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const APP_CONFIG: &str = "\
<?php
return [
    'name' => 'Acme',
];
";

const FILESYSTEMS_CONFIG: &str = "\
<?php
return [
    'disks' => [
        'archive' => ['driver' => 'local'],
    ],
];
";

const CONSUMER: &str = "\
<?php
namespace App;

class Settings {
    public function demo(): void {
        config('app.name');
        config('app.name');
    }
}
";

const STORAGE_HOMONYM_CONSUMER: &str = "\
<?php
namespace App;

final class Storage {
    public static function disk(string $name): void {}
}

Storage::disk('archive');
";

async fn setup(composer_json: &str) -> (phpantom_lsp::Backend, tempfile::TempDir, Url) {
    let (backend, dir) = create_psr4_workspace(
        composer_json,
        &[
            ("config/app.php", APP_CONFIG),
            ("src/Settings.php", CONSUMER),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Settings.php")).unwrap();
    open_php(&backend, &uri, CONSUMER).await;

    (backend, dir, uri)
}

async fn setup_storage_homonym() -> (phpantom_lsp::Backend, tempfile::TempDir, Url) {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON_NON_LARAVEL,
        &[
            ("config/filesystems.php", FILESYSTEMS_CONFIG),
            ("src/StorageConsumer.php", STORAGE_HOMONYM_CONSUMER),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/StorageConsumer.php")).unwrap();
    open_php(&backend, &uri, STORAGE_HOMONYM_CONSUMER).await;

    (backend, dir, uri)
}

/// Lands inside the `'app.name'` string literal on the first `config()`
/// call (line 5, 0-based) in `CONSUMER`.
const KEY_POSITION: Position = Position {
    line: 5,
    character: 20,
};

#[tokio::test]
async fn non_laravel_direct_resource_homonym_has_no_laravel_editor_features() {
    let (backend, _dir, uri) = setup_storage_homonym().await;
    let position = position_after(STORAGE_HOMONYM_CONSUMER, "Storage::disk('arch");

    let completion =
        completion_response(&backend, &uri, position.line, position.character, None).await;
    let completion_items = match completion {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    assert!(
        completion_items.iter().all(|item| item.label != "archive"),
        "a non-Laravel Storage homonym must not complete configured disks, got {completion_items:?}"
    );

    let hover = hover_response(&backend, &uri, position.line, position.character).await;
    assert!(
        hover.is_none(),
        "a non-Laravel Storage homonym must not hover as a storage disk, got {hover:?}"
    );

    let definition = goto_definition_at(&backend, &uri, position.line, position.character).await;
    assert!(
        definition.is_none(),
        "a non-Laravel Storage homonym must not jump to filesystems.php, got {definition:?}"
    );

    let references = references_response(&backend, &uri, position, true)
        .await
        .unwrap_or_default();
    assert!(
        references.is_empty(),
        "a non-Laravel Storage homonym must not fabricate disk references, got {references:?}"
    );
}

#[tokio::test]
async fn non_laravel_project_hover_does_not_fabricate_config_key() {
    let (backend, _dir, uri) = setup(COMPOSER_JSON_NON_LARAVEL).await;

    let hover = hover_response(&backend, &uri, KEY_POSITION.line, KEY_POSITION.character).await;

    assert!(
        hover.is_none(),
        "a non-Laravel project's own config() must not hover as a config key, got {hover:?}"
    );
}

#[tokio::test]
async fn laravel_project_hover_still_shows_config_key() {
    let (backend, _dir, uri) = setup(COMPOSER_JSON_LARAVEL).await;

    let hover = hover_response(&backend, &uri, KEY_POSITION.line, KEY_POSITION.character).await;

    assert!(
        hover.is_some(),
        "a real Laravel project should still hover config('app.name')"
    );
}

#[tokio::test]
async fn non_laravel_project_definition_does_not_jump_to_config_file() {
    let (backend, _dir, uri) = setup(COMPOSER_JSON_NON_LARAVEL).await;

    let result =
        goto_definition_at(&backend, &uri, KEY_POSITION.line, KEY_POSITION.character).await;

    assert!(
        result.is_none(),
        "a non-Laravel project's own config() must not jump to config/app.php, got {result:?}"
    );
}

#[tokio::test]
async fn non_laravel_project_references_does_not_fabricate_config_key_refs() {
    let (backend, _dir, uri) = setup(COMPOSER_JSON_NON_LARAVEL).await;

    let result = references_response(&backend, &uri, KEY_POSITION, true)
        .await
        .unwrap_or_default();

    assert!(
        result.is_empty(),
        "a non-Laravel project's own config() must not produce Laravel string-key \
         references, got {result:?}"
    );
}

#[tokio::test]
async fn laravel_project_references_still_find_config_keys() {
    let (backend, _dir, uri) = setup(COMPOSER_JSON_LARAVEL).await;

    let result = references_response(&backend, &uri, KEY_POSITION, true)
        .await
        .unwrap_or_default();

    assert!(
        result.len() >= 2,
        "a real Laravel project should find both config('app.name') calls, got {result:?}"
    );
}

#[tokio::test]
async fn non_laravel_project_prepare_rename_rejects_config_key() {
    let (backend, _dir, uri) = setup(COMPOSER_JSON_NON_LARAVEL).await;

    let result = backend
        .prepare_rename(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: KEY_POSITION,
        })
        .await
        .unwrap();

    assert!(
        result.is_none(),
        "a non-Laravel project's own config() must not be renameable as a config key, \
         got {result:?}"
    );
}

#[tokio::test]
async fn non_laravel_project_rename_does_not_rewrite_config_key() {
    let (backend, _dir, uri) = setup(COMPOSER_JSON_NON_LARAVEL).await;

    let result = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: KEY_POSITION,
            },
            new_name: "renamed.key".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap();

    assert!(
        result.is_none(),
        "a non-Laravel project's own config() must not be renamed as a config key, \
         got {result:?}"
    );
}
