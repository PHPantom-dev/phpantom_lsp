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
        Path::new(&module).ends_with("app/modules/common/src"),
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

#[tokio::test]
async fn the_locked_phpunit_version_is_read_and_follows_an_update() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("composer.json"),
        r#"{ "require-dev": { "phpunit/phpunit": "^10.5" } }"#,
    );
    write(
        &dir.path().join("composer.lock"),
        r#"{ "packages": [], "packages-dev": [{ "name": "phpunit/phpunit", "version": "10.5.38" }] }"#,
    );

    let backend = index_root_project(dir.path()).await;
    assert_eq!(backend.phpunit_major(), Some(10));

    write(
        &dir.path().join("composer.lock"),
        r#"{ "packages": [], "packages-dev": [{ "name": "phpunit/phpunit", "version": "9.6.20" }] }"#,
    );
    backend.rescan_composer_indexes(dir.path(), &[dir.path().join("composer.lock")]);
    assert_eq!(backend.phpunit_major(), Some(9));
}

/// Drupal keeps PHP in `.module`, `.install`, … files. Once the web root
/// is detected every workspace walk has to read them, not only the class
/// scan of the web root, or Find References misses the calls in them.
#[tokio::test]
async fn a_drupal_site_reads_its_hook_files_as_php_everywhere() {
    use tower_lsp_server::gen_lsp_types::{Position, Uri};

    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("composer.json"),
        r#"{ "require": { "drupal/core-recommended": "^11" } }"#,
    );
    write(
        &dir.path().join("web/core/lib/Drupal.php"),
        "<?php\nclass Drupal {}\n",
    );
    let thing_path = dir.path().join("web/modules/custom/demo/src/Thing.php");
    let thing = "<?php\nnamespace Demo;\nclass Thing {\n    public function go(): void {}\n}\n";
    write(&thing_path, thing);
    write(
        &dir.path().join("web/modules/custom/demo/demo.install"),
        "<?php\nfunction demo_install() {\n    (new \\Demo\\Thing())->go();\n}\n",
    );

    let backend = index_root_project(dir.path()).await;
    assert!(
        backend
            .index_filters()
            .is_php_file(&dir.path().join("demo.install"))
    );

    let thing_uri = Uri::from_file_path(&thing_path).unwrap().to_string();
    let locations = backend
        .find_references(&thing_uri, thing, Position::new(3, 21), false)
        .expect("references to Thing::go()");
    assert!(
        locations
            .iter()
            .any(|loc| loc.uri.as_str().ends_with("demo.install")),
        "the call in demo.install must be found: {locations:?}"
    );
}

/// A Drupal tree with its web root at `web_root` and no `composer.json`
/// of the project's own, only core's and a module's.  The module sits in a
/// vendor folder, outside `contrib` and `custom`, and the tree ignores
/// every module the way a site's `.gitignore` ignores contributed ones.
fn write_drupal_tree(web_root: &Path) {
    write(
        &web_root.join("core/lib/Drupal.php"),
        "<?php\nclass Drupal {}\n",
    );
    write(
        &web_root.join("core/composer.json"),
        r#"{ "name": "drupal/core", "autoload": { "psr-4": { "Drupal\\Core\\": "lib/Drupal/Core" } } }"#,
    );
    let module = web_root.join("modules/acme/acme_greeter");
    write(
        &module.join("composer.json"),
        r#"{ "name": "acme/acme_greeter", "type": "drupal-module" }"#,
    );
    write(
        &module.join("src/Greeter.php"),
        "<?php\nnamespace Drupal\\acme_greeter;\nclass Greeter {}\n",
    );
    write(
        &module.join("acme_greeter.services.yml"),
        "services:\n  acme_greeter.greeter:\n    class: Drupal\\acme_greeter\\Greeter\n",
    );
    write(&web_root.join(".gitignore"), "/modules/\n");
}

/// What every Drupal tree without a `composer.json` must give, with its
/// web root at `web_root` inside the workspace at `root`, a git checkout
/// so that the tree's `.gitignore` holds.
async fn assert_indexed_as_a_drupal_site(root: &Path, web_root: &Path) {
    std::fs::create_dir_all(root.join(".git")).unwrap();
    assert!(
        !crate::composer::discover_subproject_roots(root).is_empty(),
        "the tree must hold the composer.json files a monorepo would be made of"
    );

    let backend = index_workspace(root).await;

    assert!(backend.is_drupal_project());
    assert!(indexes(&backend, "Drupal\\acme_greeter\\Greeter"));
    // The workspace walk leaves core to the Drupal scan.
    assert!(indexes(&backend, "Drupal"));
    let services = web_root.join("modules/acme/acme_greeter/acme_greeter.services.yml");
    assert!(
        backend.drupal_declaration_files().contains(&services),
        "{:?}",
        backend.drupal_declaration_files()
    );
    for project in ["core", "modules/acme/acme_greeter"] {
        assert!(
            !is_composer_project(&backend, &web_root.join(project)),
            "{project} must not be indexed as a project of its own"
        );
    }
}

#[tokio::test]
async fn a_drupal_tree_without_composer_json_is_a_drupal_site() {
    let dir = tempfile::tempdir().unwrap();
    write_drupal_tree(dir.path());
    assert_indexed_as_a_drupal_site(dir.path(), dir.path()).await;
}

#[tokio::test]
async fn a_drupal_tree_under_web_without_composer_json_is_a_drupal_site() {
    let dir = tempfile::tempdir().unwrap();
    let web_root = dir.path().join("web");
    write_drupal_tree(&web_root);
    assert_indexed_as_a_drupal_site(dir.path(), &web_root).await;
}

/// Without Drupal core, a workspace holding a `core/` folder with a
/// `composer.json` is still a monorepo, and one without any is still a
/// plain workspace; neither is a Drupal site.
#[tokio::test]
async fn a_workspace_without_drupal_core_is_no_drupal_site() {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("core/composer.json"),
        r#"{ "autoload": { "psr-4": { "Core\\": "src/" } } }"#,
    );
    write(
        &dir.path().join("core/src/Kernel.php"),
        "<?php\nnamespace Core;\nclass Kernel {}\n",
    );
    let backend = index_workspace(dir.path()).await;
    assert!(!backend.is_drupal_project());
    assert!(is_composer_project(&backend, &dir.path().join("core")));

    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("lib/Thing.php"),
        "<?php\nnamespace Plain;\nclass Thing {}\n",
    );
    let backend = index_workspace(dir.path()).await;
    assert!(!backend.is_drupal_project());
    assert!(indexes(&backend, "Plain\\Thing"));
}

/// A Drupal project in `docroot/` of a workspace without a `composer.json`
/// of its own owns the site: it is indexed as a Composer project, with the
/// dependencies its gitignored `vendor/` holds.
#[tokio::test]
async fn a_drupal_project_in_a_workspace_folder_keeps_its_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    let docroot = dir.path().join("docroot");
    write_subproject(&docroot, &["Client"]);
    write(
        &docroot.join("composer.json"),
        r#"{ "require": { "drupal/core-recommended": "^11", "acme/lib": "*" } }"#,
    );
    write(
        &docroot.join("core/lib/Drupal.php"),
        "<?php\nclass Drupal {}\n",
    );
    write(&docroot.join(".gitignore"), "/vendor/\n");

    let backend = index_workspace(dir.path()).await;

    assert!(backend.is_drupal_project());
    assert!(is_composer_project(&backend, &docroot));
    assert!(indexes(&backend, "Acme\\Lib\\Client"));
}

/// A Composer project with its own dependencies beside a Drupal tree that
/// has no `composer.json`, or inside it when the tree is the workspace, is
/// still indexed as a project, and the tree as a Drupal site.
#[tokio::test]
async fn a_drupal_tree_without_composer_json_keeps_a_project_beside_it() {
    for web in ["", "web"] {
        let dir = tempfile::tempdir().unwrap();
        let web_root = if web.is_empty() {
            dir.path().to_path_buf()
        } else {
            dir.path().join(web)
        };
        write_drupal_tree(&web_root);
        let behat = dir.path().join("tests/behat");
        write_subproject(&behat, &["Client"]);
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();

        let backend = index_workspace(dir.path()).await;

        assert!(backend.is_drupal_project(), "{web:?}");
        assert!(is_composer_project(&backend, &behat), "{web:?}");
        assert!(indexes(&backend, "Acme\\Lib\\Client"), "{web:?}");
        assert!(
            indexes(&backend, "Drupal\\acme_greeter\\Greeter"),
            "{web:?}"
        );
        assert!(
            !is_composer_project(&backend, &web_root.join("core")),
            "{web:?}"
        );
    }
}

/// A root project using `bamarni/composer-bin-plugin`, whose `phpunit` bin
/// namespace installs `PHPUnit\Framework\<class>` for each of `classes`
/// at PHPUnit `version`.
fn write_bin_plugin_project(root: &Path, plugin: bool, classes: &[&str], version: &str) {
    let require = if plugin {
        r#""require-dev": { "bamarni/composer-bin-plugin": "^1.8" },"#
    } else {
        ""
    };
    write(
        &root.join("composer.json"),
        &format!(r#"{{ {require} "autoload": {{ "psr-4": {{ "App\\": "src/" }} }} }}"#),
    );
    let namespace = root.join("vendor-bin/phpunit");
    write(
        &namespace.join("composer.json"),
        r#"{ "require": { "phpunit/phpunit": "*" } }"#,
    );
    write(
        &namespace.join("composer.lock"),
        &format!(r#"{{ "packages": [{{ "name": "phpunit/phpunit", "version": "{version}" }}] }}"#),
    );
    write(
        &namespace.join("vendor/composer/installed.json"),
        r#"{ "packages": [ {
            "name": "phpunit/phpunit",
            "install-path": "../phpunit/phpunit",
            "autoload": { "psr-4": { "PHPUnit\\": "src/" } }
        } ] }"#,
    );
    let src = namespace.join("vendor/phpunit/phpunit/src");
    if src.exists() {
        std::fs::remove_dir_all(&src).unwrap();
    }
    for class in classes {
        write(
            &src.join(format!("Framework/{class}.php")),
            &format!("<?php\nnamespace PHPUnit\\Framework;\nclass {class} {{}}\n"),
        );
    }
}

fn indexed_uri(backend: &Backend, fqn: &str) -> Option<String> {
    backend.symbols.fqn_uri_index.read().get(fqn).cloned()
}

fn is_composer_project(backend: &Backend, dir: &Path) -> bool {
    backend
        .workspace
        .composer_projects
        .read()
        .iter()
        .any(|project| project.root == dir)
}

#[tokio::test]
async fn a_bin_namespace_installs_tools_for_the_root_project() {
    let dir = tempfile::tempdir().unwrap();
    write_bin_plugin_project(dir.path(), true, &["TestCase"], "10.5.38");
    // A nested project of its own, so the root's files run in a scope.
    write_subproject(&dir.path().join("platform"), &["Client"]);

    let backend = index_root_project(dir.path()).await;

    let namespace = dir.path().join("vendor-bin/phpunit");
    assert!(!is_composer_project(&backend, &namespace));
    let uri = indexed_uri(&backend, "PHPUnit\\Framework\\TestCase")
        .expect("the class the bin namespace installs must be indexed");
    let test_uri = crate::util::path_to_uri(&dir.path().join("tests/FooTest.php"));
    let scope = backend
        .project_scope_for(&test_uri)
        .expect("the root has a scope beside platform/");
    assert!(scope.sees(&uri), "the root's files must see {uri}");
    let platform_uri = crate::util::path_to_uri(&dir.path().join("platform/src/A.php"));
    let platform_scope = backend.project_scope_for(&platform_uri).unwrap();
    assert!(
        !platform_scope.sees(&uri),
        "another project must not see the root's tools"
    );

    // Library code: never analysed or edited as the project's own.
    assert!(
        backend
            .workspace
            .vendor_dir_paths
            .lock()
            .contains(&namespace.join("vendor")),
        "the bin namespace's vendor must be registered as library code"
    );
    assert_eq!(
        backend
            .symbols
            .fqn_origin_index
            .read()
            .get("PHPUnit\\Framework\\TestCase")
            .copied(),
        Some(ClassCompletionOrigin::VendorExplicit)
    );
    assert_eq!(backend.phpunit_major(), Some(10));
}

#[tokio::test]
async fn without_the_plugin_a_vendor_bin_project_stays_a_nested_project() {
    let dir = tempfile::tempdir().unwrap();
    write_bin_plugin_project(dir.path(), false, &["TestCase"], "10.5.38");

    let backend = index_root_project(dir.path()).await;

    assert!(is_composer_project(
        &backend,
        &dir.path().join("vendor-bin/phpunit")
    ));
    assert!(indexes(&backend, "PHPUnit\\Framework\\TestCase"));
}

#[tokio::test]
async fn the_root_vendor_wins_over_a_bin_namespace() {
    let dir = tempfile::tempdir().unwrap();
    write_bin_plugin_project(dir.path(), true, &["TestCase"], "10.5.38");
    write(
        &dir.path().join("vendor/composer/installed.json"),
        r#"{ "packages": [ {
            "name": "phpunit/phpunit",
            "install-path": "../phpunit/phpunit",
            "autoload": { "psr-4": { "PHPUnit\\": "src/" } }
        } ] }"#,
    );
    write(
        &dir.path()
            .join("vendor/phpunit/phpunit/src/Framework/TestCase.php"),
        "<?php\nnamespace PHPUnit\\Framework;\nclass TestCase {}\n",
    );
    let root_copy = crate::util::path_to_uri(
        &dir.path()
            .join("vendor/phpunit/phpunit/src/Framework/TestCase.php"),
    );

    let backend = index_root_project(dir.path()).await;
    assert_eq!(
        indexed_uri(&backend, "PHPUnit\\Framework\\TestCase"),
        Some(root_copy.clone())
    );

    // …and still does after the bin namespace is updated.
    let lock = dir.path().join("vendor-bin/phpunit/composer.lock");
    backend.rescan_composer_indexes(dir.path(), &[lock]);
    assert_eq!(
        indexed_uri(&backend, "PHPUnit\\Framework\\TestCase"),
        Some(root_copy)
    );
}

#[tokio::test]
async fn an_update_inside_a_bin_namespace_is_picked_up() {
    let dir = tempfile::tempdir().unwrap();
    write_bin_plugin_project(dir.path(), true, &["TestCase"], "9.6.20");

    let backend = index_root_project(dir.path()).await;
    assert_eq!(backend.phpunit_major(), Some(9));

    write_bin_plugin_project(dir.path(), true, &["Assert"], "10.5.38");
    let lock = dir.path().join("vendor-bin/phpunit/composer.lock");
    backend.rescan_composer_indexes(dir.path(), &[lock]);

    assert!(indexes(&backend, "PHPUnit\\Framework\\Assert"));
    assert!(
        !indexes(&backend, "PHPUnit\\Framework\\TestCase"),
        "a class the update removed must stop resolving"
    );
    assert!(
        !is_composer_project(&backend, &dir.path().join("vendor-bin/phpunit")),
        "the update must not index the namespace as a nested project"
    );
    assert_eq!(backend.phpunit_major(), Some(10));
}
