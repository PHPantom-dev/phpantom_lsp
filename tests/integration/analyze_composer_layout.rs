//! The `analyze` command indexes the workspace the way the language server
//! does, so a Composer project kept in a subdirectory of the root (a
//! polyglot repository with `composer.json` under `php/`) has its vendor
//! classes resolved on the command line too.

use std::fs;
use std::path::Path;

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// A workspace with no root `composer.json` and a Composer project in
/// `php/` that calls `$call` on a class from its installed vendor.
fn polyglot_workspace(call: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "Cargo.toml", "[package]\n");
    // A git checkout that ignores the installed vendor, as one does: a walk
    // of the tree alone never reaches the vendor classes.
    fs::create_dir_all(root.join(".git")).unwrap();
    write(root, "php/.gitignore", "vendor/\n");
    write(
        root,
        "php/composer.json",
        r#"{ "require": { "acme/lib": "*" }, "autoload": { "psr-4": { "App\\": "src/" } } }"#,
    );
    write(
        root,
        "php/vendor/composer/installed.json",
        r#"{ "packages": [ {
            "name": "acme/lib",
            "install-path": "../acme/lib",
            "autoload": { "psr-4": { "Acme\\Lib\\": "src/" } }
        } ] }"#,
    );
    write(
        root,
        "php/vendor/acme/lib/src/Client.php",
        "<?php\nnamespace Acme\\Lib;\nclass Client { public function post(): int { return 1; } }\n",
    );
    write(
        root,
        "php/src/Service.php",
        &format!(
            "<?php\nnamespace App;\nuse Acme\\Lib\\Client;\nclass Service {{\n    public function go(Client $c): int {{ return $c->{call}(); }}\n}}\n"
        ),
    );
    dir
}

async fn analyze(root: &Path) -> i32 {
    phpantom_lsp::analyse::run(phpantom_lsp::analyse::AnalyseOptions {
        workspace_root: root.to_path_buf(),
        path_filters: Vec::new(),
        severity_filter: phpantom_lsp::analyse::SeverityFilter::All,
        use_colour: false,
        output_format: phpantom_lsp::analyse::OutputFormat::Json,
        // Suppresses the progress bar, which has no terminal to draw on here.
        debug: true,
        verbosity: 0,
        // Never the machine's own; the test asserts on the project alone.
        global_config: None,
    })
    .await
}

#[tokio::test]
async fn a_composer_project_in_a_subdirectory_resolves_its_vendor_classes() {
    let dir = polyglot_workspace("post");
    assert_eq!(
        analyze(dir.path()).await,
        0,
        "Acme\\Lib\\Client comes from php/vendor and must resolve"
    );
}

#[tokio::test]
async fn a_composer_project_in_a_subdirectory_still_reports_real_errors() {
    let dir = polyglot_workspace("missing");
    assert_eq!(analyze(dir.path()).await, 1);
}
