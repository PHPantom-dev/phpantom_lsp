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
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ServicesConfigurator extends AbstractConfigurator {\n    public function set(?string $id, ?string $class = null): ServiceConfigurator {}\n    public function alias(string $id, string $referencedId): void {}\n}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/ServiceConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ServiceConfigurator extends AbstractConfigurator {\n    public function decorate(?string $id): static {}\n    public function parent(string $parent): static {}\n    public function args(array $arguments): static {}\n}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/AbstractConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nabstract class AbstractConfigurator {}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/ParametersConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ParametersConfigurator extends AbstractConfigurator {\n    public function set(string $name, mixed $value): static {}\n}\n",
    ),
    (
        "stubs/dependency-injection/ContainerBuilder.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection;\nclass ContainerBuilder implements ContainerInterface {\n    public function get(string $id): mixed {}\n    public function has(string $id): bool {}\n    public function getParameter(string $name): mixed {}\n    public function setParameter(string $name, mixed $value): void {}\n    public function register(string $id, ?string $class = null): Definition {}\n    public function setDefinition(string $id, Definition $definition): Definition {}\n    public function setAlias(string $alias, string $id): void {}\n    public function getDefinition(string $id): Definition {}\n}\n",
    ),
    (
        "stubs/dependency-injection/Definition.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection;\nclass Definition {\n    public function __construct(?string $class = null) {}\n}\n",
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
    $services->set('app.child')->parent('app.mailer')->args(['%app.sender_name%', 'plain %']);
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
            (php_config.clone(), 12),
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
            (php_config.clone(), 12),
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

/// The `invalid_symfony_parameter` messages a slow-diagnostics pass reports
/// for `content`.
fn parameter_problems(backend: &Backend, uri: &Uri, content: &str) -> Vec<String> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code)) if code == "invalid_symfony_parameter"
            )
        })
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect()
}

#[tokio::test]
async fn parameters_set_outside_the_configuration_are_declared() {
    let kernel = r#"<?php
namespace App;

use Symfony\Component\HttpKernel\Kernel as BaseKernel;

class Kernel extends BaseKernel
{
    protected function getKernelParameters(): array
    {
        return parent::getKernelParameters() + ['app.from_kernel' => true];
    }
}
"#;
    let pass = r#"<?php
namespace App\DependencyInjection;

final class UntypedPass
{
    public function process($container): void
    {
        $container->setParameter('app.untyped', true);
    }
}
"#;
    let consumer = r#"<?php
use Symfony\Component\DependencyInjection\ContainerInterface;

function read(ContainerInterface $container): void {
    $container->getParameter('app.from_kernel');
    $container->getParameter('app.untyped');
    $container->getParameter('app.missing_parameter');
}
"#;
    let (backend, dir) = workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("src/Kernel.php", kernel),
        ("src/DependencyInjection/UntypedPass.php", pass),
        ("src/consumer.php", consumer),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/consumer.php");
    assert_eq!(
        parameter_problems(&backend, &consumer_uri, consumer),
        vec!["Unknown container parameter: 'app.missing_parameter'".to_string()]
    );
}

#[tokio::test]
async fn a_parameter_name_built_at_runtime_leaves_its_prefix_unjudged() {
    let pass = r#"<?php
namespace App\DependencyInjection;

use Symfony\Component\DependencyInjection\ContainerBuilder;

final class ConfigPass
{
    public function process(ContainerBuilder $container, array $config): void
    {
        foreach ($config as $key => $value) {
            $container->setParameter('app.' . $key, $value);
        }
    }
}
"#;
    // A query's parameters are no container's.
    let repository = r#"<?php
namespace App\Repository;

final class Query
{
    public function setParameter(string $key, mixed $value): void {}
}

final class Repository
{
    public function find(Query $query, string $key): void
    {
        $query->setParameter('app.' . $key, 1);
    }
}
"#;
    let consumer = r#"<?php
use Symfony\Component\DependencyInjection\ContainerInterface;

function read(ContainerInterface $container): void {
    $container->getParameter('app.from_config');
}
"#;
    let (backend, dir) = workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("src/DependencyInjection/ConfigPass.php", pass),
        ("src/consumer.php", consumer),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/consumer.php");
    assert!(parameter_problems(&backend, &consumer_uri, consumer).is_empty());

    let (backend, dir) = workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("src/Repository/Repository.php", repository),
        ("src/consumer.php", consumer),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/consumer.php");
    assert_eq!(
        parameter_problems(&backend, &consumer_uri, consumer),
        vec!["Unknown container parameter: 'app.from_config'".to_string()]
    );

    // The pattern is no name to offer.
    let (backend, dir) = workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("src/DependencyInjection/ConfigPass.php", pass),
        ("src/consumer.php", consumer),
    ])
    .await;
    let consumer_uri = uri_for(&dir, "src/consumer.php");
    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "'app.from_config'", 5),
    )
    .await;
    assert!(
        !labels.iter().any(|label| label.ends_with('*')),
        "{labels:?}"
    );
}

const KERNEL_PHP: &str = r#"<?php
namespace App;

use Symfony\Component\DependencyInjection\ContainerBuilder;
use Symfony\Component\DependencyInjection\Definition;

final class Kernel
{
    protected function build(ContainerBuilder $container): void
    {
        $container->register('app.registered', \App\Service\Mailer::class);
        $container->setDefinition('app.defined', new Definition(\App\Service\Mailer::class));
        $container->setAlias('app.registered_alias', 'app.registered');
        $container->setParameter('app.built_sender', 'PHPantom');
        $container->getDefinition('app.registered');
    }
}
"#;

#[tokio::test]
async fn a_container_builder_declares_services_and_parameters() {
    let consumer = r#"<?php
use Symfony\Component\DependencyInjection\ContainerInterface;

function send(ContainerInterface $container): void {
    $container->get('app.registered');
    $container->get('app.registered_alias');
    $container->get('app.defined');
    $container->getParameter('app.built_sender');
    $container->get('app.unregistered');
}
"#;
    let (backend, dir) = workspace(&[
        ("src/Kernel.php", KERNEL_PHP),
        ("config/services.yaml", SERVICES_YAML),
        ("src/consumer.php", consumer),
    ])
    .await;
    let kernel = uri_for(&dir, "src/Kernel.php");
    let consumer_uri = uri_for(&dir, "src/consumer.php");

    for (needle, line) in [
        ("get('app.registered')", 10),
        ("get('app.registered_alias')", 12),
        ("get('app.defined')", 11),
        ("getParameter('app.built_sender')", 13),
    ] {
        let found = definitions(
            &backend,
            &consumer_uri,
            position_in(consumer, needle, needle.find('\'').unwrap() + 3),
        )
        .await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!((&found[0].uri, found[0].range.start.line), (&kernel, line));
    }

    // The alias's target and a definition lookup read the declared name.
    let found = references(
        &backend,
        &kernel,
        position_in(KERNEL_PHP, "register('app.registered'", 12),
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![
            (consumer_uri.clone(), 4),
            (kernel.clone(), 10),
            (kernel.clone(), 12),
            (kernel.clone(), 14),
        ])
    );

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(consumer_uri.as_str(), consumer, &mut diagnostics);
    let reported: Vec<String> = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code)) if code.starts_with("invalid_symfony_")
            )
        })
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect();
    assert_eq!(
        reported,
        vec!["Unknown service: 'app.unregistered'".to_string()]
    );
}

#[tokio::test]
async fn a_configurator_names_a_parent_and_placeholders_in_its_arguments() {
    let (backend, dir) = workspace(&[
        ("config/services.yaml", SERVICES_YAML),
        ("config/services.php", SERVICES_PHP),
    ])
    .await;
    let yaml = uri_for(&dir, "config/services.yaml");
    let php_config = uri_for(&dir, "config/services.php");

    let found = definitions(
        &backend,
        &php_config,
        position_in(SERVICES_PHP, "parent('app.mailer')", 10),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((&found[0].uri, found[0].range.start.line), (&yaml, 3));

    let found = definitions(
        &backend,
        &php_config,
        position_in(SERVICES_PHP, "'%app.sender_name%'", 4),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((&found[0].uri, found[0].range.start.line), (&yaml, 1));
}

const BUNDLE_INSTALLED: &str = r#"{ "packages": [ {
    "name": "acme/demo-bundle",
    "install-path": "../acme/demo-bundle",
    "autoload": { "psr-4": { "Acme\\DemoBundle\\": "src/" } }
} ] }"#;

const BUNDLE_SERVICES: &str = r#"<?php
namespace Symfony\Component\DependencyInjection\Loader\Configurator;

return static function (ContainerConfigurator $container) {
    $container->parameters()->set('acme_demo.greeting', 'Hello');
    $container->services()
        ->set('acme_demo.greeter', \Acme\DemoBundle\Greeter::class)
        ->set('.acme_demo.hidden');
};
"#;

#[tokio::test]
async fn an_installed_bundle_declares_services_and_parameters() {
    let consumer = r#"<?php
use Symfony\Component\DependencyInjection\ContainerInterface;

function greet(ContainerInterface $container): void {
    $container->get('acme_demo.greeter');
    $container->getParameter('acme_demo.greeting');
    $container->get('acme_demo.');
    $greeter = $container->get('acme_demo.greeter');
    $greeter;
}
"#;
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<(&str, &str)> = STUBS
        .iter()
        .copied()
        .chain([
            ("composer.json", COMPOSER),
            ("vendor/composer/installed.json", BUNDLE_INSTALLED),
            (
                "vendor/acme/demo-bundle/src/AcmeDemoBundle.php",
                "<?php\nnamespace Acme\\DemoBundle;\nfinal class AcmeDemoBundle {}\n",
            ),
            (
                "vendor/acme/demo-bundle/src/Greeter.php",
                "<?php\nnamespace Acme\\DemoBundle;\nfinal class Greeter {\n    public function greet(): string { return ''; }\n}\n",
            ),
            ("vendor/acme/demo-bundle/config/services.php", BUNDLE_SERVICES),
            (
                "vendor/acme/demo-bundle/src/Resources/config/legacy.xml",
                "<container><services><service id=\"acme_demo.legacy\"/></services></container>\n",
            ),
            ("src/consumer.php", consumer),
        ])
        .collect();
    for (path, content) in &files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = Backend::new_test();
    crate::common::initialize_with_options(&backend, dir.path(), None).await;
    let consumer_uri = uri_for(&dir, "src/consumer.php");
    open_document(&backend, &consumer_uri, "php", consumer).await;
    let bundle_services = uri_for(&dir, "vendor/acme/demo-bundle/config/services.php");

    let found = definitions(
        &backend,
        &consumer_uri,
        position_in(consumer, "get('acme_demo.greeter')", 7),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (&found[0].uri, found[0].range.start.line),
        (&bundle_services, 6)
    );

    let found = definitions(
        &backend,
        &consumer_uri,
        position_in(consumer, "getParameter('acme_demo.greeting')", 15),
    )
    .await;
    assert_eq!(
        (&found[0].uri, found[0].range.start.line),
        (&bundle_services, 4)
    );

    let labels = completion_labels(
        &backend,
        &consumer_uri,
        position_in(consumer, "get('acme_demo.')", 15),
    )
    .await;
    assert!(
        labels.contains(&"acme_demo.greeter".to_string())
            && labels.contains(&"acme_demo.legacy".to_string())
            && !labels.contains(&".acme_demo.hidden".to_string()),
        "{labels:?}"
    );

    // The service is typed by the class the bundle builds it from.
    let at = position_in(consumer, "$greeter;", 2);
    let hover = crate::common::hover_text_at(&backend, &consumer_uri, at.line, at.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains(" = Greeter\n"), "{hover}");
}
