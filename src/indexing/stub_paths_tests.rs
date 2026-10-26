use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::Backend;
use crate::config::{ClientStubOptions, Config, StubsConfig};

/// A backend over `root` whose `[stubs] paths` are `paths`.
fn backend_with_stub_paths(root: &Path, paths: &[&str]) -> Backend {
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());
    set_stub_paths(&backend, paths);
    backend
}

fn set_stub_paths(backend: &Backend, paths: &[&str]) {
    backend.set_config(Config {
        stubs: StubsConfig {
            paths: Some(paths.iter().map(|p| p.to_string()).collect()),
        },
        ..Config::default()
    });
}

fn write(root: &Path, relative: &str, content: &str) -> std::path::PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    path
}

/// Parse a project file the way opening it in the editor would.
fn parse_project_file(backend: &Backend, path: &Path) {
    let content = fs::read_to_string(path).unwrap();
    backend.update_ast(&crate::util::path_to_uri(path), &content);
}

fn method_names(backend: &Backend, class: &str) -> Vec<String> {
    let class = backend
        .find_or_load_class(class)
        .unwrap_or_else(|| panic!("{class} should resolve"));
    class.methods.iter().map(|m| m.name.to_string()).collect()
}

const PROJECT_FOO: &str =
    "<?php\nnamespace App;\nclass Foo { public function fromProject(): void {} }\n";
const STUB_FOO: &str = "<?php\nnamespace App;\nclass Foo { public function fromStub(): void {} }\n";

#[test]
fn a_stub_class_wins_over_the_project_class_of_the_same_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let project = write(root, "src/Foo.php", PROJECT_FOO);
    write(root, "stubs/Foo.php", STUB_FOO);
    let backend = backend_with_stub_paths(root, &["stubs"]);

    // Parsed before the stub index exists, as a file opened during
    // startup would be.
    parse_project_file(&backend, &project);
    backend.refresh_stub_paths();

    assert_eq!(method_names(&backend, "App\\Foo"), ["fromStub"]);
}

#[test]
fn removing_the_stub_path_brings_the_project_class_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let project = write(root, "src/Foo.php", PROJECT_FOO);
    write(root, "stubs/Foo.php", STUB_FOO);
    let backend = backend_with_stub_paths(root, &["stubs"]);
    parse_project_file(&backend, &project);
    backend.refresh_stub_paths();
    assert_eq!(method_names(&backend, "App\\Foo"), ["fromStub"]);

    set_stub_paths(&backend, &[]);
    backend.refresh_stub_paths();

    assert_eq!(method_names(&backend, "App\\Foo"), ["fromProject"]);
    assert!(!backend.is_external_stub_uri(&crate::util::path_to_uri(&root.join("stubs/Foo.php"))));
}

#[test]
fn a_stub_function_and_class_win_over_the_embedded_builtins() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "stubs/builtins.php",
        "<?php\nfunction strlen(string $string): string {}\nclass ArrayObject { public function fromStub(): void {} }\n",
    );
    let mut backend = Backend::new_test_with_all_stubs(
        HashMap::from([(
            "ArrayObject",
            "<?php\nclass ArrayObject { public function fromEmbedded(): void {} }\n",
        )]),
        HashMap::from([("strlen", "<?php\nfunction strlen(string $string): int {}\n")]),
        HashMap::new(),
    );
    *backend.state_mut().workspace.workspace_root.write() = Some(root.to_path_buf());
    set_stub_paths(&backend, &["stubs"]);

    // The embedded copies are already loaded, as they would be in any
    // session that reached them before the stub paths changed.
    let embedded = backend.find_or_load_function(&["strlen"]).unwrap();
    assert_eq!(embedded.return_type_str().as_deref(), Some("int"));
    assert_eq!(method_names(&backend, "ArrayObject"), ["fromEmbedded"]);

    backend.refresh_stub_paths();

    let function = backend.find_or_load_function(&["strlen"]).unwrap();
    assert_eq!(function.return_type_str().as_deref(), Some("string"));
    assert_eq!(method_names(&backend, "ArrayObject"), ["fromStub"]);
    assert_eq!(
        backend.load_stub_class("ArrayObject").map(|c| c
            .methods
            .iter()
            .map(|m| m.name.to_string())
            .collect::<Vec<_>>()),
        Some(vec!["fromStub".to_string()])
    );
}

#[test]
fn a_stub_constant_wins_over_a_project_define() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let project = write(
        root,
        "src/constants.php",
        "<?php\ndefine('GREETING', 'project');\n",
    );
    write(
        root,
        "stubs/constants.php",
        "<?php\nconst GREETING = 'stub';\n",
    );
    let backend = backend_with_stub_paths(root, &["stubs"]);
    backend.refresh_stub_paths();

    assert_eq!(
        backend.lookup_global_constant("GREETING"),
        Some(Some("'stub'".to_string()))
    );

    // A project file parsed afterwards does not take the name back.
    parse_project_file(&backend, &project);
    assert_eq!(
        backend.lookup_global_constant("GREETING"),
        Some(Some("'stub'".to_string()))
    );
}

#[test]
fn the_config_paths_win_over_the_editor_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "stubs/Shared.php", STUB_FOO);
    let bundled = tempfile::tempdir().unwrap();
    write(bundled.path(), "Shared.php", PROJECT_FOO);
    write(
        bundled.path(),
        "Bundled.php",
        "<?php\nnamespace App;\nclass Bundled {}\n",
    );
    let backend = backend_with_stub_paths(root, &["stubs"]);
    backend.set_client_stub_options(ClientStubOptions {
        path: Some(bundled.path().to_path_buf()),
    });
    backend.refresh_stub_paths();

    assert_eq!(method_names(&backend, "App\\Foo"), ["fromStub"]);
    assert!(backend.find_or_load_class("App\\Bundled").is_some());
}

#[test]
fn a_phpstorm_stubs_checkout_is_read_from_its_map() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let bundled = tempfile::tempdir().unwrap();
    write(
        bundled.path(),
        "PhpStormStubsMap.php",
        "<?php\nnamespace JetBrains\\PHPStormStub;\nfinal class PhpStormStubsMap\n{\nconst CLASSES = array (\n  'Mapped' => 'ext/Mapped.php',\n);\n\nconst FUNCTIONS = array (\n  'mapped_fn' => 'ext/Mapped.php',\n);\n\nconst CONSTANTS = array (\n);\n}\n",
    );
    write(
        bundled.path(),
        "ext/Mapped.php",
        "<?php\nclass Mapped {}\nfunction mapped_fn(): int {}\n",
    );
    // Not in the map, so not a stub: the map is the whole index.
    write(
        bundled.path(),
        "ext/Unmapped.php",
        "<?php\nclass Unmapped {}\n",
    );
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());
    backend.set_client_stub_options(ClientStubOptions {
        path: Some(bundled.path().to_path_buf()),
    });
    backend.refresh_stub_paths();

    assert!(backend.find_or_load_class("Mapped").is_some());
    assert!(backend.find_or_load_function(&["mapped_fn"]).is_some());
    assert!(backend.find_or_load_class("Unmapped").is_none());
}

#[test]
fn a_stub_path_is_registered_as_library_code() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let stub = write(root, "stubs/Foo.php", STUB_FOO);
    let backend = backend_with_stub_paths(root, &["stubs"]);
    backend.refresh_stub_paths();

    let uri = crate::util::path_to_uri(&stub);
    assert!(backend.is_external_stub_uri(&uri));
    assert!(
        backend
            .workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str())),
        "a stub file inside the workspace is still never analysed"
    );
}

#[test]
fn client_stub_options_are_read_from_either_shape() {
    let expected = Some(ClientStubOptions {
        path: Some("/opt/stubs".into()),
    });
    assert_eq!(
        ClientStubOptions::from_client_settings(&serde_json::json!({
            "stubs": { "path": "/opt/stubs" }
        })),
        expected
    );
    assert_eq!(
        ClientStubOptions::from_client_settings(&serde_json::json!({
            "phpantom": { "stubs": { "path": "/opt/stubs" } }
        })),
        expected
    );
    assert_eq!(
        ClientStubOptions::from_client_settings(&serde_json::json!({ "indexing": {} })),
        None
    );
}

#[test]
fn a_declaration_removed_before_the_project_php_version_is_not_a_stub() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let bundled = tempfile::tempdir().unwrap();
    write(
        bundled.path(),
        "PhpStormStubsMap.php",
        "<?php\nnamespace JetBrains\\PHPStormStub;\nfinal class PhpStormStubsMap\n{\nconst CLASSES = array (\n  'OldClass' => 'standard/standard.php',\n  'Kept' => 'standard/standard.php',\n);\n\nconst FUNCTIONS = array (\n  'each' => 'standard/standard.php',\n  'kept_fn' => 'standard/standard.php',\n);\n\nconst CONSTANTS = array (\n  'OLD_CONST' => 'standard/standard.php',\n  'KEPT_CONST' => 'standard/standard.php',\n);\n}\n",
    );
    write(
        bundled.path(),
        "standard/standard.php",
        "<?php\n/**\n * @removed 8.0\n */\nfunction each(array &$array): array {}\nfunction kept_fn(): int {}\n/**\n * @removed 8.0\n */\nclass OldClass {}\nclass Kept {}\n/**\n * @removed 8.0\n */\ndefine('OLD_CONST', 1);\ndefine('KEPT_CONST', 2);\n",
    );
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());
    backend.set_client_stub_options(ClientStubOptions {
        path: Some(bundled.path().to_path_buf()),
    });
    backend.set_php_version(crate::types::PhpVersion { major: 7, minor: 4 });
    backend.refresh_stub_paths();

    assert!(backend.find_or_load_function(&["each"]).is_some());
    assert!(backend.find_or_load_class("OldClass").is_some());
    assert!(backend.lookup_global_constant("OLD_CONST").is_some());

    backend.set_php_version(crate::types::PhpVersion { major: 8, minor: 0 });

    assert!(backend.find_or_load_function(&["kept_fn"]).is_some());
    assert!(backend.find_or_load_class("Kept").is_some());
    assert!(backend.lookup_global_constant("KEPT_CONST").is_some());
    assert!(backend.find_or_load_function(&["each"]).is_none());
    assert!(backend.find_or_load_class("OldClass").is_none());
    assert!(backend.lookup_global_constant("OLD_CONST").is_none());
    assert!(
        !backend
            .symbols
            .autoload_function_index
            .read()
            .contains_key("each")
    );
    assert!(
        !backend
            .symbols
            .autoload_constant_index
            .read()
            .contains_key("OLD_CONST")
    );
}

/// A Composer package to install: its name, its `type`, and its files as
/// `(relative path, content)`.
type Package<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

/// Install Composer packages into `root`: each goes under
/// `vendor/<name>`, and `installed.json` lists them in order.
fn install_packages(root: &Path, packages: &[Package]) {
    write(root, "composer.json", "{}");
    let mut entries = Vec::new();
    for (name, kind, files) in packages {
        for (relative, content) in *files {
            write(root, &format!("vendor/{name}/{relative}"), content);
        }
        entries.push(serde_json::json!({
            "name": name,
            "type": kind,
            "install-path": format!("../{name}"),
        }));
    }
    write(
        root,
        "vendor/composer/installed.json",
        &serde_json::json!({ "packages": entries }).to_string(),
    );
}

const WORDPRESS_STUBS: &[(&str, &str)] = &[(
    "wordpress-stubs.php",
    "<?php\nclass WP_Post { public function fromWordPressStubs(): void {} }\nfunction add_action(string $hook, callable $callback): true {}\n",
)];

#[test]
fn a_composer_stub_package_is_a_stub_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    install_packages(
        root,
        &[
            ("php-stubs/wordpress-stubs", "library", WORDPRESS_STUBS),
            (
                "acme/extension-headers",
                "php-stubs",
                &[("acme.php", "<?php\nfunction acme_ext(): int {}\n")],
            ),
            (
                "acme/runtime",
                "library",
                &[("Plain.php", "<?php\nclass NotAStub {}\n")],
            ),
        ],
    );
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());
    backend.rescan_composer_indexes(root, &[]);

    assert_eq!(method_names(&backend, "WP_Post"), ["fromWordPressStubs"]);
    assert!(backend.find_or_load_function(&["add_action"]).is_some());
    assert!(backend.find_or_load_function(&["acme_ext"]).is_some());
    assert!(backend.find_or_load_class("NotAStub").is_none());
    let stub_uri = crate::util::path_to_uri(
        &root
            .join("vendor/php-stubs/wordpress-stubs/wordpress-stubs.php")
            .canonicalize()
            .unwrap(),
    );
    assert!(backend.is_external_stub_uri(&stub_uri));
}

#[test]
fn signature_only_builtin_stubs_are_not_a_stub_package() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    install_packages(
        root,
        &[(
            "phpstan/php-8-stubs",
            "library",
            &[(
                "stubs/ext/standard/strlen.php",
                "<?php\nfunction strlen(string $string): string {}\n",
            )],
        )],
    );
    let mut backend = Backend::new_test_with_all_stubs(
        HashMap::new(),
        HashMap::from([("strlen", "<?php\nfunction strlen(string $string): int {}\n")]),
        HashMap::new(),
    );
    *backend.state_mut().workspace.workspace_root.write() = Some(root.to_path_buf());
    backend.rescan_composer_indexes(root, &[]);

    assert!(backend.stub_paths().is_empty());
    let function = backend.find_or_load_function(&["strlen"]).unwrap();
    assert_eq!(function.return_type_str().as_deref(), Some("int"));
}

#[test]
fn composer_stub_packages_rank_between_the_config_paths_and_the_editor_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "stubs/Foo.php", STUB_FOO);
    install_packages(
        root,
        &[(
            "acme/foo-stubs",
            "library",
            &[
                ("Foo.php", PROJECT_FOO),
                (
                    "Bar.php",
                    "<?php\nnamespace App;\nclass Bar { public function fromPackage(): void {} }\n",
                ),
            ],
        )],
    );
    let bundled = tempfile::tempdir().unwrap();
    write(
        bundled.path(),
        "Bar.php",
        "<?php\nnamespace App;\nclass Bar { public function fromEditor(): void {} }\n",
    );
    let backend = backend_with_stub_paths(root, &["stubs"]);
    backend.set_client_stub_options(ClientStubOptions {
        path: Some(bundled.path().to_path_buf()),
    });
    backend.rescan_composer_indexes(root, &[]);

    assert_eq!(method_names(&backend, "App\\Foo"), ["fromStub"]);
    assert_eq!(method_names(&backend, "App\\Bar"), ["fromPackage"]);
}

#[test]
fn a_composer_change_installs_and_removes_stub_packages() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    install_packages(root, &[]);
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());
    backend.rescan_composer_indexes(root, &[]);
    assert!(backend.find_or_load_function(&["add_action"]).is_none());

    install_packages(
        root,
        &[("php-stubs/wordpress-stubs", "library", WORDPRESS_STUBS)],
    );
    backend.rescan_composer_indexes(root, &[root.join("composer.lock")]);
    assert!(backend.find_or_load_function(&["add_action"]).is_some());
    assert_eq!(method_names(&backend, "WP_Post"), ["fromWordPressStubs"]);

    fs::remove_dir_all(root.join("vendor/php-stubs")).unwrap();
    install_packages(root, &[]);
    backend.rescan_composer_indexes(root, &[root.join("composer.lock")]);
    assert!(backend.stub_paths().is_empty());
    assert!(backend.find_or_load_function(&["add_action"]).is_none());
    assert!(backend.find_or_load_class("WP_Post").is_none());
}

const RANGE_STUB: &str = "<?php\nfunction range($start, $end, int|float $step = 1): array {}\n";

#[test]
fn a_phpstorm_stubs_checkout_gets_the_embedded_corrections() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    install_packages(
        root,
        &[(
            "jetbrains/phpstorm-stubs",
            "library",
            &[
                (
                    "PhpStormStubsMap.php",
                    "<?php\nnamespace JetBrains\\PHPStormStub;\nfinal class PhpStormStubsMap\n{\nconst CLASSES = array (\n);\n\nconst FUNCTIONS = array (\n  'range' => 'standard/standard_8.php',\n);\n\nconst CONSTANTS = array (\n);\n}\n",
                ),
                ("standard/standard_8.php", RANGE_STUB),
            ],
        )],
    );
    let backend = Backend::new_test_with_workspace(root.to_path_buf(), Vec::new());
    backend.rescan_composer_indexes(root, &[]);

    let range = backend.find_or_load_function(&["range"]).unwrap();
    let uri = backend
        .symbols
        .global_functions
        .read()
        .get("range")
        .unwrap()
        .0
        .clone();
    assert!(
        uri.ends_with("/standard/standard_8.php"),
        "range comes from the installed checkout, got {uri}"
    );
    assert!(range.conditional_return.is_some());
}

#[test]
fn a_stub_path_that_is_not_phpstorm_stubs_is_left_as_written() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "stubs/range.php", RANGE_STUB);
    let backend = backend_with_stub_paths(root, &["stubs"]);
    backend.refresh_stub_paths();

    let range = backend.find_or_load_function(&["range"]).unwrap();
    assert!(range.conditional_return.is_none());
}
