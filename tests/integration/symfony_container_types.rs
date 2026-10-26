//! The return types Symfony's service container decides: a service fetched
//! by its ID, a container parameter, and what a query bus's
//! `HandleTrait::handle()` gets back from the message's handler.

use crate::common::{create_psr4_workspace, hover_text_at, open_document, split_cursor};
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Psr\\Container\\": "stubs/psr-container/",
        "Symfony\\Bundle\\FrameworkBundle\\": "stubs/framework-bundle/",
        "Symfony\\Component\\DependencyInjection\\": "stubs/dependency-injection/",
        "Symfony\\Component\\Messenger\\": "stubs/messenger/"
    } }
}"#;

/// Just enough of Symfony for the type engine to tell a container, a
/// parameter bag, a controller, and a query bus from any other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/psr-container/ContainerInterface.php",
        "<?php\nnamespace Psr\\Container;\ninterface ContainerInterface {\n    public function get(string $id): mixed;\n    public function has(string $id): bool;\n}\n",
    ),
    (
        "stubs/dependency-injection/ContainerInterface.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection;\ninterface ContainerInterface extends \\Psr\\Container\\ContainerInterface {\n    public function getParameter(string $name): array|bool|string|int|float|\\UnitEnum|null;\n}\n",
    ),
    (
        "stubs/dependency-injection/ParameterBag/ParameterBagInterface.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\ParameterBag;\ninterface ParameterBagInterface {\n    public function get(string $name): array|bool|string|int|float|\\UnitEnum|null;\n}\n",
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
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ServiceConfigurator {\n    public function class(?string $class): static {}\n    public function args(array $arguments): static {}\n}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/ParametersConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ParametersConfigurator {\n    public function set(string $name, mixed $value): static {}\n}\n",
    ),
    (
        "stubs/framework-bundle/Controller/AbstractController.php",
        "<?php\nnamespace Symfony\\Bundle\\FrameworkBundle\\Controller;\nabstract class AbstractController {\n    protected \\Psr\\Container\\ContainerInterface $container;\n    protected function getParameter(string $name): array|bool|string|int|float|\\UnitEnum|null {}\n}\n",
    ),
    (
        "stubs/messenger/HandleTrait.php",
        "<?php\nnamespace Symfony\\Component\\Messenger;\ntrait HandleTrait {\n    private function handle(object $message): mixed {}\n}\n",
    ),
    (
        "stubs/messenger/Attribute/AsMessageHandler.php",
        "<?php\nnamespace Symfony\\Component\\Messenger\\Attribute;\n#[\\Attribute]\nclass AsMessageHandler {\n    public function __construct(?string $bus = null, ?string $handles = null, ?string $method = null) {}\n}\n",
    ),
    (
        "src/Service/Mailer.php",
        "<?php\nnamespace App\\Service;\nclass Mailer {\n    public function send(): void {}\n}\n",
    ),
    (
        "src/Service/SmtpMailer.php",
        "<?php\nnamespace App\\Service;\nclass SmtpMailer extends Mailer {}\n",
    ),
    (
        "src/Service/Newsletter.php",
        "<?php\nnamespace App\\Service;\nclass Newsletter {}\n",
    ),
];

const SERVICES_YAML: &str = "parameters:
    app.sender_name: PHPantom
    app.port: 25
    app.ratio: 0.5
    app.debug: true
    app.locales: [en, da]
    app.limits:
        daily: 10
    app.mailer_class: App\\Service\\SmtpMailer
    app.copied_sender: '%app.sender_name%'
    app.timeout: '%env(int:MAILER_TIMEOUT)%'
services:
    app.mailer:
        class: App\\Service\\Mailer
        arguments: ['%app.sender_name%']
    app.mailer_alias: '@app.mailer'
    app.long_alias:
        alias: app.mailer_alias
    app.smtp_mailer:
        class: '%app.mailer_class%'
    app.unknown: ~
";

const SERVICES_XML: &str = r#"<?xml version="1.0"?>
<container>
  <parameters>
    <parameter key="app.xml_count">3</parameter>
    <parameter key="app.xml_label" type="string">3</parameter>
  </parameters>
  <services>
    <service id="app.xml_mailer" class="App\Service\Mailer"/>
    <service id="app.xml_alias" alias="app.xml_mailer"/>
  </services>
</container>
"#;

const SERVICES_PHP: &str = r#"<?php
namespace Symfony\Component\DependencyInjection\Loader\Configurator;

use App\Service\Newsletter;
use App\Service\SmtpMailer;

return static function (ContainerConfigurator $container): void {
    $services = $container->services();
    $parameters = $container->parameters();

    $services->set('app.newsletter', Newsletter::class);
    $services->set('app.php_smtp')->args([])->class(SmtpMailer::class);
    $services->alias('app.php_alias', 'app.newsletter');
    $parameters->set('app.php_sender', 'PHPantom');
    $parameters->set('app.php_retries', 3);
};
"#;

/// The hover at the `§` marker in a controller method whose body is
/// `body`, with the project's configuration in place.
async fn hover(body: &str) -> String {
    let source = format!(
        "<?php
namespace App\\Controller;

use Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController;
use Symfony\\Component\\DependencyInjection\\ContainerInterface;
use Symfony\\Component\\DependencyInjection\\ParameterBag\\ParameterBagInterface;

final class MailController extends AbstractController
{{
    public function send(ContainerInterface $container, ParameterBagInterface $bag): void
    {{
{body}
    }}
}}
"
    );
    hover_in(&[], "src/Controller/MailController.php", &source).await
}

/// The hover at the `§` marker in the file `path`, with the project's
/// configuration and `files` in place.
async fn hover_in(files: &[(&str, &str)], path: &str, source: &str) -> String {
    let (stripped, position) = split_cursor(source);
    let mut all: Vec<(&str, &str)> = STUBS.to_vec();
    all.extend([
        ("config/services.yaml", SERVICES_YAML),
        ("config/services.xml", SERVICES_XML),
        ("config/services.php", SERVICES_PHP),
    ]);
    all.extend(files);
    all.push((path, stripped.as_str()));
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (file, content) in [
        ("config/services.yaml", SERVICES_YAML),
        ("config/services.xml", SERVICES_XML),
        ("config/services.php", SERVICES_PHP),
    ] {
        let language = file.rsplit('.').next().unwrap();
        open_document(&backend, &uri_for(&dir, file), language, content).await;
    }
    for (file, content) in files {
        open_document(&backend, &uri_for(&dir, file), "php", content).await;
    }
    let uri = uri_for(&dir, path);
    open_document(&backend, &uri, "php", &stripped).await;
    hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .unwrap_or_default()
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dir.path().join(rel)).unwrap()
}

/// Whether a variable's hover shows it typed as exactly `ty`.
fn shows(hover: &str, ty: &str) -> bool {
    hover.contains(&format!(" = {ty}\n"))
}

#[tokio::test]
async fn a_service_id_is_typed_by_its_declared_class() {
    let hover = hover("        $mailer = $container->get('app.mailer');\n        $mai§ler;").await;
    assert!(shows(&hover, "Mailer"), "{hover}");
}

#[tokio::test]
async fn aliases_are_followed_to_the_service_they_name() {
    for id in ["app.mailer_alias", "app.long_alias", "app.xml_alias"] {
        let hover = hover(&format!(
            "        $mailer = $container->get('{id}');\n        $mai§ler;"
        ))
        .await;
        assert!(shows(&hover, "Mailer"), "{id}: {hover}");
    }
}

#[tokio::test]
async fn a_class_given_as_a_parameter_is_the_parameter_value() {
    let hover =
        hover("        $mailer = $container->get('app.smtp_mailer');\n        $mai§ler;").await;
    assert!(shows(&hover, "SmtpMailer"), "{hover}");
}

#[tokio::test]
async fn php_configurators_declare_service_classes() {
    for (id, class) in [
        ("app.newsletter", "Newsletter"),
        ("app.php_smtp", "SmtpMailer"),
        ("app.php_alias", "Newsletter"),
    ] {
        let hover = hover(&format!(
            "        $service = $this->container->get('{id}');\n        $serv§ice;"
        ))
        .await;
        assert!(shows(&hover, class), "{id}: {hover}");
    }
}

#[tokio::test]
async fn an_undeclared_or_classless_service_keeps_the_declared_type() {
    for id in ["app.unknown", "app.nowhere"] {
        let hover = hover(&format!(
            "        $service = $container->get('{id}');\n        $serv§ice;"
        ))
        .await;
        assert!(shows(&hover, "mixed"), "{id}: {hover}");
    }
}

#[tokio::test]
async fn parameters_are_typed_by_their_declared_values() {
    for (name, ty) in [
        ("app.sender_name", "string"),
        ("app.port", "int"),
        ("app.ratio", "float"),
        ("app.debug", "bool"),
        ("app.locales", "array"),
        ("app.limits", "array"),
        ("app.copied_sender", "string"),
        ("app.timeout", "int"),
        ("app.xml_count", "int"),
        ("app.xml_label", "string"),
        ("app.php_sender", "string"),
        ("app.php_retries", "int"),
        ("kernel.project_dir", "string"),
    ] {
        let hover = hover(&format!(
            "        $value = $this->getParameter('{name}');\n        $val§ue;"
        ))
        .await;
        assert!(shows(&hover, ty), "{name}: {hover}");
    }
}

#[tokio::test]
async fn every_parameter_reader_is_typed() {
    for call in [
        "$container->getParameter('app.port')",
        "$bag->get('app.port')",
    ] {
        let hover = hover(&format!("        $value = {call};\n        $val§ue;")).await;
        assert!(shows(&hover, "int"), "{call}: {hover}");
    }
}

const QUERIES: &str = r#"<?php
namespace App\Query;

use Symfony\Component\Messenger\Attribute\AsMessageHandler;

class GetOrder {}
class Order {}
class GetTotal {}
class Twice {}
class Unhandled {}

#[AsMessageHandler]
final class GetOrderHandler
{
    public function __invoke(GetOrder $query): Order
    {
        return new Order();
    }
}

final class TotalHandlers
{
    #[AsMessageHandler]
    public function total(GetTotal $query): int
    {
        return 1;
    }
}

#[AsMessageHandler]
final class TwiceA
{
    public function __invoke(Twice $query): int { return 1; }
}

#[AsMessageHandler]
final class TwiceB
{
    public function __invoke(Twice $query): string { return ''; }
}
"#;

/// The hover at the `§` marker inside a query bus built on `HandleTrait`.
async fn bus_hover(body: &str) -> String {
    let source = format!(
        "<?php
namespace App\\Query;

use Symfony\\Component\\Messenger\\HandleTrait;

final class QueryBus
{{
    use HandleTrait;

    public function run(): void
    {{
{body}
    }}
}}
"
    );
    hover_in(
        &[("src/Query/Queries.php", QUERIES)],
        "src/Query/QueryBus.php",
        &source,
    )
    .await
}

#[tokio::test]
async fn handle_returns_what_the_message_handler_returns() {
    let hover =
        bus_hover("        $order = $this->handle(new GetOrder());\n        $ord§er;").await;
    assert!(shows(&hover, "Order"), "{hover}");
    let hover =
        bus_hover("        $total = $this->handle(new GetTotal());\n        $tot§al;").await;
    assert!(shows(&hover, "int"), "{hover}");
}

#[tokio::test]
async fn handle_keeps_mixed_without_exactly_one_handler() {
    for message in ["Twice", "Unhandled"] {
        let hover = bus_hover(&format!(
            "        $result = $this->handle(new {message}());\n        $res§ult;"
        ))
        .await;
        assert!(shows(&hover, "mixed"), "{message}: {hover}");
    }
}
