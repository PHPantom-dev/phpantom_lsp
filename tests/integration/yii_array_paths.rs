//! Key paths in `yiisoft/arrays`: the keys `ArrayHelper` reads complete
//! from the value the keys before them reach, a key that reads a property
//! or calls a method jumps to it, and the getters are typed by the value
//! the path reaches.
use crate::common::{
    completion_response, hover_on, initialize_with_options, offset_position, open_document,
    response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/arrays": "^3.0"
    },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Yiisoft\\Arrays\\": "stubs/arrays/"
        }
    }
}"#;

const ARRAY_HELPER_PHP: &str = r#"<?php
namespace Yiisoft\Arrays;

final class ArrayHelper
{
    public static function getValue(array|object $array, array|\Closure|float|int|string $key, mixed $default = null): mixed { return $default; }
    public static function getValueByPath(array|object $array, array|\Closure|float|int|string $path, mixed $default = null, string $delimiter = '.'): mixed { return $default; }
    public static function setValueByPath(array &$array, array|float|int|string|null $path, mixed $value, string $delimiter = '.'): void {}
    public static function remove(array &$array, array|float|int|string $key, mixed $default = null): mixed { return $default; }
    public static function keyExists(array $array, array|float|int|string $key, bool $caseSensitive = true): bool { return true; }
}
"#;

const USER_PHP: &str = r#"<?php
namespace App;

final class User
{
    public Profile $profile;
    /** @var array{theme: string, size?: int} */
    public array $settings = [];
    private string $secret = '';

    public function name(): string { return ''; }
}
"#;

const PROFILE_PHP: &str = r#"<?php
namespace App;

final class Profile
{
    public ?Address $address = null;
    public string $bio = '';
}
"#;

const ADDRESS_PHP: &str = r#"<?php
namespace App;

final class Address
{
    public string $city = '';
}
"#;

const REPORT_PHP: &str = r#"<?php
namespace App;

use Yiisoft\Arrays\ArrayHelper;

final class Report
{
    /**
     * @param array{user: User, tags: list<string>, meta?: array{count: int}} $data
     */
    public function run(User $user, array $data): void
    {
        $city = ArrayHelper::getValueByPath($user, 'profile.address.city');
        $bio = ArrayHelper::getValue($user, ['profile', 'bio']);
        $theme = ArrayHelper::getValueByPath($data, 'user.settings.theme');
        $count = ArrayHelper::getValueByPath($data, 'meta.count', false);
        $name = ArrayHelper::getValue($user, 'name()');
        $tag = ArrayHelper::getValue($data, ['tags', 0]);
        $removed = ArrayHelper::remove($data, 'user');
        $slashed = ArrayHelper::getValueByPath($user, 'profile/bio', delimiter: '/');
        $unsplit = ArrayHelper::getValue($user, 'profile.bio');
        $chained = ArrayHelper::getValue($user, 'profile')->bio;
    }

    /**
     * @param array{user: User, tags: list<string>, meta?: array{count: int}} $data
     */
    public function write(array $data): void
    {
        ArrayHelper::setValueByPath($data, 'user.settings.size', 3);
    }
}
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("stubs/arrays/ArrayHelper.php", ARRAY_HELPER_PHP),
        ("src/User.php", USER_PHP),
        ("src/Profile.php", PROFILE_PHP),
        ("src/Address.php", ADDRESS_PHP),
        ("src/Report.php", REPORT_PHP),
    ]
}

/// A session over the workspace with its PHP files open.
async fn workspace() -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files() {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    for (path, content) in files() {
        if path.ends_with(".php") && !path.starts_with("stubs/") {
            open_document(&backend, &uri_for(&dir, path), "php", content).await;
        }
    }
    (backend, dir)
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dunce::canonicalize(dir.path()).unwrap().join(rel)).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

/// The labels completion offers in `content` once `typed` replaces
/// `needle`, with the cursor `inside` bytes into what was typed.
async fn completions(
    backend: &Backend,
    dir: &tempfile::TempDir,
    needle: &str,
    typed: &str,
    inside: usize,
) -> Vec<String> {
    let uri = uri_for(dir, "src/Report.php");
    let content = REPORT_PHP.replacen(needle, typed, 1);
    open_document(backend, &uri, "php", &content).await;
    let position = position_in(&content, typed, inside);
    let labels = response_labels(
        completion_response(backend, &uri, position.line, position.character, None).await,
    );
    open_document(backend, &uri, "php", REPORT_PHP).await;
    labels
}

/// Where go-to-definition from `inside` bytes into `needle` lands.
async fn definition_of(
    backend: &Backend,
    dir: &tempfile::TempDir,
    needle: &str,
    inside: usize,
) -> Option<Location> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_for(dir, "src/Report.php"),
                },
                position: position_in(REPORT_PHP, needle, inside),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    match response {
        Some(DefinitionResponse::Definition(Definition::Location(location))) => Some(location),
        Some(DefinitionResponse::Definition(Definition::LocationList(mut list))) => list.pop(),
        _ => None,
    }
}

#[tokio::test]
async fn the_getters_are_typed_by_the_value_the_path_reaches() {
    let (backend, dir) = workspace().await;
    let uri = uri_for(&dir, "src/Report.php");
    for (variable, expected) in [
        ("$city =", "string|null"),
        ("$bio =", "string"),
        ("$theme =", "string"),
        ("$count =", "int|false"),
        ("$name =", "string"),
        ("$tag =", "string|null"),
        ("$removed =", "User"),
        ("$slashed =", "string"),
        ("$unsplit =", "mixed"),
    ] {
        let hover = hover_on(&backend, &uri, REPORT_PHP, variable).await;
        assert!(
            hover.contains(expected),
            "{variable} should be {expected}, got {hover}"
        );
    }
    let hover = hover_on(&backend, &uri, REPORT_PHP, "->bio;").await;
    assert!(
        hover.contains("Profile"),
        "a member of a getter's result is read on the value it reaches: {hover}"
    );
}

#[tokio::test]
async fn a_segment_completes_from_the_value_the_keys_before_it_reach() {
    let (backend, dir) = workspace().await;
    let needle = "'profile.address.city'";

    let labels = completions(&backend, &dir, needle, "''", 1).await;
    assert!(labels.contains(&"profile".to_string()), "{labels:?}");
    assert!(labels.contains(&"settings".to_string()), "{labels:?}");
    // A method a path calls is written by hand, not offered.
    assert!(!labels.contains(&"name()".to_string()), "{labels:?}");
    assert!(!labels.contains(&"secret".to_string()), "{labels:?}");

    let labels = completions(&backend, &dir, needle, "'profile.'", 9).await;
    assert_eq!(labels, ["address", "bio"]);

    let labels = completions(&backend, &dir, needle, "'profile.address.c'", 18).await;
    assert_eq!(labels, ["city"]);

    let labels = completions(
        &backend,
        &dir,
        "'user.settings.theme'",
        "'user.settings.'",
        15,
    )
    .await;
    assert_eq!(labels, ["theme", "size"]);

    // A list of keys walks its elements before the one being typed.
    let labels = completions(&backend, &dir, "['profile', 'bio']", "['profile', '']", 13).await;
    assert_eq!(labels, ["address", "bio"]);

    // A writer reads arrays only: an object's properties are no keys of it.
    let labels = completions(&backend, &dir, "'user.settings.size'", "'user.'", 6).await;
    assert!(labels.is_empty(), "{labels:?}");
    let labels = completions(&backend, &dir, "'user.settings.size'", "''", 1).await;
    assert_eq!(labels, ["user", "tags", "meta"]);
}

#[tokio::test]
async fn a_segment_jumps_to_the_property_or_method_it_reads() {
    let (backend, dir) = workspace().await;
    let address = uri_for(&dir, "src/Address.php");
    let profile = uri_for(&dir, "src/Profile.php");
    let user = uri_for(&dir, "src/User.php");

    let location = definition_of(&backend, &dir, "'profile.address.city'", 18)
        .await
        .expect("city should resolve");
    assert_eq!(location.uri, address);
    assert_eq!(
        location.range.start.line,
        position_in(ADDRESS_PHP, "$city", 0).line
    );

    let location = definition_of(&backend, &dir, "'profile.address.city'", 10)
        .await
        .expect("address should resolve");
    assert_eq!(location.uri, profile);
    assert_eq!(
        location.range.start.line,
        position_in(PROFILE_PHP, "$address", 0).line
    );

    let location = definition_of(&backend, &dir, "['profile', 'bio']", 14)
        .await
        .expect("bio should resolve");
    assert_eq!(location.uri, profile);

    let location = definition_of(&backend, &dir, "'name()'", 2)
        .await
        .expect("name() should resolve");
    assert_eq!(location.uri, user);
    assert_eq!(
        location.range.start.line,
        position_in(USER_PHP, "function name", 0).line
    );

    // Through an array's key into the object it holds.
    let location = definition_of(&backend, &dir, "'user.settings.theme'", 7)
        .await
        .expect("settings should resolve");
    assert_eq!(location.uri, user);

    // `getValue()` reads its key whole.
    assert!(
        definition_of(&backend, &dir, "'profile.bio'", 10)
            .await
            .is_none()
    );
}
