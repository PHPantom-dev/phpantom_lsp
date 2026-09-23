//! PSR-4 lookups follow Composer's own rules where several mappings could
//! place a class: the longest prefix first, the directories of one prefix in
//! the order they are listed, the root fallback mapping last, and
//! `autoload-dev` alongside `autoload`.
//!
//! Cases adapted from laravel-lsp's MIT-licensed test suite.

use std::fs;

use crate::common::{
    ILLUMINATE_COMPONENT_STUB, LIVEWIRE_COMPONENT_STUB, complete_labels_at_opened,
    complete_labels_at_opened_with_trigger, create_psr4_workspace, open_document,
    open_initialized_php, open_php_at, position_after, workspace_uri,
};

/// Open `content` at `path` in a workspace laid out by `composer_json` and
/// `files`, and complete just past the first occurrence of `after`.
async fn complete_after(
    composer_json: &str,
    files: &[(&str, &str)],
    path: &str,
    content: &str,
    after: &str,
) -> Vec<String> {
    let mut all = files.to_vec();
    all.push((path, content));
    let (backend, dir) = create_psr4_workspace(composer_json, &all);
    let uri = open_php_at(&backend, &dir, path, content).await;
    let position = position_after(content, after);
    complete_labels_at_opened(&backend, &uri, position.line, position.character).await
}

fn offers(labels: &[String], name: &str) -> bool {
    labels.iter().any(|label| label.starts_with(name))
}

const USER_CONSUMER: &str = "\
<?php
namespace App;
use App\\Models\\User;
class Consumer {
    public function go(User $u): void {
        $u->
    }
}
";

const SERVICE_CONSUMER: &str = "\
<?php
namespace App;
class Consumer {
    public function go(Service $s): void {
        $s->
    }
}
";

/// Composer tries the most specific prefix first, and a file found there
/// ends the search, even when a shorter prefix would also place the class.
#[tokio::test]
async fn the_more_specific_prefix_wins_when_both_directories_hold_the_class() {
    let composer = r#"{"autoload": {"psr-4": {
        "App\\": "app/",
        "App\\Models\\": "custom/models/"
    }}}"#;
    let labels = complete_after(
        composer,
        &[
            (
                "custom/models/User.php",
                "<?php\nnamespace App\\Models;\nclass User { public function fromCustom(): void {} }\n",
            ),
            (
                "app/Models/User.php",
                "<?php\nnamespace App\\Models;\nclass User { public function fromApp(): void {} }\n",
            ),
        ],
        "app/Consumer.php",
        USER_CONSUMER,
        "$u->",
    )
    .await;
    assert!(
        offers(&labels, "fromCustom"),
        "App\\Models\\ should place User in custom/models/, got: {labels:?}"
    );
    assert!(
        !offers(&labels, "fromApp"),
        "the shorter App\\ prefix must not win, got: {labels:?}"
    );
}

/// The directories of one prefix are tried in the order they are listed,
/// so the first one holding the file wins.
#[tokio::test]
async fn the_first_listed_directory_wins_when_an_array_mapping_finds_the_class_twice() {
    let composer = r#"{"autoload": {"psr-4": {"App\\": ["app/", "src/"]}}}"#;
    let labels = complete_after(
        composer,
        &[
            (
                "app/Service.php",
                "<?php\nnamespace App;\nclass Service { public function fromApp(): void {} }\n",
            ),
            (
                "src/Service.php",
                "<?php\nnamespace App;\nclass Service { public function fromSrc(): void {} }\n",
            ),
        ],
        "app/Consumer.php",
        SERVICE_CONSUMER,
        "$s->",
    )
    .await;
    assert!(
        offers(&labels, "fromApp"),
        "app/ is listed first, got: {labels:?}"
    );
    assert!(
        !offers(&labels, "fromSrc"),
        "src/ must only be reached when app/ has no file, got: {labels:?}"
    );
}

/// A mapping declared only under `autoload-dev` places classes too.
#[tokio::test]
async fn an_autoload_dev_mapping_resolves_its_classes() {
    let composer = r#"{
        "autoload": {"psr-4": {"App\\": "app/"}},
        "autoload-dev": {"psr-4": {"Database\\Factories\\": "database/factories/"}}
    }"#;
    let content = "\
<?php
namespace App;
use Database\\Factories\\UserFactory;
class Consumer {
    public function go(UserFactory $f): void {
        $f->
    }
}
";
    let labels = complete_after(
        composer,
        &[(
            "database/factories/UserFactory.php",
            "<?php\nnamespace Database\\Factories;\nclass UserFactory { public function definition(): array { return []; } }\n",
        )],
        "app/Consumer.php",
        content,
        "$f->",
    )
    .await;
    assert!(
        offers(&labels, "definition"),
        "the autoload-dev mapping should place UserFactory, got: {labels:?}"
    );
}

/// A mapping directory written without its trailing slash still works.
#[tokio::test]
async fn a_mapping_directory_without_a_trailing_slash_resolves() {
    let composer = r#"{"autoload": {"psr-4": {"App\\": "app"}}}"#;
    let labels = complete_after(
        composer,
        &[(
            "app/Models/User.php",
            "<?php\nnamespace App\\Models;\nclass User { public function fromApp(): void {} }\n",
        )],
        "app/Consumer.php",
        USER_CONSUMER,
        "$u->",
    )
    .await;
    assert!(
        offers(&labels, "fromApp"),
        "\"app\" should be read as app/, got: {labels:?}"
    );
}

/// An empty prefix is Composer's root fallback: it places a class of any
/// namespace below its directory.
#[tokio::test]
async fn the_root_fallback_mapping_resolves_a_namespaced_class() {
    let composer = r#"{"autoload": {"psr-4": {"": "lib/"}}}"#;
    let content = "\
<?php
function go(\\Legacy\\Thing $t): void {
    $t->
}
";
    let labels = complete_after(
        composer,
        &[(
            "lib/Legacy/Thing.php",
            "<?php\nnamespace Legacy;\nclass Thing { public function legacyOnly(): void {} }\n",
        )],
        "lib/consumer.php",
        content,
        "$t->",
    )
    .await;
    assert!(
        offers(&labels, "legacyOnly"),
        "the root fallback should place Legacy\\Thing in lib/Legacy/, got: {labels:?}"
    );
}

/// The root fallback is consulted only after every real prefix, so a
/// prefix that places the class wins over it.
#[tokio::test]
async fn a_real_prefix_wins_over_the_root_fallback() {
    let composer = r#"{"autoload": {"psr-4": {"": "lib/", "App\\": "app/"}}}"#;
    let labels = complete_after(
        composer,
        &[
            (
                "app/Service.php",
                "<?php\nnamespace App;\nclass Service { public function fromApp(): void {} }\n",
            ),
            (
                "lib/App/Service.php",
                "<?php\nnamespace App;\nclass Service { public function fromLib(): void {} }\n",
            ),
        ],
        "app/Consumer.php",
        SERVICE_CONSUMER,
        "$s->",
    )
    .await;
    assert!(
        offers(&labels, "fromApp"),
        "App\\ should win over the fallback, got: {labels:?}"
    );
    assert!(
        !offers(&labels, "fromLib"),
        "the fallback must only be reached when App\\ has no file, got: {labels:?}"
    );
}

/// `resolve_class_path` is documented to accept PHP's fully-qualified
/// spelling with its leading backslash.
#[test]
fn resolve_class_path_accepts_a_leading_backslash() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload": {"psr-4": {"App\\": "app/"}}}"#,
    )
    .expect("failed to write composer.json");
    let file = dir.path().join("app/Models/User.php");
    fs::create_dir_all(file.parent().unwrap()).expect("failed to create dirs");
    fs::write(&file, "<?php\nnamespace App\\Models;\nclass User {}\n")
        .expect("failed to write PHP file");

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let resolved =
        phpantom_lsp::composer::resolve_class_path(&mappings, dir.path(), "\\App\\Models\\User");
    assert!(
        resolved
            .as_ref()
            .is_some_and(|path| path.ends_with("app/Models/User.php")),
        "\\App\\Models\\User should resolve like App\\Models\\User, got: {resolved:?}"
    );
}

/// A vendor package's directory name need not match its namespace:
/// `installed.json` says where the package lives, and its own PSR-4 map
/// says what is in it.
#[tokio::test]
async fn a_vendor_package_whose_directory_differs_from_its_namespace_resolves() {
    let installed = r#"{
        "packages": [
            {
                "name": "acme/bible-models",
                "install-path": "../acme/bible-models",
                "autoload": { "psr-4": { "Acme\\BibleModels\\": "src/" } }
            }
        ]
    }"#;
    let consumer = "\
<?php
namespace App;
use Acme\\BibleModels\\Models\\Version;
class Consumer {
    public function go(Version $v): void {
        $v->
    }
}
";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload": {"psr-4": {"App\\": "app/"}}}"#,
        &[
            ("vendor/composer/installed.json", installed),
            (
                "vendor/acme/bible-models/src/Models/Version.php",
                "<?php\nnamespace Acme\\BibleModels\\Models;\nclass Version { public function abbreviation(): string { return ''; } }\n",
            ),
            ("app/Consumer.php", consumer),
        ],
    );
    let uri = open_initialized_php(&backend, "app/Consumer.php").await;
    let position = position_after(consumer, "$v->");
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    assert!(
        offers(&labels, "abbreviation"),
        "the hyphenated package directory should hold Acme\\BibleModels, got: {labels:?}"
    );
}

/// A namespace spread over the directories of an array mapping is found
/// in each of them: component classes under either `app/` or `src/`
/// answer to their tags.
#[tokio::test]
async fn components_in_every_directory_of_an_array_mapping_resolve() {
    let composer = r#"{"autoload": {"psr-4": {
        "App\\": ["app/", "src/"],
        "Illuminate\\": "stubs/Illuminate/",
        "Livewire\\": "stubs/Livewire/"
    }}}"#;
    for (tag, member) in [("alert", "severity"), ("badge", "tone")] {
        let template = format!("<x-{tag}>\n{{{{ $component-> }}}}\n</x-{tag}>\n");
        let (backend, _dir) = create_psr4_workspace(
            composer,
            &[
                (
                    "stubs/Illuminate/View/Component.php",
                    ILLUMINATE_COMPONENT_STUB,
                ),
                ("stubs/Livewire/Component.php", LIVEWIRE_COMPONENT_STUB),
                (
                    "app/View/Components/Alert.php",
                    "<?php\nnamespace App\\View\\Components;\n\
                     use Illuminate\\View\\Component;\n\
                     class Alert extends Component {\n\
                         public function severity(): string { return ''; }\n\
                         public function render() {}\n\
                     }\n",
                ),
                (
                    "src/View/Components/Badge.php",
                    "<?php\nnamespace App\\View\\Components;\n\
                     use Illuminate\\View\\Component;\n\
                     class Badge extends Component {\n\
                         public function tone(): string { return ''; }\n\
                         public function render() {}\n\
                     }\n",
                ),
                ("resources/views/page.blade.php", template.as_str()),
            ],
        );
        let uri = workspace_uri(&backend, "resources/views/page.blade.php");
        open_document(&backend, &uri, "blade", &template).await;
        let labels = complete_labels_at_opened_with_trigger(&backend, &uri, 1, 15, ">").await;
        assert!(
            labels
                .iter()
                .any(|label| label.trim_end_matches("()") == member),
            "<x-{tag}> should resolve to the class declaring {member}, got: {labels:?}"
        );
    }
}
