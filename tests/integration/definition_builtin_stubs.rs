//! Go-to-definition on built-in symbols lands in the project's own copy of
//! `jetbrains/phpstorm-stubs` when Composer installed one, which is a stub
//! source like any other stub package.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::common::{goto_definition_at, open_php};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// What the binary embeds.  Declared on line 1, unlike the installed copy,
/// so a jump that lands on the installed file's lines came from there.
const EMBEDDED_PDO: &str = "<?php\nclass PDO {\n    public const PARAM_STR = 2;\n    public function prepare(string $query) {}\n}\n";
const EMBEDDED_BACKED_ENUM: &str =
    "<?php\ninterface BackedEnum {\n    public readonly int|string $value;\n}\n";
const EMBEDDED_STANDARD: &str =
    "<?php\nfunction array_map(?callable $callback, array $array): array {}\n";
const EMBEDDED_CORE: &str = "<?php\ndefine('PHP_EOL', \"\\n\");\n";

const INSTALLED_PDO: &str = "<?php

// Start of PDO

/**
 * Represents a connection between PHP and a database server.
 */
class PDO
{
    public const PARAM_STR = 2;

    public function prepare(string $query) {}
}
";

const INSTALLED_BACKED_ENUM: &str = "<?php

interface BackedEnum extends UnitEnum
{
    public static function from(int|string $value): static;

    public static function tryFrom(int|string $value): ?static;

    public readonly int|string $value;
}
";

const INSTALLED_STANDARD: &str = "<?php

/**
 * Applies the callback to the elements of the given arrays.
 */
function array_map(?callable $callback, array $array, array ...$arrays): array {}
";

const INSTALLED_CORE: &str = "<?php

/**
 * The correct 'End Of Line' symbol for this platform.
 */
define('PHP_EOL', \"\\n\");
";

const STUBS_MAP: &str = "<?php

declare(strict_types=1);

namespace JetBrains\\PHPStormStub;

/**
 * This is a generated file, do not modify it directly!
 */
final class PhpStormStubsMap
{
const DIR = __DIR__;

const CLASSES = array (
  'BackedEnum' => 'Core/Core_c.php',
  'PDO' => 'PDO/PDO.php',
);

const FUNCTIONS = array (
  'array_map' => 'standard/standard_9.php',
);

const CONSTANTS = array (
  'PHP_EOL' => 'Core/Core_d.php',
);
}
";

const INSTALLED_JSON: &str = r#"{
    "packages": [
        { "name": "jetbrains/phpstorm-stubs", "install-path": "../jetbrains/phpstorm-stubs",
          "autoload": { "files": ["PhpStormStubsMap.php"] } }
    ]
}"#;

const APP: &str = "<?php
$pdo = new PDO('sqlite::memory:');
$pdo->prepare('SELECT 1');
echo PDO::PARAM_STR;
array_map(null, []);
echo PHP_EOL;
new pdo('sqlite::memory:');
";

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// The root of the installed phpstorm-stubs package in `workspace`.
fn stubs_root(workspace: &Path) -> PathBuf {
    workspace.join("vendor/jetbrains/phpstorm-stubs")
}

fn install_stubs(workspace: &Path) {
    let root = stubs_root(workspace);
    write(&root.join("PhpStormStubsMap.php"), STUBS_MAP);
    write(&root.join("PDO/PDO.php"), INSTALLED_PDO);
    write(&root.join("Core/Core_c.php"), INSTALLED_BACKED_ENUM);
    write(&root.join("standard/standard_9.php"), INSTALLED_STANDARD);
    write(&root.join("Core/Core_d.php"), INSTALLED_CORE);
    write(
        &workspace.join("vendor/composer/installed.json"),
        INSTALLED_JSON,
    );
}

/// A backend whose embedded stubs declare `PDO`, `array_map` and
/// `PHP_EOL`, initialized on a workspace that may have phpstorm-stubs
/// installed.
async fn backend_for(workspace: &Path) -> Backend {
    write(&workspace.join("composer.json"), "{}");
    let backend = Backend::new_test_with_all_stubs(
        HashMap::from([("PDO", EMBEDDED_PDO), ("BackedEnum", EMBEDDED_BACKED_ENUM)]),
        HashMap::from([("array_map", EMBEDDED_STANDARD)]),
        HashMap::from([("PHP_EOL", EMBEDDED_CORE)]),
    );
    *backend.workspace_root().write() = Some(workspace.to_path_buf());
    backend.initialized(InitializedParams {}).await;
    backend
}

/// The single location a go-to-definition response names.
fn location(response: Option<DefinitionResponse>) -> Location {
    match response {
        Some(DefinitionResponse::Definition(Definition::Location(location))) => location,
        other => panic!("expected a single location, got: {other:?}"),
    }
}

fn assert_lands_in(response: Option<DefinitionResponse>, file: &Path, line: u32) {
    let location = location(response);
    let expected = Uri::from_file_path(file.canonicalize().unwrap()).unwrap();
    assert_eq!(location.uri, expected, "jumped to the wrong file");
    assert_eq!(location.range.start.line, line, "jumped to the wrong line");
}

async fn open_app(backend: &Backend, workspace: &Path) -> Uri {
    let uri = Uri::from_file_path(workspace.join("app.php")).unwrap();
    open_php(backend, &uri, APP).await;
    uri
}

#[tokio::test]
async fn class_jumps_to_the_installed_stub() {
    let dir = tempfile::tempdir().unwrap();
    install_stubs(dir.path());
    let backend = backend_for(dir.path()).await;
    let uri = open_app(&backend, dir.path()).await;

    let pdo = stubs_root(dir.path()).join("PDO/PDO.php");
    assert_lands_in(goto_definition_at(&backend, &uri, 1, 12).await, &pdo, 7);
    // Class names are case-insensitive.
    assert_lands_in(goto_definition_at(&backend, &uri, 6, 5).await, &pdo, 7);
}

#[tokio::test]
async fn members_jump_to_the_installed_stub() {
    let dir = tempfile::tempdir().unwrap();
    install_stubs(dir.path());
    let backend = backend_for(dir.path()).await;
    let uri = open_app(&backend, dir.path()).await;

    let pdo = stubs_root(dir.path()).join("PDO/PDO.php");
    assert_lands_in(goto_definition_at(&backend, &uri, 2, 9).await, &pdo, 11);
    assert_lands_in(goto_definition_at(&backend, &uri, 3, 12).await, &pdo, 9);
}

#[tokio::test]
async fn property_jumps_to_the_installed_stub_not_a_parameter_of_its_name() {
    let dir = tempfile::tempdir().unwrap();
    install_stubs(dir.path());
    let backend = backend_for(dir.path()).await;
    let uri = Uri::from_file_path(dir.path().join("enum.php")).unwrap();
    let text = "<?php\nfunction value($value) { return $value; }\nfunction f(\\BackedEnum $e) {\n    return $e->value;\n}\n";
    open_php(&backend, &uri, text).await;

    let core = stubs_root(dir.path()).join("Core/Core_c.php");
    assert_lands_in(goto_definition_at(&backend, &uri, 3, 16).await, &core, 8);
}

#[tokio::test]
async fn type_definition_jumps_to_the_installed_stub() {
    let dir = tempfile::tempdir().unwrap();
    install_stubs(dir.path());
    let backend = backend_for(dir.path()).await;
    let uri = open_app(&backend, dir.path()).await;

    let response = backend
        .goto_type_definition(TypeDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 2,
                    character: 2,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .map(|response| match response {
            TypeDefinitionResponse::Definition(definition) => {
                DefinitionResponse::Definition(definition)
            }
            other => panic!("expected a definition, got: {other:?}"),
        });
    let pdo = stubs_root(dir.path()).join("PDO/PDO.php");
    assert_lands_in(response, &pdo, 7);
}

#[tokio::test]
async fn function_jumps_to_the_installed_stub() {
    let dir = tempfile::tempdir().unwrap();
    install_stubs(dir.path());
    let backend = backend_for(dir.path()).await;
    let uri = open_app(&backend, dir.path()).await;

    let standard = stubs_root(dir.path()).join("standard/standard_9.php");
    assert_lands_in(goto_definition_at(&backend, &uri, 4, 3).await, &standard, 5);
}

#[tokio::test]
async fn constant_jumps_to_the_installed_stub() {
    let dir = tempfile::tempdir().unwrap();
    install_stubs(dir.path());
    let backend = backend_for(dir.path()).await;
    let uri = open_app(&backend, dir.path()).await;

    let core = stubs_root(dir.path()).join("Core/Core_d.php");
    assert_lands_in(goto_definition_at(&backend, &uri, 5, 7).await, &core, 5);
}

#[tokio::test]
async fn built_ins_have_no_definition_without_the_package() {
    let dir = tempfile::tempdir().unwrap();
    let backend = backend_for(dir.path()).await;
    let uri = open_app(&backend, dir.path()).await;

    for (line, character) in [(1, 12), (2, 9), (3, 12), (4, 3), (5, 7)] {
        let response = goto_definition_at(&backend, &uri, line, character).await;
        assert!(
            response.is_none(),
            "line {line}: a built-in has nowhere to jump to, got: {response:?}"
        );
    }
}

#[tokio::test]
async fn type_resolution_uses_the_installed_stubs_over_the_embedded_ones() {
    let dir = tempfile::tempdir().unwrap();
    install_stubs(dir.path());
    let backend = backend_for(dir.path()).await;
    let uri = Uri::from_file_path(dir.path().join("enum.php")).unwrap();
    // Only the installed `BackedEnum` declares `from()`.
    let text = "<?php\nfunction f(\\BackedEnum $e) {\n    return $e::from(1);\n}\n";
    open_php(&backend, &uri, text).await;

    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri.as_str(), text, &mut diags);
    assert!(diags.is_empty(), "from() is declared: {diags:?}");
    let core = stubs_root(dir.path()).join("Core/Core_c.php");
    assert_lands_in(goto_definition_at(&backend, &uri, 2, 16).await, &core, 4);
}

#[tokio::test]
async fn a_built_in_the_installed_stubs_lack_still_resolves() {
    let dir = tempfile::tempdir().unwrap();
    install_stubs(dir.path());
    write(&dir.path().join("composer.json"), "{}");
    let backend = Backend::new_test_with_all_stubs(
        HashMap::from([("PDO", EMBEDDED_PDO), ("Newer", "<?php\nclass Newer {}\n")]),
        HashMap::new(),
        HashMap::new(),
    );
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());
    backend.initialized(InitializedParams {}).await;
    let uri = Uri::from_file_path(dir.path().join("newer.php")).unwrap();
    let text = "<?php\nnew Newer();\nnew PDO('sqlite::memory:');\n";
    open_php(&backend, &uri, text).await;

    // An older installed copy has no `Newer`, so the embedded one answers.
    let mut diags = Vec::new();
    backend.collect_unknown_class_diagnostics(uri.as_str(), text, &mut diags);
    assert!(diags.is_empty(), "both classes resolve: {diags:?}");
}

#[tokio::test]
async fn removing_the_package_mid_session_stops_the_jump() {
    let dir = tempfile::tempdir().unwrap();
    install_stubs(dir.path());
    let backend = backend_for(dir.path()).await;
    let uri = open_app(&backend, dir.path()).await;

    let pdo = stubs_root(dir.path()).join("PDO/PDO.php");
    assert_lands_in(goto_definition_at(&backend, &uri, 1, 12).await, &pdo, 7);

    // `composer remove jetbrains/phpstorm-stubs`.
    fs::remove_dir_all(stubs_root(dir.path())).unwrap();
    write(
        &dir.path().join("vendor/composer/installed.json"),
        r#"{ "packages": [] }"#,
    );
    let lock = dir.path().join("composer.lock");
    write(&lock, "{}");
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Uri::from_file_path(&lock).unwrap(),
                kind: FileChangeType::Changed,
            }],
        })
        .await;

    let response = goto_definition_at(&backend, &uri, 1, 12).await;
    assert!(
        response.is_none(),
        "the removed package's files must not be jumped to, got: {response:?}"
    );
}

#[tokio::test]
async fn updating_the_package_mid_session_follows_the_new_map() {
    let dir = tempfile::tempdir().unwrap();
    install_stubs(dir.path());
    let backend = backend_for(dir.path()).await;
    let uri = open_app(&backend, dir.path()).await;

    let old = stubs_root(dir.path()).join("PDO/PDO.php");
    assert_lands_in(goto_definition_at(&backend, &uri, 1, 12).await, &old, 7);

    // `composer update jetbrains/phpstorm-stubs` moves the class.
    let root = stubs_root(dir.path());
    fs::remove_file(&old).unwrap();
    let moved = root.join("pdo_v2/PDO.php");
    write(
        &moved,
        &format!("<?php\n\n{}", &INSTALLED_PDO["<?php\n".len()..]),
    );
    write(
        &root.join("PhpStormStubsMap.php"),
        &STUBS_MAP.replace("'PDO/PDO.php'", "'pdo_v2/PDO.php'"),
    );
    let lock = dir.path().join("composer.lock");
    write(&lock, "{}");
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Uri::from_file_path(&lock).unwrap(),
                kind: FileChangeType::Changed,
            }],
        })
        .await;

    assert_lands_in(goto_definition_at(&backend, &uri, 1, 12).await, &moved, 8);
}
