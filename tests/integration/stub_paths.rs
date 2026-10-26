use crate::common::{goto_definition_at, open_php};
use std::fs;

use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const CALLER: &str = r#"<?php

function main(): void
{
    bundled_helper();
    project_helper();
}
"#;

/// The editor's stub directory and a `[stubs]` path both resolve, a stub
/// file is where go-to-definition lands, and neither is diagnosed.
#[tokio::test]
async fn editor_and_config_stub_paths_resolve_and_navigate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("stubs")).unwrap();
    fs::write(
        root.join("stubs/project.php"),
        "<?php\nfunction project_helper(): void {}\n",
    )
    .unwrap();
    fs::write(
        root.join(".phpantom.toml"),
        "[stubs]\npaths = [\"stubs\"]\n",
    )
    .unwrap();
    fs::write(root.join("main.php"), CALLER).unwrap();

    let bundled = tempfile::tempdir().unwrap();
    fs::write(
        bundled.path().join("bundled.php"),
        "<?php\nfunction bundled_helper(): void {}\n",
    )
    .unwrap();

    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());
    backend
        .initialize(InitializeParams {
            initialization_options: Some(serde_json::json!({
                "stubs": { "path": bundled.path() }
            })),
            ..InitializeParams::default()
        })
        .await
        .unwrap();
    backend.initialized(InitializedParams {}).await;

    let uri = Uri::from_file_path(root.join("main.php")).unwrap();
    open_php(&backend, &uri, CALLER).await;

    let mut diags = Vec::new();
    backend.collect_unknown_function_diagnostics(uri.as_str(), CALLER, &mut diags);
    assert!(diags.is_empty(), "both helpers are declared: {diags:?}");

    for (line, file) in [
        (4, bundled.path().join("bundled.php")),
        (5, root.join("stubs/project.php")),
    ] {
        let target = match goto_definition_at(&backend, &uri, line, 6).await {
            Some(DefinitionResponse::Definition(Definition::Location(location))) => location.uri,
            other => panic!("expected one location on line {line}, got {other:?}"),
        };
        assert_eq!(target, Uri::from_file_path(&file).unwrap());
    }
}

/// A WordPress theme with its own `composer.json`, opened inside the
/// WordPress install it belongs to, gets core's and WooCommerce's
/// declarations from the stub packages it installed, though neither
/// package declares a Composer autoload.
#[tokio::test]
async fn a_theme_subproject_resolves_through_its_composer_stub_packages() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let theme = root.join("wp-content/themes/sage");
    let write = |path: &std::path::Path, content: &str| {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    };
    write(
        &theme.join("composer.json"),
        r#"{ "autoload": { "psr-4": { "App\\": "app/" } } }"#,
    );
    let wordpress = theme.join("vendor/php-stubs/wordpress-stubs/wordpress-stubs.php");
    write(
        &wordpress,
        "<?php\n\nnamespace {\n    class WP_Query {}\n    function add_action($hook_name, $callback) {}\n}\n",
    );
    write(
        &theme.join("vendor/php-stubs/woocommerce-stubs/woocommerce-stubs.php"),
        "<?php\n\nnamespace {\n    function wc_get_product($the_product = false) {}\n}\n",
    );
    write(
        &theme.join("vendor/composer/installed.json"),
        r#"{ "packages": [
            { "name": "php-stubs/woocommerce-stubs", "install-path": "../php-stubs/woocommerce-stubs" },
            { "name": "php-stubs/wordpress-stubs", "install-path": "../php-stubs/wordpress-stubs" }
        ] }"#,
    );
    let source = "<?php\n\nnamespace App;\n\nadd_action('init', fn () => wc_get_product(1));\nnew \\WP_Query();\n";
    let file = theme.join("app/setup.php");
    write(&file, source);

    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());
    backend.initialized(InitializedParams {}).await;
    let uri = Uri::from_file_path(&file).unwrap();
    open_php(&backend, &uri, source).await;

    let mut diags = Vec::new();
    backend.collect_unknown_function_diagnostics(uri.as_str(), source, &mut diags);
    backend.collect_unknown_class_diagnostics(uri.as_str(), source, &mut diags);
    assert!(
        diags.is_empty(),
        "the stub packages declare all three: {diags:?}"
    );

    let target = match goto_definition_at(&backend, &uri, 4, 2).await {
        Some(DefinitionResponse::Definition(Definition::Location(location))) => location.uri,
        other => panic!("expected one location, got {other:?}"),
    };
    assert_eq!(
        target,
        Uri::from_file_path(wordpress.canonicalize().unwrap()).unwrap()
    );
}
