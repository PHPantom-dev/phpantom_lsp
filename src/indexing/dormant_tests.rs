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

/// Core's plugin managers and plugin classes go to sleep like the rest of
/// core, and the plugin index still reads them.
#[tokio::test]
async fn dormant_plugin_classes_still_declare_their_plugins() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    let manager = "web/core/lib/Drupal/Core/Block/BlockManager.php";
    let plugin = "web/core/modules/system/src/Plugin/Block/SystemBrandingBlock.php";
    for (path, content) in [
        ("web/core/lib/Drupal.php", "<?php\nclass Drupal {}\n"),
        (
            "web/core/lib/Drupal/Component/Plugin/PluginManagerInterface.php",
            "<?php\nnamespace Drupal\\Component\\Plugin;\ninterface PluginManagerInterface {}\n",
        ),
        (
            "web/core/lib/Drupal/Component/Plugin/Attribute/AttributeInterface.php",
            "<?php\nnamespace Drupal\\Component\\Plugin\\Attribute;\ninterface AttributeInterface {}\n",
        ),
        (
            "web/core/lib/Drupal/Core/Plugin/DefaultPluginManager.php",
            "<?php\nnamespace Drupal\\Core\\Plugin;\nclass DefaultPluginManager implements \\Drupal\\Component\\Plugin\\PluginManagerInterface {}\n",
        ),
        (
            "web/core/lib/Drupal/Core/Block/Attribute/Block.php",
            "<?php\nnamespace Drupal\\Core\\Block\\Attribute;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nclass Block implements \\Drupal\\Component\\Plugin\\Attribute\\AttributeInterface {\n    public function __construct(public readonly string $id) {}\n}\n",
        ),
        (
            manager,
            "<?php\nnamespace Drupal\\Core\\Block;\nuse Drupal\\Core\\Block\\Attribute\\Block;\nuse Drupal\\Core\\Plugin\\DefaultPluginManager;\nclass BlockManager extends DefaultPluginManager {\n    public function __construct(\\Traversable $namespaces, $module_handler) {\n        parent::__construct('Plugin/Block', $namespaces, $module_handler, BlockPluginInterface::class, Block::class, 'Drupal\\Core\\Block\\Annotation\\Block');\n    }\n}\n",
        ),
        (
            plugin,
            "<?php\nnamespace Drupal\\system\\Plugin\\Block;\nuse Drupal\\Core\\Block\\Attribute\\Block;\n#[Block(id: 'system_branding_block')]\nclass SystemBrandingBlock {}\n",
        ),
    ] {
        write(&dir.path().join(path), content);
    }
    let backend = indexed(dir.path()).await;
    for path in [manager, plugin] {
        assert!(
            backend
                .dormant_files
                .read()
                .contains(&uri(dir.path(), path)),
            "{path}"
        );
    }

    assert_eq!(
        backend.drupal_plugin_ids("Drupal\\Core\\Block\\BlockManager"),
        ["system_branding_block"]
    );
}

/// What a test module's entity class declares outlives its parse: test code
/// sees the entity type before anything wakes the file.
#[tokio::test]
async fn a_dormant_test_module_still_declares_its_entity_types() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path());
    write(
        &dir.path().join("web/core/lib/Drupal.php"),
        "<?php\nclass Drupal {}\n",
    );
    let entity = "web/core/modules/config/tests/config_test/src/Entity/ConfigTest.php";
    write(
        &dir.path().join(entity),
        "<?php\nnamespace Drupal\\config_test\\Entity;\n/**\n * @ConfigEntityType(\n *   id = \"config_test\",\n * )\n */\nclass ConfigTest {}\n",
    );
    let backend = indexed(dir.path()).await;
    let entity = uri(dir.path(), entity);
    assert!(backend.dormant_files.read().contains(&entity));

    let test = uri(
        dir.path(),
        "web/core/modules/config/tests/src/Functional/ConfigTest.php",
    );
    let found = backend.drupal_entity_types_seen_from("config_test", Some(&test));
    assert_eq!(
        found.first().map(|(class, _)| class.as_str()),
        Some("Drupal\\config_test\\Entity\\ConfigTest"),
        "{found:?}"
    );
    assert!(
        backend
            .drupal_entity_types_seen_from("config_test", Some(&uri(dir.path(), "src/Page.php")))
            .is_empty()
    );
}
