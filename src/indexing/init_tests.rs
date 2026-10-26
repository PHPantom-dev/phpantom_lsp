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
