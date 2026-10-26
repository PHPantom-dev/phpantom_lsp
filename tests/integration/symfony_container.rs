//! Symfony service IDs and container parameters, from their declarations
//! in YAML, XML, and PHP configurators to the PHP that reads them.
use crate::common::message_text;

use crate::common::{
    completion_response, create_psr4_workspace, offset_position, open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Psr\\Container\\": "stubs/psr-container/",
        "Symfony\\Component\\DependencyInjection\\": "stubs/dependency-injection/"
    } }
}"#;

/// Just enough of the container and its configurators for the type engine
/// to tell a container from any other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/psr-container/ContainerInterface.php",
        "<?php\nnamespace Psr\\Container;\ninterface ContainerInterface {\n    public function get(string $id): mixed;\n    public function has(string $id): bool;\n}\n",
    ),
    (
        "stubs/dependency-injection/ContainerInterface.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection;\ninterface ContainerInterface extends \\Psr\\Container\\ContainerInterface {\n    public function getParameter(string $name): mixed;\n}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/ContainerConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ContainerConfigurator {\n    public function services(): ServicesConfigurator {}\n    public function parameters(): ParametersConfigurator {}\n}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/ServicesConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ServicesConfigurator {\n    public function set(?string $id, ?string $class = null): ServiceConfigurator {}\n    public function alias(string $id, string $referencedId): void {}\n}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/ServiceConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ServiceConfigurator {\n    public function decorate(?string $id): static {}\n}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/ParametersConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ParametersConfigurator {\n    public function set(string $name, mixed $value): static {}\n}\n",
    ),
    (
        "stubs/dependency-injection/Attribute/Autowire.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Attribute;\n#[\\Attribute]\nclass Autowire {\n    public function __construct(mixed $value = null, ?string $service = null, ?string $param = null) {}\n}\n",
    ),
];

const SERVICES_YAML: &str = "parameters:
    app.sender_name: PHPantom
services:
    app.mailer:
        class: App\\Service\\Mailer
        arguments: ['%app.sender_name%']
    app.mailer_alias: '@app.mailer'
";

const SERVICES_PHP: &str = r#"<?php
namespace Symfony\Component\DependencyInjection\Loader\Configurator;

return static function (ContainerConfigurator $container): void {
    $services = $container->services();
    $parameters = $container->parameters();

    $services->set('app.php_mailer');
    $services->alias('app.php_mailer_alias', 'app.php_mailer');
    $services->set('app.decorator')->decorate('app.mailer');
    $parameters->set('app.php_sender', 'PHPantom');
    $services->set('app.newsletter')->args([service('app.mailer'), param('app.sender_name')]);
};
"#;

const CONSUMER_PHP: &str = r#"<?php
namespace App\Controller;

use Symfony\Component\DependencyInjection\Attribute\Autowire;
use Symfony\Component\DependencyInjection\ContainerInterface;

final class MailController
{
    public function __construct(
        #[Autowire(param: 'app.sender_name')]
        private string $sender,
        #[Autowire(service: 'app.php_mailer')]
        private object $mailer,
        #[Autowire('%app.sender_name%/%kernel.project_dir%')]
        private string $path,
    ) {}

    public function send(ContainerInterface $container, \ArrayObject $other): void
    {
        $container->get('app.mailer');
        $container->getParameter('app.php_sender');
        $other->offsetGet('app.mailer');
    }
}
"#;

/// A workspace holding the stubs and `files`, every one of them open.
async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = STUBS.iter().chain(files).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in files {
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

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<Location> {
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

fn sorted(mut locations: Vec<(Uri, u32)>) -> Vec<(Uri, u32)> {
    locations.sort();
    locations
}

async fn completion_labels(backend: &Backend, uri: &Uri, position: Position) -> Vec<String> {
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

#[tokio::test]
async fn a_container_lookup_goes_to_the_yaml_declaration() {
    let (backend, dir) = workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("src/Controller/MailController.php", CONSUMER_PHP),
    ])
    .await;
    let yaml = uri_for(&dir, "config/services.yaml");
    let consumer = uri_for(&dir, "src/Controller/MailController.php");

    let found = definitions(
        &backend,
        &consumer,
        position_in(CONSUMER_PHP, "get('app.mailer')", 7),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].uri, yaml);
    assert_eq!(found[0].range.start.line, 3);

    let found = definitions(
        &backend,
        &consumer,
        position_in(CONSUMER_PHP, "param: 'app.sender_name'", 10),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((&found[0].uri, found[0].range.start.line), (&yaml, 1));

    // A placeholder inside an `#[Autowire]` value reads the parameter too.
    let found = definitions(
        &backend,
        &consumer,
        position_in(CONSUMER_PHP, "'%app.sender_name%/", 3),
    )
    .await;
    assert_eq!((&found[0].uri, found[0].range.start.line), (&yaml, 1));

    // `offsetGet()` on something that is not a container names nothing.
    let found = definitions(
        &backend,
        &consumer,
        position_in(CONSUMER_PHP, "offsetGet('app.mailer')", 13),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn references_reach_yaml_php_configurators_and_lookups() {
    let (backend, dir) = workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("config/services.php", SERVICES_PHP),
        ("src/Controller/MailController.php", CONSUMER_PHP),
    ])
    .await;
    let yaml = uri_for(&dir, "config/services.yaml");
    let php_config = uri_for(&dir, "config/services.php");
    let consumer = uri_for(&dir, "src/Controller/MailController.php");

    let found = references(
        &backend,
        &yaml,
        position_in(SERVICES_YAML, "app.mailer:", 4),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![
            (consumer.clone(), 19),
            (php_config.clone(), 9),
            (php_config.clone(), 11),
            (yaml.clone(), 3),
            (yaml.clone(), 6),
        ])
    );

    let found = references(
        &backend,
        &consumer,
        position_in(CONSUMER_PHP, "'app.sender_name'", 3),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![
            (consumer.clone(), 9),
            (consumer.clone(), 13),
            (php_config.clone(), 11),
            (yaml.clone(), 1),
            (yaml.clone(), 5),
        ])
    );
}

#[tokio::test]
async fn php_configurators_declare_services_and_parameters() {
    let (backend, dir) = workspace(&[
        ("config/services.php", SERVICES_PHP),
        ("src/Controller/MailController.php", CONSUMER_PHP),
    ])
    .await;
    let php_config = uri_for(&dir, "config/services.php");
    let consumer = uri_for(&dir, "src/Controller/MailController.php");

    // The alias names the service the line above declares.
    let found = definitions(
        &backend,
        &php_config,
        position_in(SERVICES_PHP, "'app.php_mailer');", 3),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((&found[0].uri, found[0].range.start.line), (&php_config, 7));

    let found = definitions(
        &backend,
        &consumer,
        position_in(CONSUMER_PHP, "getParameter('app.php_sender')", 15),
    )
    .await;
    assert_eq!(
        (&found[0].uri, found[0].range.start.line),
        (&php_config, 10)
    );

    let found = definitions(
        &backend,
        &consumer,
        position_in(CONSUMER_PHP, "service: 'app.php_mailer'", 12),
    )
    .await;
    assert_eq!((&found[0].uri, found[0].range.start.line), (&php_config, 7));
}

#[tokio::test]
async fn an_xml_alias_goes_to_the_service_it_names() {
    let services_xml = r#"<?xml version="1.0"?>
<container>
  <parameters>
    <parameter key="app.sender_name">PHPantom</parameter>
  </parameters>
  <services>
    <service id="app.mailer" class="App\Service\Mailer"/>
    <service id="app.mailer_alias" alias="app.mailer"/>
  </services>
</container>
"#;
    let (backend, dir) = workspace(&[("config/services.xml", services_xml)]).await;
    let xml = uri_for(&dir, "config/services.xml");

    let found = definitions(
        &backend,
        &xml,
        position_in(services_xml, "alias=\"app.mailer\"", 10),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((&found[0].uri, found[0].range.start.line), (&xml, 6));
}

#[tokio::test]
async fn service_ids_and_parameters_complete_from_the_declarations() {
    let consumer = r#"<?php
use Symfony\Component\DependencyInjection\Attribute\Autowire;
use Symfony\Component\DependencyInjection\ContainerInterface;

function send(ContainerInterface $container, \ArrayObject $other): void {
    $container->get('app.m');
    $other->offsetGet('app.m');
}

final class Consumer {
    public function __construct(#[Autowire(param: 'app.s')] string $sender) {}
}
"#;
    let yaml_with_cursor = "services:\n    app.other:\n        arguments: ['@app.m']\n";
    let (backend, dir) = workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("config/other.yaml", yaml_with_cursor),
        ("src/consumer.php", consumer),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/consumer.php");
    let other_yaml = uri_for(&dir, "config/other.yaml");

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "get('app.m'", 10),
    )
    .await;
    assert!(
        labels.contains(&"app.mailer".to_string())
            && labels.contains(&"app.mailer_alias".to_string()),
        "{labels:?}"
    );
    assert!(
        !labels.contains(&"app.sender_name".to_string()),
        "{labels:?}"
    );

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "offsetGet('app.m'", 16),
    )
    .await;
    assert!(!labels.contains(&"app.mailer".to_string()), "{labels:?}");

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "param: 'app.s'", 13),
    )
    .await;
    assert_eq!(labels, vec!["app.sender_name".to_string()]);

    let labels = completion_labels(
        &backend,
        &other_yaml,
        position_in(yaml_with_cursor, "'@app.m'", 7),
    )
    .await;
    assert!(labels.contains(&"app.mailer".to_string()), "{labels:?}");
}

#[tokio::test]
async fn only_undeclared_application_names_are_reported() {
    let consumer = r#"<?php
use Symfony\Component\DependencyInjection\ContainerInterface;

function send(ContainerInterface $container): void {
    $container->get('app.mailer');
    $container->get('app.missing');
    $container->getParameter('app.sender_name');
    $container->getParameter('app.missing_parameter');
    $container->get('router');
    $container->getParameter('kernel.project_dir');
    $container->get('App\Service\Missing');
    $container->get('app.php_mailer');
    $container->getParameter('app.php_sender');
}
"#;
    // The last two are declared only by the PHP configurator, whose calls
    // are confirmed from inside this file's diagnostic pass.
    let (backend, dir) = workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("config/services.php", SERVICES_PHP),
        ("src/consumer.php", consumer),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/consumer.php");

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(consumer_uri.as_str(), consumer, &mut diagnostics);
    let mut reported: Vec<String> = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code)) if code.starts_with("invalid_symfony_")
            )
        })
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect();
    reported.sort();
    assert_eq!(
        reported,
        vec![
            "Unknown container parameter: 'app.missing_parameter'".to_string(),
            "Unknown service: 'App\\Service\\Missing'".to_string(),
            "Unknown service: 'app.missing'".to_string(),
        ]
    );
}
