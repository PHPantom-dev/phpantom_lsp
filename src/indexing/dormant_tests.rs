use std::path::Path;

use crate::Backend;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A project that installs `drupal/core` into `web/core` the way
/// `composer/installers` does, and links a module of its own in through a
/// path repository.
fn write_project(root: &Path) {
    write(
        &root.join("composer.json"),
        r#"{
            "require": { "drupal/core": "*", "acme/own": "*" },
            "autoload": { "psr-4": { "App\\": "src/" } }
        }"#,
    );
    write(
        &root.join("vendor/composer/installed.json"),
        r#"{ "packages": [
            {
                "name": "drupal/core",
                "type": "drupal-core",
                "install-path": "../../web/core",
                "autoload": { "psr-4": { "Drupal\\Core\\": "lib/Drupal/Core" } }
            },
            {
                "name": "acme/own",
                "dist": { "type": "path" },
                "transport-options": { "symlink": true },
                "install-path": "../../modules/own",
                "autoload": { "psr-4": { "Acme\\Own\\": "src/" } }
            }
        ] }"#,
    );
    write(
        &root.join("web/core/lib/Drupal/Core/Widget.php"),
        "<?php\nnamespace Drupal\\Core;\nclass Widget {\n    public function render(): string { return ''; }\n}\n",
    );
    write(
        &root.join("web/core/lib/Drupal/Core/Gadget.php"),
        "<?php\nnamespace Drupal\\Core;\nclass Gadget {\n    public function show(Widget $widget): void { $widget->render(); }\n}\n",
    );
    write(
        &root.join("modules/own/src/Thing.php"),
        "<?php\nnamespace Acme\\Own;\nclass Thing {\n    public function show(\\Drupal\\Core\\Widget $widget): void { $widget->render(); }\n}\n",
    );
    write(
        &root.join("src/Page.php"),
        "<?php\nnamespace App;\nclass Page {\n    public function show(\\Drupal\\Core\\Widget $widget): void { $widget->render(); }\n}\n",
    );
}

async fn indexed(root: &Path) -> Backend {
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(root.to_path_buf());
    let package = crate::composer::read_composer_package(root);
    backend
        .discover_workspace_symbols(root, Default::default(), package, None)
        .await;
    backend.ensure_workspace_indexed();
    backend
}

fn uri(root: &Path, path: &str) -> String {
    crate::util::path_to_uri(&root.join(path))
}

#[tokio::test]
async fn an_installed_package_file_keeps_no_parse_once_indexed() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let backend = indexed(dir.path()).await;

    let widget = uri(dir.path(), "web/core/lib/Drupal/Core/Widget.php");
    assert!(backend.symbol_map_for(&widget).is_none());
    assert!(backend.dormant_files.read().contains(&widget));
    assert!(
        backend.find_or_load_class("Drupal\\Core\\Widget").is_some(),
        "its classes still resolve"
    );

    // The project's own code, and a package it links in from its own
    // tree, stay resident.
    for own in ["src/Page.php", "modules/own/src/Thing.php"] {
        assert!(
            backend.symbol_map_for(&uri(dir.path(), own)).is_some(),
            "{own}"
        );
    }
}

#[tokio::test]
async fn a_search_reads_a_dormant_file_again() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let backend = indexed(dir.path()).await;
    let gadget = uri(dir.path(), "web/core/lib/Drupal/Core/Gadget.php");
    assert!(backend.symbol_map_for(&gadget).is_none());

    let found: Vec<String> = backend
        .user_file_symbol_maps_for_methods(&["render"])
        .into_iter()
        .map(|(uri, _)| uri)
        .collect();

    assert!(found.contains(&gadget), "{found:?}");
    assert!(
        found.contains(&uri(dir.path(), "src/Page.php")),
        "{found:?}"
    );
    assert!(backend.symbol_map_for(&gadget).is_some());
    assert!(!backend.dormant_files.read().contains(&gadget));
}

#[tokio::test]
async fn a_refreshed_index_leaves_dormant_files_asleep() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let backend = indexed(dir.path()).await;

    backend.ensure_workspace_indexed();

    let widget = uri(dir.path(), "web/core/lib/Drupal/Core/Widget.php");
    assert!(backend.symbol_map_for(&widget).is_none());
    assert!(backend.dormant_files.read().contains(&widget));
}
