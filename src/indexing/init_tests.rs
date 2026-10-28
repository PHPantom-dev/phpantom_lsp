use std::path::Path;

use crate::{Backend, ClassCompletionOrigin};

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A subproject that requires `acme/lib`, installed with one class per
/// name in `classes`, alongside a path-repository module symlinked into
/// its vendor directory.
fn write_subproject(sub: &Path, classes: &[&str]) {
    write(
        &sub.join("composer.json"),
        r#"{
            "require": { "acme/lib": "*" },
            "autoload": { "psr-4": { "App\\": "src/" } }
        }"#,
    );
    write(
        &sub.join("vendor/composer/installed.json"),
        r#"{ "packages": [
            {
                "name": "acme/lib",
                "install-path": "../acme/lib",
                "autoload": { "psr-4": { "Acme\\Lib\\": "src/" } }
            },
            {
                "name": "demo/common",
                "dist": { "type": "path" },
                "transport-options": { "symlink": true },
                "install-path": "../../modules/common",
                "autoload": { "psr-4": { "Demo\\Common\\": "src/" } }
            }
        ] }"#,
    );
    let lib_src = sub.join("vendor/acme/lib/src");
    if lib_src.exists() {
        std::fs::remove_dir_all(&lib_src).unwrap();
    }
    for class in classes {
        write(
            &lib_src.join(format!("{class}.php")),
            &format!("<?php\nnamespace Acme\\Lib;\nclass {class} {{}}\n"),
        );
    }
    write(
        &sub.join("modules/common/src/Thing.php"),
        "<?php\nnamespace Demo\\Common;\nclass Thing {}\n",
    );
}

async fn index_workspace(root: &Path) -> Backend {
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(root.to_path_buf());
    backend
        .discover_workspace_symbols(root, Default::default(), None, None)
        .await;
    backend
}

fn mapping_base(backend: &Backend, prefix: &str) -> Option<String> {
    backend
        .workspace
        .psr4_mappings
        .read()
        .iter()
        .find(|m| m.prefix == prefix)
        .map(|m| m.base_path.clone())
}

#[tokio::test]
async fn monorepo_subprojects_register_path_repository_mappings() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("app");
    write_subproject(&sub, &["Client"]);

    let backend = index_workspace(dir.path()).await;

    let module = mapping_base(&backend, "Demo\\Common\\")
        .expect("the path-repository module's namespace must be mapped");
    assert!(
        module
            .trim_end_matches('/')
            .ends_with("app/modules/common/src"),
        "got {module}"
    );
    let own = mapping_base(&backend, "App\\").expect("the subproject's own namespace");
    assert!(Path::new(&own).is_absolute(), "got {own}");
}

#[tokio::test]
async fn monorepo_subproject_classmap_classes_rank_as_vendor_code() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("app");
    write_subproject(&sub, &["Client"]);
    write(
        &sub.join("vendor/composer/autoload_classmap.php"),
        "<?php\n$vendorDir = dirname(__DIR__);\nreturn array(\n    'Acme\\\\Lib\\\\Client' => $vendorDir . '/acme/lib/src/Client.php',\n);\n",
    );

    let backend = index_workspace(dir.path()).await;

    assert_eq!(
        backend
            .symbols
            .fqn_origin_index
            .read()
            .get("Acme\\Lib\\Client")
            .copied(),
        Some(ClassCompletionOrigin::VendorExplicit)
    );
}

#[tokio::test]
async fn a_composer_update_in_one_subproject_rescans_only_that_subproject() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    let other = dir.path().join("other");
    write_subproject(&app, &["Old"]);
    write(
        &other.join("composer.json"),
        r#"{ "require": { "other/pkg": "*" } }"#,
    );
    write(
        &other.join("vendor/composer/installed.json"),
        r#"{ "packages": [ {
            "name": "other/pkg",
            "install-path": "../other/pkg",
            "autoload": { "psr-4": { "Other\\Pkg\\": "src/" } }
        } ] }"#,
    );
    write(
        &other.join("vendor/other/pkg/src/Widget.php"),
        "<?php\nnamespace Other\\Pkg;\nclass Widget {}\n",
    );

    let backend = index_workspace(dir.path()).await;
    assert!(
        backend
            .symbols
            .fqn_uri_index
            .read()
            .get("Acme\\Lib\\Old")
            .is_some()
    );

    write_subproject(&app, &["New"]);
    backend.rescan_composer_indexes(dir.path(), &[app.join("composer.lock")]);

    let index = backend.symbols.fqn_uri_index.read();
    assert!(index.get("Acme\\Lib\\New").is_some());
    assert!(
        index.get("Acme\\Lib\\Old").is_none(),
        "a class the update removed must stop resolving"
    );
    assert!(index.get("Other\\Pkg\\Widget").is_some());
    drop(index);

    let roots = backend.workspace.vendor_package_origin_roots.read();
    for name in ["acme/lib", "other/pkg"] {
        assert!(
            roots.iter().any(|(_, _, pkg)| pkg == name),
            "{name} must keep its package root"
        );
    }
    drop(roots);
    assert!(mapping_base(&backend, "Demo\\Common\\").is_some());
}

/// Index `root` as a workspace with a `composer.json` of its own.
async fn index_root_project(root: &Path) -> Backend {
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(root.to_path_buf());
    let package = crate::composer::read_composer_package(root);
    assert!(package.is_some(), "the root needs a composer.json");
    backend
        .discover_workspace_symbols(root, Default::default(), package, None)
        .await;
    backend
}

fn indexes(backend: &Backend, fqn: &str) -> bool {
    backend.symbols.fqn_uri_index.read().get(fqn).is_some()
}

#[tokio::test]
async fn a_project_with_its_own_vendor_below_a_root_project_is_indexed() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("composer.json"),
        r#"{ "require": { "php": ">=8.1" } }"#,
    );
    let platform = dir.path().join("platform");
    write_subproject(&platform, &["Client"]);

    let backend = index_root_project(dir.path()).await;

    assert!(indexes(&backend, "Acme\\Lib\\Client"));
    let own = mapping_base(&backend, "App\\").expect("the nested project's own namespace");
    assert!(
        Path::new(&own).starts_with(&platform),
        "the mapping must point into platform/, got {own}"
    );
}

#[tokio::test]
async fn a_nested_composer_json_without_a_vendor_is_not_a_project() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("composer.json"),
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
    );
    // A test fixture or a path-repository package: nothing installed.
    write(
        &dir.path().join("tests/fixtures/pkg/composer.json"),
        r#"{ "autoload": { "psr-4": { "Fixture\\": "src/" } } }"#,
    );
    // A package the root installed, bundling a vendor of its own.
    let bundled = dir.path().join("vendor/acme/bundled");
    write(
        &bundled.join("composer.json"),
        r#"{ "autoload": { "psr-4": { "Bundled\\": "src/" } } }"#,
    );
    write(
        &bundled.join("vendor/composer/installed.json"),
        r#"{ "packages": [] }"#,
    );

    let backend = index_root_project(dir.path()).await;

    assert!(mapping_base(&backend, "App\\").is_some());
    assert!(mapping_base(&backend, "Fixture\\").is_none());
    assert!(mapping_base(&backend, "Bundled\\").is_none());
}

#[tokio::test]
async fn a_nested_project_installed_after_startup_is_indexed_on_rescan() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("composer.json"),
        r#"{ "require": { "php": ">=8.1" } }"#,
    );
    let platform = dir.path().join("platform");
    write(
        &platform.join("composer.json"),
        r#"{ "require": { "acme/lib": "*" } }"#,
    );

    let backend = index_root_project(dir.path()).await;
    assert!(!indexes(&backend, "Acme\\Lib\\Client"));

    write_subproject(&platform, &["Client"]);
    backend.rescan_composer_indexes(dir.path(), &[platform.join("composer.lock")]);
    assert!(indexes(&backend, "Acme\\Lib\\Client"));

    // Known from now on, so a later update rescans it like any other.
    write_subproject(&platform, &["Renamed"]);
    backend.rescan_composer_indexes(dir.path(), &[platform.join("composer.lock")]);
    assert!(indexes(&backend, "Acme\\Lib\\Renamed"));
    assert!(!indexes(&backend, "Acme\\Lib\\Client"));
}
