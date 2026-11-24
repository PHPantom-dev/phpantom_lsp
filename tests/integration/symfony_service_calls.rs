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

#[tokio::test]
async fn service_wiring_resolves_arguments_tags_and_factories_on_their_own_classes() {
    let php = r"<?php
namespace App;
class Builder {
    public function make(string $directory): Product { return new Product($directory); }
    public static function create(): Product { return new Product(''); }
}
class Product {
    public function __construct(string $directory) {}
    public function receive(): void {}
}
class Unrelated {
    public function receive(): void {}
    public function make(): void {}
}";
    let yaml = r"services:
  _defaults:
    bind:
      $directory: default
  builder:
    class: App\Builder
  builder.alias: '@builder'
  App\Product:
    arguments:
      '$directory': product
    tags:
      - { name: kernel.event_listener, method: receive, event: foo }
      - { name: other, method: receive }
  from.factory:
    class: App\Product
    factory: ['@builder.alias', 'make']
    arguments: { $directory: factory }
  still.static:
    factory: 'App\Builder::create'
";
    let xml = r#"<services><service id="App\Product"><tag name="kernel.event_listener" method="receive"/></service></services>"#;
    let (backend, dir) = symfony_workspace(&[
        ("src/Builder.php", php),
        ("config/services.yaml", yaml),
        ("config/services.xml", xml),
    ])
    .await;
    let config = uri_for(&dir, "config/services.yaml");
    let mapping = uri_for(&dir, "config/services.xml");
    let owner = uri_for(&dir, "src/Builder.php");
    for (needle, line) in [
        ("$directory': product", 7),
        ("$directory: factory", 3),
        ("receive, event", 8),
        ("make']", 3),
        ("create'", 4),
    ] {
        let pos = position_in(yaml, needle, 1);
        assert_eq!(
            definitions(&backend, &config, pos).await,
            vec![(owner.clone(), line)],
            "{needle}"
        );
        assert!(
            references(&backend, &config, pos)
                .await
                .contains(&(config.clone(), pos.line)),
            "{needle}"
        );
    }
    let mut defaults = definitions(
        &backend,
        &config,
        position_in(yaml, "$directory: default", 1),
    )
    .await;
    defaults.sort();
    assert_eq!(defaults, vec![(owner.clone(), 3), (owner.clone(), 7)]);
    assert_eq!(
        definitions(&backend, &mapping, position_in(xml, "receive", 1)).await,
        vec![(owner.clone(), 8)]
    );
    assert!(
        definitions(&backend, &config, position_in(yaml, "receive }", 1))
            .await
            .is_empty()
    );
    let refs = references(&backend, &owner, position_in(php, "make(string", 1)).await;
    assert!(refs.contains(&(config.clone(), 15)), "{refs:?}");
    let refs = references(
        &backend,
        &owner,
        position_in(php, "__construct(string $directory)", 21),
    )
    .await;
    for line in [3, 9] {
        assert!(refs.contains(&(config.clone(), line)), "{refs:?}");
    }
    assert!(!refs.contains(&(config.clone(), 16)), "{refs:?}");
    let pos = position_in(php, "__construct(string $directory)", 21);
    assert!(
        crate::common::rename_result(&backend, &owner, pos.line, pos.character, "elsewhere")
            .await
            .ok()
            .flatten()
            .is_none()
    );
    for (needle, expected) in [("make']", "make"), ("create'", "create")] {
        let completions = completion_labels(&backend, &config, position_in(yaml, needle, 2)).await;
        assert!(
            completions.iter().any(|label| label == expected),
            "{completions:?}"
        );
    }
}

#[tokio::test]
async fn default_bindings_follow_the_file_resource_and_respect_its_exclusions() {
    let php = r"<?php namespace App\Feature; class Handler { public function __construct(string $path) {} }";
    let excluded = r"<?php namespace App\Feature; class Hidden { public function __construct(string $path) {} }";
    let yaml = r"services:
  App\Feature\:
    resource: '../src/Feature'
    exclude:
      - '../src/Feature/Hidden.php'
  _defaults:
    bind:
      $path: example
      'string $path': typed
      'int $path': mismatch
when@test:
  services:
    App\Mailer: ~
    _defaults:
      bind: { $path: other }
";
    let (backend, dir) = symfony_workspace(&[
        ("src/Feature/Handler.php", php),
        ("src/Feature/Hidden.php", excluded),
        ("config/services.yaml", yaml),
    ])
    .await;
    let config = uri_for(&dir, "config/services.yaml");
    assert_eq!(
        definitions(&backend, &config, position_in(yaml, "$path: example", 0)).await,
        vec![(uri_for(&dir, "src/Feature/Handler.php"), 0)]
    );
    for (needle, count) in [("$path': typed", 1), ("$path': mismatch", 0)] {
        assert_eq!(
            definitions(&backend, &config, position_in(yaml, needle, 1))
                .await
                .len(),
            count
        );
    }
    assert!(
        definitions(&backend, &config, position_in(yaml, "$path: other", 1))
            .await
            .is_empty()
    );
}
