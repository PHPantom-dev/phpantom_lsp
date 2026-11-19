//! The methods a service definition's `calls:` names, in Symfony's YAML and
//! XML services files and Drupal's `*.services.yml`.

use crate::common::{
    completion_response, create_psr4_workspace, offset_position, open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const MAILER: &str = "<?php
namespace App;

class BaseMailer {
    public function setLogger(object $logger): void {}
}

class Mailer extends BaseMailer {
    public function setTransport(object $transport): void {}
    public function setRetries(int $retries): void {}
    protected function setSecret(string $secret): void {}
    public static function create(): static {}
    public function __construct() {}
}
";

const SERVICES_YAML: &str = "services:
    App\\Mailer:
        calls:
            - [setLogger, ['@logger']]
            - setTransport: ['@mailer.transport']
    mailer.copy:
        class: App\\Mailer
        calls:
            - method: setRetries
              arguments: [3]
";

const SERVICES_XML: &str = r#"<container>
  <services>
    <service id="mailer.xml" class="App\Mailer">
      <call method="setTransport"/>
    </service>
  </services>
</container>
"#;

async fn symfony_workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = [("src/Mailer.php", MAILER)]
        .into_iter()
        .chain(files.iter().copied())
        .collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in all {
        let language = if path.ends_with(".php") {
            "php"
        } else if path.ends_with(".xml") {
            "xml"
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

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32)> {
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
    let locations = match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    };
    locations
        .into_iter()
        .map(|location| (location.uri, location.range.start.line))
        .collect()
}

async fn references(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32)> {
    let mut found: Vec<(Uri, u32)> = backend
        .references(ReferenceParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: false,
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

async fn completion_labels(backend: &Backend, uri: &Uri, position: Position) -> Vec<String> {
    let mut labels = response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    );
    labels.sort();
    labels
}

#[tokio::test]
async fn a_called_method_goes_to_its_declaration() {
    let (backend, dir) = symfony_workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("config/services.xml", SERVICES_XML),
    ])
    .await;
    let php = uri_for(&dir, "src/Mailer.php");
    let yaml = uri_for(&dir, "config/services.yaml");
    let xml = uri_for(&dir, "config/services.xml");

    // Keyed by its class, and on a parent of it.
    let found = definitions(&backend, &yaml, position_in(SERVICES_YAML, "setLogger", 3)).await;
    assert_eq!(found, vec![(php.clone(), 4)]);
    let found = definitions(
        &backend,
        &yaml,
        position_in(SERVICES_YAML, "setTransport", 3),
    )
    .await;
    assert_eq!(found, vec![(php.clone(), 8)]);
    // Named by its `class:`.
    let found = definitions(&backend, &yaml, position_in(SERVICES_YAML, "setRetries", 3)).await;
    assert_eq!(found, vec![(php.clone(), 9)]);

    let found = definitions(&backend, &xml, position_in(SERVICES_XML, "setTransport", 3)).await;
    assert_eq!(found, vec![(php, 8)]);
}

#[tokio::test]
async fn a_called_method_counts_the_calls_among_its_references() {
    let (backend, dir) = symfony_workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("config/services.xml", SERVICES_XML),
    ])
    .await;
    let php = uri_for(&dir, "src/Mailer.php");
    let yaml = uri_for(&dir, "config/services.yaml");
    let xml = uri_for(&dir, "config/services.xml");

    let found = references(&backend, &php, position_in(MAILER, "setTransport", 3)).await;
    assert_eq!(found, vec![(xml, 3), (yaml, 4)]);
}

#[tokio::test]
async fn a_call_completes_the_public_methods_of_the_class() {
    let yaml = "services:\n    App\\Mailer:\n        calls:\n            - [set\n";
    let (backend, dir) = symfony_workspace(&[("config/services.yaml", yaml)]).await;
    let uri = uri_for(&dir, "config/services.yaml");

    let labels = completion_labels(&backend, &uri, position_in(yaml, "[set", 4)).await;
    assert_eq!(labels, vec!["setLogger", "setRetries", "setTransport"]);
}

#[tokio::test]
async fn a_drupal_services_file_navigates_and_completes_its_calls() {
    let greeter = "<?php\nnamespace Drupal\\acme;\nclass Greeter {\n    public function setGreeting(string $greeting): void {}\n}\n";
    let services = "services:\n  acme.greeter:\n    class: Drupal\\acme\\Greeter\n    calls:\n      - [setGreeting, ['hello']]\n      - [set]\n";
    let (backend, dir) = crate::drupal_services::workspace(&[
        ("web/modules/custom/acme/src/Greeter.php", greeter),
        ("web/modules/custom/acme/acme.services.yml", services),
    ])
    .await;
    let uri = uri_for(&dir, "web/modules/custom/acme/acme.services.yml");
    let php = uri_for(&dir, "web/modules/custom/acme/src/Greeter.php");

    let found = definitions(&backend, &uri, position_in(services, "setGreeting", 3)).await;
    assert_eq!(found, vec![(php, 3)]);

    let labels = completion_labels(&backend, &uri, position_in(services, "[set]", 4)).await;
    assert_eq!(labels, vec!["setGreeting"]);
}
