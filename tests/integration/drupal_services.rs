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
    let dir = tempfile::tempdir().unwrap();
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

#[tokio::test]
async fn references_reach_the_lookup_and_the_yaml_argument() {
    let (backend, dir) = workspace(&[(SERVICES_PATH, SERVICES), (CONSUMER_PATH, CONSUMER)]).await;
    let mut found: Vec<(Uri, u32)> = backend
        .references(ReferenceParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_for(&dir, SERVICES_PATH),
                },
                position: position_in(SERVICES, "acme.greeter:", 3),
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
    let yaml = uri_for(&dir, SERVICES_PATH);
    let php = uri_for(&dir, CONSUMER_PATH);
    let mut expected = vec![(yaml.clone(), 1), (yaml, 4), (php, 7)];
    expected.sort();
    assert_eq!(found, expected);
}
