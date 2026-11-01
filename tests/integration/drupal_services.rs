//! Drupal's services: the IDs a module's, core's, or the site's
//! `*.services.yml` declares, read like Symfony's container configuration
//! but found where Drupal keeps them.

use crate::common::{
    completion_response, hover_text_at, initialize_with_options, offset_position, open_document,
    response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "drupal/core-recommended": "^11" },
    "extra": { "drupal-scaffold": { "locations": { "web-root": "web/" } } }
}"#;

/// Just enough of the container interfaces, where Composer would put core.
const STUBS: &[(&str, &str)] = &[
    (
        "web/core/lib/Psr/Container/ContainerInterface.php",
        "<?php\nnamespace Psr\\Container;\ninterface ContainerInterface {\n    public function get(string $id): mixed;\n    public function has(string $id): bool;\n}\n",
    ),
    (
        "web/core/lib/Symfony/ContainerInterface.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection;\ninterface ContainerInterface extends \\Psr\\Container\\ContainerInterface {}\n",
    ),
    (
        "web/core/lib/Drupal.php",
        "<?php\nclass Drupal {\n    public static function service(string $id): object {}\n    public static function hasService(string $id): bool {}\n}\n",
    ),
    (
        "web/modules/custom/acme/src/Greeter.php",
        "<?php\nnamespace Drupal\\acme;\nclass Greeter {\n    public function greet(): string { return ''; }\n}\n",
    ),
];

const SERVICES: &str = "services:\n  acme.greeter:\n    class: Drupal\\acme\\Greeter\n  acme.friendly_greeter:\n    arguments: ['@acme.greeter']\n    class: Drupal\\acme\\Greeter\n";

const CONSUMER: &str = r#"<?php
namespace Drupal\acme;

use Symfony\Component\DependencyInjection\ContainerInterface;

final class Consumer {
    public static function create(ContainerInterface $container): void {
        $greeter = $container->get('acme.greeter');
        $greeter;
    }
}
"#;

const SERVICES_PATH: &str = "web/modules/custom/acme/acme.services.yml";
const CONSUMER_PATH: &str = "web/modules/custom/acme/src/Consumer.php";

/// An initialized Drupal project holding the stubs and `files`, every one
/// of `files` open.
async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    workspace_in(tempfile::tempdir().unwrap(), files).await
}

/// [`workspace`] in `dir`, which may hold files already.
async fn workspace_in(
    dir: tempfile::TempDir,
    files: &[(&str, &str)],
) -> (Backend, tempfile::TempDir) {
    std::fs::write(dir.path().join("composer.json"), COMPOSER).unwrap();
    for (path, content) in STUBS.iter().chain(files) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    for (path, content) in files {
        let language = if path.ends_with(".php") {
            "php"
        } else {
            "yaml"
        };
        open_document(&backend, &uri_for(&dir, path), language, content).await;
    }
    (backend, dir)
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dir.path().join(rel)).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

#[tokio::test]
async fn a_container_lookup_is_typed_by_the_declared_class() {
    let (backend, dir) = workspace(&[(SERVICES_PATH, SERVICES), (CONSUMER_PATH, CONSUMER)]).await;
    let position = position_in(CONSUMER, "$greeter;", 2);
    let hover = hover_text_at(
        &backend,
        &uri_for(&dir, CONSUMER_PATH),
        position.line,
        position.character,
    )
    .await
    .unwrap_or_default();
    assert!(hover.contains("Greeter"), "{hover}");
}

#[tokio::test]
async fn a_container_lookup_goes_to_the_services_file() {
    let (backend, dir) = workspace(&[(SERVICES_PATH, SERVICES), (CONSUMER_PATH, CONSUMER)]).await;
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_for(&dir, CONSUMER_PATH),
                },
                position: position_in(CONSUMER, "get('acme.greeter')", 7),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let locations = match response {
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(locations.len(), 1, "{locations:?}");
    assert_eq!(locations[0].uri, uri_for(&dir, SERVICES_PATH));
    assert_eq!(locations[0].range.start.line, 1);
}

#[tokio::test]
async fn service_ids_complete_in_php_and_in_the_services_file() {
    let consumer = CONSUMER.replace("get('acme.greeter')", "get('acme.')");
    let yaml = format!("{SERVICES}  acme.other:\n    arguments: ['@acme.']\n");
    let (backend, dir) = workspace(&[
        (SERVICES_PATH, yaml.as_str()),
        (CONSUMER_PATH, consumer.as_str()),
    ])
    .await;

    let position = position_in(&consumer, "get('acme.')", 10);
    let labels = response_labels(
        completion_response(
            &backend,
            &uri_for(&dir, CONSUMER_PATH),
            position.line,
            position.character,
            None,
        )
        .await,
    );
    assert!(labels.contains(&"acme.greeter".to_string()), "{labels:?}");

    let position = position_in(&yaml, "'@acme.']", 7);
    let labels = response_labels(
        completion_response(
            &backend,
            &uri_for(&dir, SERVICES_PATH),
            position.line,
            position.character,
            None,
        )
        .await,
    );
    assert!(labels.contains(&"acme.greeter".to_string()), "{labels:?}");
}

/// Every reference to the name at `position`, declarations included, as
/// files and lines.
async fn references_at(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32)> {
    let mut found: Vec<(Uri, u32)> = backend
        .references(ReferenceParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .map(|location| (location.uri, location.range.start.line))
        .collect();
    found.sort();
    found
}

#[tokio::test]
async fn references_reach_the_lookup_and_the_yaml_argument() {
    let (backend, dir) = workspace(&[(SERVICES_PATH, SERVICES), (CONSUMER_PATH, CONSUMER)]).await;
    let found = references_at(
        &backend,
        &uri_for(&dir, SERVICES_PATH),
        position_in(SERVICES, "acme.greeter:", 3),
    )
    .await;
    let yaml = uri_for(&dir, SERVICES_PATH);
    let php = uri_for(&dir, CONSUMER_PATH);
    let mut expected = vec![(yaml.clone(), 1), (yaml, 4), (php, 7)];
    expected.sort();
    assert_eq!(found, expected);
}

/// Core sits in a folder the project's `.gitignore` leaves out, and its
/// test modules declare services no real site has.
#[tokio::test]
async fn core_services_are_read_and_test_modules_are_not() {
    let core_services =
        "services:\n  config.factory:\n    class: Drupal\\Core\\Config\\ConfigFactory\n";
    let fixture_services =
        "services:\n  config.fixture:\n    class: Drupal\\Core\\Config\\ConfigFactory\n";
    let consumer = CONSUMER.replace(
        "$greeter = $container->get('acme.greeter');",
        "$greeter = $container->get('config.factory');\n        $container->get('config.fixture');",
    );
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in [
        // A repository whose `.gitignore` leaves core out, as a Composer
        // install of Drupal does.
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        ("web/core/core.services.yml", core_services),
        (
            "web/core/modules/system/tests/modules/fixture/fixture.services.yml",
            fixture_services,
        ),
        (
            "web/core/lib/Drupal/Core/Config/ConfigFactory.php",
            "<?php\nnamespace Drupal\\Core\\Config;\nclass ConfigFactory {}\n",
        ),
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_in(dir, &[(CONSUMER_PATH, consumer.as_str())]).await;
    let php = uri_for(&dir, CONSUMER_PATH);

    // Finding references indexes the workspace, core's YAML included.
    let found = references_at(
        &backend,
        &php,
        position_in(&consumer, "get('config.factory')", 7),
    )
    .await;
    assert!(
        found.contains(&(uri_for(&dir, "web/core/core.services.yml"), 1)),
        "{found:?}"
    );

    let position = position_in(&consumer, "$greeter;", 2);
    let hover = hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("ConfigFactory"), "{hover}");

    let found = references_at(
        &backend,
        &php,
        position_in(&consumer, "get('config.fixture')", 7),
    )
    .await;
    assert!(
        found.iter().all(|(uri, _)| *uri == php),
        "a test module's services file was read: {found:?}"
    );
}

const STATIC_CONSUMER: &str = r#"<?php
namespace Drupal\acme;

final class StaticConsumer {
    public function run(): void {
        $greeter = \Drupal::service('acme.greeter');
        $greeter;
        \Drupal::hasService('acme.greeter');
    }
}
"#;

const STATIC_CONSUMER_PATH: &str = "web/modules/custom/acme/src/StaticConsumer.php";

/// The definitions of the name at `position`.
async fn definitions_at(backend: &Backend, uri: &Uri, position: Position) -> Vec<Location> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    }
}

#[tokio::test]
async fn drupal_service_is_typed_and_navigates_like_a_container_lookup() {
    let (backend, dir) = workspace(&[
        (SERVICES_PATH, SERVICES),
        (STATIC_CONSUMER_PATH, STATIC_CONSUMER),
    ])
    .await;
    let php = uri_for(&dir, STATIC_CONSUMER_PATH);

    let position = position_in(STATIC_CONSUMER, "$greeter;", 2);
    let hover = hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Greeter"), "{hover}");

    for needle in ["service('acme.greeter')", "hasService('acme.greeter')"] {
        let inside = needle.find('\'').unwrap() + 2;
        let found =
            definitions_at(&backend, &php, position_in(STATIC_CONSUMER, needle, inside)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(found[0].uri, uri_for(&dir, SERVICES_PATH));
    }
}

/// A file the editor opens before the workspace walk finds the Drupal site
/// is read again once it does.
#[tokio::test]
async fn a_file_opened_before_the_walk_ends_reads_drupal_names() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("composer.json"), COMPOSER).unwrap();
    for (path, content) in STUBS.iter().chain(&[
        (SERVICES_PATH, SERVICES),
        (STATIC_CONSUMER_PATH, STATIC_CONSUMER),
    ]) {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let backend = Backend::new_test();
    #[allow(deprecated)] // `root_uri` is what this generation of clients sends
    let params = InitializeParams {
        root_uri: Some(Uri::from_file_path(dir.path()).unwrap()),
        ..Default::default()
    };
    backend.initialize(params).await.unwrap();
    let php = uri_for(&dir, STATIC_CONSUMER_PATH);
    open_document(&backend, &php, "php", STATIC_CONSUMER).await;
    backend.initialized(InitializedParams {}).await;
    // A test backend runs no background index, which would read it.
    open_document(&backend, &uri_for(&dir, SERVICES_PATH), "yaml", SERVICES).await;

    let found = definitions_at(
        &backend,
        &php,
        position_in(STATIC_CONSUMER, "service('acme.greeter')", 10),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].uri, uri_for(&dir, SERVICES_PATH));
}

#[tokio::test]
async fn drupal_service_ids_complete() {
    let consumer = STATIC_CONSUMER.replace("service('acme.greeter')", "service('acme.')");
    let (backend, dir) = workspace(&[
        (SERVICES_PATH, SERVICES),
        (STATIC_CONSUMER_PATH, consumer.as_str()),
    ])
    .await;
    let position = position_in(&consumer, "service('acme.')", 9);
    let labels = response_labels(
        completion_response(
            &backend,
            &uri_for(&dir, STATIC_CONSUMER_PATH),
            position.line,
            position.character,
            None,
        )
        .await,
    );
    assert!(labels.contains(&"acme.greeter".to_string()), "{labels:?}");
}

#[tokio::test]
async fn a_drupal_class_in_another_namespace_names_no_service() {
    let consumer = r#"<?php
namespace Other;

class Drupal {
    public static function service(string $id): object {}
}

function run(): void {
    Drupal::service('acme.greeter');
}
"#;
    let path = "web/modules/custom/acme/src/Other.php";
    let (backend, dir) = workspace(&[(SERVICES_PATH, SERVICES), (path, consumer)]).await;
    let found = definitions_at(
        &backend,
        &uri_for(&dir, path),
        position_in(consumer, "service('acme.greeter')", 10),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn rename_leaves_core_alone() {
    let consumer = r#"<?php
namespace Drupal\acme;

final class Renamed {
    public function run(): Greeter {
        \Drupal::service('config.factory');
        return new Greeter();
    }
}
"#;
    let core_services =
        "services:\n  config.factory:\n    class: Drupal\\Core\\Config\\ConfigFactory\n";
    let path = "web/modules/custom/acme/src/Renamed.php";
    let (backend, dir) = workspace(&[
        ("web/core/core.services.yml", core_services),
        (SERVICES_PATH, SERVICES),
        (path, consumer),
    ])
    .await;
    let php = uri_for(&dir, path);

    let core_class = position_in(consumer, "Drupal::service", 2);
    assert!(
        crate::common::prepare_rename(&backend, &php, core_class.line, core_class.character)
            .await
            .is_none(),
        "the core class \\Drupal was offered for rename"
    );
    let core_service = position_in(consumer, "config.factory", 2);
    assert!(
        crate::common::prepare_rename(&backend, &php, core_service.line, core_service.character)
            .await
            .is_none(),
        "a core service ID was offered for rename"
    );
    let custom_class = position_in(consumer, "new Greeter", 5);
    assert!(
        crate::common::prepare_rename(&backend, &php, custom_class.line, custom_class.character)
            .await
            .is_some(),
        "a custom class could not be renamed"
    );
}
