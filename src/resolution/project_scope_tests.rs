use std::path::Path;

use tower_lsp_server::gen_lsp_types::Code;

use crate::Backend;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A Composer project at `root` that installed `acme/lib`, whose
/// `Acme\Lib\Client` declares the method `method`, and, when `guzzle` is
/// set, a `GuzzleHttp\Client` as well.
fn write_project(root: &Path, method: &str, guzzle: bool) {
    write(
        &root.join("composer.json"),
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
    );
    let mut packages = vec![
        r#"{ "name": "acme/lib", "install-path": "../acme/lib",
             "autoload": { "psr-4": { "Acme\\Lib\\": "src/" } } }"#,
    ];
    if guzzle {
        packages.push(
            r#"{ "name": "guzzlehttp/guzzle", "install-path": "../guzzlehttp/guzzle",
                 "autoload": { "psr-4": { "GuzzleHttp\\": "src/" } } }"#,
        );
    }
    write(
        &root.join("vendor/composer/installed.json"),
        &format!(r#"{{ "packages": [ {} ] }}"#, packages.join(",")),
    );
    write(
        &root.join("vendor/acme/lib/src/Client.php"),
        &format!(
            "<?php\nnamespace Acme\\Lib;\nclass Client {{\n    public function {method}(): void {{}}\n}}\n"
        ),
    );
    if guzzle {
        write(
            &root.join("vendor/guzzlehttp/guzzle/src/Client.php"),
            "<?php\nnamespace GuzzleHttp;\nclass Client {\n    public function send(): void {}\n}\n",
        );
    }
}

/// A monorepo whose `platform/` installs Guzzle and a version of
/// `acme/lib` with `oldMethod()`, and whose `mobile/` installs no Guzzle
/// and a version of `acme/lib` with `newMethod()`.
async fn monorepo() -> (tempfile::TempDir, Backend) {
    let dir = tempfile::tempdir().unwrap();
    write_project(&dir.path().join("platform"), "oldMethod", true);
    write_project(&dir.path().join("mobile"), "newMethod", false);
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    backend
        .discover_workspace_symbols(dir.path(), Default::default(), None, None)
        .await;
    (dir, backend)
}

/// The codes of the diagnostics `source`, written to `path`, gets.
fn diagnostic_codes(backend: &Backend, path: &Path, source: &str) -> Vec<String> {
    write(path, source);
    let uri = crate::util::path_to_uri(path);
    backend.update_ast(&uri, source);
    backend
        .collect_workspace_file_diagnostics(&uri, &[])
        .expect("the file is readable")
        .into_iter()
        .filter_map(|d| match d.code {
            Some(Code::String(code)) => Some(code),
            _ => None,
        })
        .collect()
}

const USES_GUZZLE: &str = "<?php\nnamespace App;\nfunction send(): void {\n    (new \\GuzzleHttp\\Client())->send();\n}\n";

#[tokio::test]
async fn a_class_only_another_project_installed_is_unknown() {
    let (dir, backend) = monorepo().await;

    let mobile = diagnostic_codes(&backend, &dir.path().join("mobile/src/a.php"), USES_GUZZLE);
    assert!(
        mobile.iter().any(|code| code == "unknown_class"),
        "mobile/ did not install Guzzle, got {mobile:?}"
    );

    let platform = diagnostic_codes(
        &backend,
        &dir.path().join("platform/src/a.php"),
        USES_GUZZLE,
    );
    assert!(
        !platform.iter().any(|code| code == "unknown_class"),
        "platform/ installed Guzzle, got {platform:?}"
    );
}

#[tokio::test]
async fn each_project_sees_its_own_version_of_a_shared_package() {
    let (dir, backend) = monorepo().await;
    let calling = |method: &str| {
        format!(
            "<?php\nnamespace App;\nfunction go(): void {{\n    (new \\Acme\\Lib\\Client())->{method}();\n}}\n"
        )
    };

    for (project, own, other) in [
        ("platform", "oldMethod", "newMethod"),
        ("mobile", "newMethod", "oldMethod"),
    ] {
        let path = dir.path().join(project).join("src/a.php");
        let codes = diagnostic_codes(&backend, &path, &calling(own));
        assert!(
            !codes.iter().any(|code| code == "unknown_member"),
            "{project}/ installed a Client with {own}(), got {codes:?}"
        );
        let codes = diagnostic_codes(&backend, &path, &calling(other));
        assert!(
            codes.iter().any(|code| code == "unknown_member"),
            "{project}/'s Client has no {other}(), got {codes:?}"
        );
    }
}

#[tokio::test]
async fn work_outside_any_file_sees_the_whole_workspace() {
    let (_dir, backend) = monorepo().await;
    assert!(backend.find_or_load_class("GuzzleHttp\\Client").is_some());
}

#[tokio::test]
async fn a_file_outside_every_project_sees_the_whole_workspace() {
    let (dir, backend) = monorepo().await;
    let codes = diagnostic_codes(&backend, &dir.path().join("tools/a.php"), USES_GUZZLE);
    assert!(
        !codes.iter().any(|code| code == "unknown_class"),
        "got {codes:?}"
    );
}

#[tokio::test]
async fn go_to_definition_opens_the_projects_own_copy() {
    let (dir, backend) = monorepo().await;
    for project in ["platform", "mobile"] {
        let path = dir.path().join(project).join("src/b.php");
        let uri = crate::util::path_to_uri(&path);
        let _scope = backend.enter_project_scope(&uri);
        let target = backend
            .class_declaration_uri("Acme\\Lib\\Client")
            .expect("both projects installed acme/lib");
        let own_vendor = crate::util::dir_uri_prefix(
            &dir.path()
                .join(project)
                .join("vendor")
                .canonicalize()
                .unwrap(),
        );
        assert!(
            target.starts_with(&own_vendor),
            "{project}/ must open its own copy, got {target}"
        );
    }
}

/// One Composer project has nothing to keep apart, so lookups take the
/// path they always did.
#[tokio::test]
async fn a_single_project_workspace_has_no_scopes() {
    let dir = tempfile::tempdir().unwrap();
    write_project(&dir.path().join("platform"), "oldMethod", true);
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    backend
        .discover_workspace_symbols(dir.path(), Default::default(), None, None)
        .await;

    let uri = crate::util::path_to_uri(&dir.path().join("platform/src/a.php"));
    assert!(backend.project_scope_for(&uri).is_none());
}

const USES_PLATFORM_ONLY: &str =
    "<?php\nnamespace App;\nfunction go(): void {\n    (new \\App\\Platform\\Only())->run();\n}\n";

const PLATFORM_ONLY: &str =
    "<?php\nnamespace App\\Platform;\nclass Only {\n    public function run(): void {}\n}\n";

#[tokio::test]
async fn another_projects_own_sources_are_unknown() {
    let dir = tempfile::tempdir().unwrap();
    write_project(&dir.path().join("platform"), "oldMethod", true);
    write_project(&dir.path().join("mobile"), "newMethod", false);
    write(
        &dir.path().join("platform/src/Platform/Only.php"),
        PLATFORM_ONLY,
    );
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    backend
        .discover_workspace_symbols(dir.path(), Default::default(), None, None)
        .await;

    let mobile = diagnostic_codes(
        &backend,
        &dir.path().join("mobile/src/a.php"),
        USES_PLATFORM_ONLY,
    );
    assert!(
        mobile.iter().any(|code| code == "unknown_class"),
        "mobile/'s autoloader cannot load platform/src/, got {mobile:?}"
    );

    let platform = diagnostic_codes(
        &backend,
        &dir.path().join("platform/src/a.php"),
        USES_PLATFORM_ONLY,
    );
    assert!(
        !platform.iter().any(|code| code == "unknown_class"),
        "got {platform:?}"
    );
}

/// A class only reachable through another project's PSR-4 mapping, never
/// indexed, is not remembered as missing for that project.
#[tokio::test]
async fn a_class_hidden_from_one_project_still_loads_for_its_own() {
    let (dir, backend) = monorepo().await;
    write(
        &dir.path().join("platform/src/Platform/Only.php"),
        PLATFORM_ONLY,
    );

    for (project, known) in [("mobile", false), ("platform", true)] {
        let uri = crate::util::path_to_uri(&dir.path().join(project).join("src/a.php"));
        let _scope = backend.enter_project_scope(&uri);
        assert_eq!(
            backend.find_or_load_class("App\\Platform\\Only").is_some(),
            known,
            "{project}/"
        );
    }
}

#[tokio::test]
async fn a_package_installed_from_another_projects_tree_is_visible() {
    let (dir, backend) = {
        let dir = tempfile::tempdir().unwrap();
        write_project(&dir.path().join("platform"), "oldMethod", true);
        write_project(&dir.path().join("mobile"), "newMethod", false);
        write(
            &dir.path().join("platform/packages/shared/src/Thing.php"),
            "<?php\nnamespace Acme\\Shared;\nclass Thing {\n    public function run(): void {}\n}\n",
        );
        write(
            &dir.path().join("mobile/vendor/composer/installed.json"),
            r#"{ "packages": [
                { "name": "acme/lib", "install-path": "../acme/lib",
                  "autoload": { "psr-4": { "Acme\\Lib\\": "src/" } } },
                { "name": "acme/shared", "install-path": "../../../platform/packages/shared",
                  "dist": { "type": "path" }, "transport-options": { "symlink": true },
                  "autoload": { "psr-4": { "Acme\\Shared\\": "src/" } } }
            ] }"#,
        );
        let backend = Backend::new_test();
        *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
        backend
            .discover_workspace_symbols(dir.path(), Default::default(), None, None)
            .await;
        (dir, backend)
    };

    let codes = diagnostic_codes(
        &backend,
        &dir.path().join("mobile/src/a.php"),
        "<?php\nnamespace App;\nfunction go(): void {\n    (new \\Acme\\Shared\\Thing())->run();\n}\n",
    );
    assert!(
        !codes.iter().any(|code| code == "unknown_class"),
        "mobile/ installed acme/shared from platform/packages/, got {codes:?}"
    );
}

#[tokio::test]
async fn a_file_outside_every_project_is_visible_to_all_of_them() {
    let (dir, backend) = monorepo().await;
    write(
        &dir.path().join("tools/Helper.php"),
        "<?php\nnamespace Tools;\nclass Helper {\n    public function run(): void {}\n}\n",
    );
    let uri = crate::util::path_to_uri(&dir.path().join("tools/Helper.php"));
    backend.update_ast(
        &uri,
        &std::fs::read_to_string(dir.path().join("tools/Helper.php")).unwrap(),
    );

    for project in ["platform", "mobile"] {
        let codes = diagnostic_codes(
            &backend,
            &dir.path().join(project).join("src/a.php"),
            "<?php\nnamespace App;\nfunction go(): void {\n    (new \\Tools\\Helper())->run();\n}\n",
        );
        assert!(
            !codes.iter().any(|code| code == "unknown_class"),
            "{project}/: got {codes:?}"
        );
    }
}

/// A project nested below the workspace's root project and the root
/// project each only see their own sources.
#[tokio::test]
async fn a_nested_project_and_the_project_around_it_keep_their_sources_apart() {
    let dir = tempfile::tempdir().unwrap();
    write_project(dir.path(), "newMethod", false);
    write_project(&dir.path().join("platform"), "oldMethod", true);
    write(
        &dir.path().join("platform/src/Platform/Only.php"),
        PLATFORM_ONLY,
    );
    write(
        &dir.path().join("src/Root/Only.php"),
        "<?php\nnamespace App\\Root;\nclass Only {\n    public function run(): void {}\n}\n",
    );
    let backend = Backend::new_test();
    *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
    let package = crate::composer::read_composer_package(dir.path());
    backend
        .discover_workspace_symbols(dir.path(), Default::default(), package, None)
        .await;

    for (file, class, known) in [
        ("src/a.php", "App\\Root\\Only", true),
        ("src/a.php", "App\\Platform\\Only", false),
        ("platform/src/a.php", "App\\Platform\\Only", true),
        ("platform/src/a.php", "App\\Root\\Only", false),
    ] {
        let uri = crate::util::path_to_uri(&dir.path().join(file));
        let _scope = backend.enter_project_scope(&uri);
        assert_eq!(
            backend.find_or_load_class(class).is_some(),
            known,
            "{class} from {file}"
        );
    }
}
