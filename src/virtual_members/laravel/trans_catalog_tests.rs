use super::*;
use crate::test_fixtures::make_backend;
use tower_lsp::lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent};

#[test]
fn translation_catalog_merges_roots_locales_groups_and_providers() {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    for directory in [
        "lang/en",
        "resources/lang/fr",
        "lang/es",
        "lang/vendor",
        "package/de",
    ] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    for (path, content) in [
        (
            "lang/en/messages.php",
            "<?php return ['hello' => 'Hello', 'group' => ['child' => 'yes']];",
        ),
        (
            "resources/lang/fr/messages.php",
            "<?php return ['hello' => 'Bonjour', 'group' => 'Groupe'];",
        ),
        ("lang/en.json", r#"{"Hello":"first", "Hello":"last"}"#),
        ("resources/lang/it.json", r#"{"Hello":"Ciao"}"#),
        ("lang/en/ignore.txt", "ignore"),
        ("lang/vendor/ignored.php", "<?php return ['key'=>'no'];"),
        ("package/de/mail.php", "<?php return ['sent'=>'Gesendet'];"),
        ("package/ignored.json", r#"{"ignored":"ignored"}"#),
    ] {
        std::fs::write(dir.path().join(path), content).unwrap();
    }
    backend
        .laravel_provider_resources
        .write()
        .trans_dirs
        .push(ProviderResource {
            path: dir.path().join("package"),
            namespace: "shop".to_string(),
        });
    let catalog = backend.cached_translations();
    assert_eq!(
        catalog
            .locales
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["de", "en", "es", "fr", "it"]
    );
    assert_eq!(catalog.entries["Hello"][0].value.as_deref(), Some("last"));
    assert_eq!(catalog.entries["messages.hello"].len(), 2);
    assert_eq!(
        catalog.entries["shop::mail.sent"][0].value.as_deref(),
        Some("Gesendet")
    );
    assert!(!catalog.entries.contains_key("ignored"));
    assert_eq!(backend.translation_definitions("messages").len(), 2);
    assert!(
        backend
            .translation_definitions("messages.missing")
            .is_empty()
    );
    assert_eq!(
        backend.resolve_trans_type("messages.hello").unwrap(),
        crate::php_type::PhpType::string()
    );
    assert_ne!(
        backend.resolve_trans_type("messages.group").unwrap(),
        crate::php_type::PhpType::string()
    );
    assert_eq!(backend.cached_trans_keys().len(), catalog.entries.len());
    assert!(Arc::ptr_eq(&catalog, &backend.cached_translations()));
    assert!(
        catalog.contains_uri(
            Url::from_file_path(dir.path().join("package/de/mail.php"))
                .unwrap()
                .as_str()
        )
    );
}

#[test]
fn translation_catalog_refreshes_buffers_close_and_watched_files() {
    let backend = make_backend();
    backend.resolved_class_cache.write().set_laravel(true);
    let dir = tempfile::tempdir().unwrap();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    std::fs::create_dir_all(dir.path().join("resources/lang/en")).unwrap();
    let path = dir.path().join("resources/lang/en/messages.php");
    let uri = Url::from_file_path(&path).unwrap();
    std::fs::write(&path, "<?php return ['key'=>'disk'];").unwrap();
    let catalog = backend.cached_translations();
    backend
        .laravel_string_key_cache
        .write()
        .invalidate_for_uri("file:///project/other.php", "<?php");
    assert!(Arc::ptr_eq(&catalog, &backend.cached_translations()));
    backend.open_files.write().insert(
        uri.to_string(),
        Arc::new("<?php return ['key'=>'buffer'];".to_string()),
    );
    backend
        .laravel_string_key_cache
        .write()
        .invalidate_for_uri(uri.as_str(), "");
    assert_eq!(
        backend.cached_translations().entries["messages.key"][0]
            .value
            .as_deref(),
        Some("buffer")
    );
    backend.open_files.write().remove(uri.as_str());
    backend.clear_file_maps(uri.as_str());
    assert_eq!(
        backend.cached_translations().entries["messages.key"][0]
            .value
            .as_deref(),
        Some("disk")
    );
    std::fs::write(&path, "<?php return ['new'=>'new'];").unwrap();
    assert!(backend.apply_watched_file_changes(
        &DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri,
                typ: FileChangeType::CHANGED
            }],
        },
        dir.path()
    ));
    assert!(
        backend
            .cached_translations()
            .entries
            .contains_key("messages.new")
    );
    let json = dir.path().join("resources/lang/fr.json");
    std::fs::write(&json, r#"{"Bonjour":"Salut"}"#).unwrap();
    assert!(backend.apply_watched_file_changes(
        &DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Url::from_file_path(json).unwrap(),
                typ: FileChangeType::CREATED
            }],
        },
        dir.path()
    ));
    assert!(
        backend
            .cached_translations()
            .entries
            .contains_key("Bonjour")
    );
}

#[test]
fn translation_catalog_handles_missing_and_unreadable_files() {
    let backend = make_backend();
    assert!(backend.cached_translations().entries.is_empty());
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("lang/en/bad.php")).unwrap();
    std::fs::write(dir.path().join("lang/en/invalid.php"), [0xff]).unwrap();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    backend.laravel_string_key_cache.write().translations = None;
    assert!(backend.cached_translations().entries.is_empty());
}
