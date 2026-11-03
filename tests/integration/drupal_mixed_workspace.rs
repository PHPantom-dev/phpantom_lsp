//! A Drupal site in a workspace with other frameworks' projects: each of
//! them keeps working in its own project, and Drupal works in its site.
//!
//! The Laravel application is the workspace root, since Laravel's
//! configuration and routes are read from there, and the Drupal site, the
//! Symfony application, and the WordPress plugin are Composer projects
//! installed beside it.  The Drupal flag is the workspace's, as every
//! framework's is, so only what works is checked here; what a project
//! without Drupal does not get is checked in a workspace of its own.

use crate::common::{
    completion_response, hover_text_at, initialize_with_options, offset_position, open_document,
    response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

// ─── Laravel, at the root ───────────────────────────────────────────────────

const LARAVEL_COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "app/" } }
}"#;

const LARAVEL_CONFIG: &str = "<?php\nreturn [\n    'name' => 'Acme',\n];\n";

const LARAVEL_SETTINGS: &str = r#"<?php
namespace App;

class Settings {
    public function demo(): void {
        config('app.name');
    }
}
"#;

// ─── Drupal, in `drupal/` ───────────────────────────────────────────────────

const DRUPAL_COMPOSER: &str = r#"{
    "require": { "drupal/core-recommended": "^11" },
    "extra": { "drupal-scaffold": { "locations": { "web-root": "web/" } } }
}"#;

const DRUPAL_CLASS: &str =
    "<?php\nclass Drupal {\n    public static function service(string $id): object {}\n}\n";

const DRUPAL_GREETER: &str = "<?php\nnamespace Drupal\\acme;\nclass Greeter {\n    public function greet(): string { return ''; }\n}\n";

const DRUPAL_SERVICES: &str = "services:\n  acme.greeter:\n    class: Drupal\\acme\\Greeter\n";

const DRUPAL_CONSUMER: &str = r#"<?php
namespace Drupal\acme;

final class StaticConsumer {
    public function run(): void {
        $greeter = \Drupal::service('acme.greeter');
        $greeter;
    }
}
"#;

const DRUPAL_SERVICES_PATH: &str = "drupal/web/modules/custom/acme/acme.services.yml";
const DRUPAL_CONSUMER_PATH: &str = "drupal/web/modules/custom/acme/src/StaticConsumer.php";

// ─── Symfony, in `symfony/` ─────────────────────────────────────────────────

const SYMFONY_COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "Shop\\": "src/",
        "Psr\\Container\\": "stubs/psr-container/",
        "Symfony\\Component\\DependencyInjection\\": "stubs/dependency-injection/"
    } }
}"#;

/// Just enough of the container and its configurators for the type engine
/// to tell a container from any other receiver.
const SYMFONY_STUBS: &[(&str, &str)] = &[
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
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ContainerConfigurator {\n    public function services(): ServicesConfigurator {}\n}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/ServicesConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ServicesConfigurator extends AbstractConfigurator {\n    public function set(?string $id, ?string $class = null): ServiceConfigurator {}\n}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/ServiceConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nclass ServiceConfigurator extends AbstractConfigurator {}\n",
    ),
    (
        "stubs/dependency-injection/Loader/Configurator/AbstractConfigurator.php",
        "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\nabstract class AbstractConfigurator {}\n",
    ),
    (
        "src/Service/Mailer.php",
        "<?php\nnamespace Shop\\Service;\nclass Mailer {\n    public function send(): void {}\n}\n",
    ),
];

const SYMFONY_SERVICES_YAML: &str =
    "services:\n    app.mailer:\n        class: Shop\\Service\\Mailer\n";

/// A service only a PHP configurator declares, which Drupal's reading of
/// services files never records.
const SYMFONY_SERVICES_PHP: &str = r#"<?php
namespace Symfony\Component\DependencyInjection\Loader\Configurator;

return static function (ContainerConfigurator $container): void {
    $services = $container->services();
    $services->set('app.php_mailer', \Shop\Service\Mailer::class);
};
"#;

const SYMFONY_CONSUMER: &str = r#"<?php
namespace Shop\Controller;

use Symfony\Component\DependencyInjection\ContainerInterface;

final class MailController
{
    public function send(ContainerInterface $container): void
    {
        $mailer = $container->get('app.mailer');
        $mailer;
        $container->get('app.php_mailer');
        $container->get('app.');
    }
}
"#;

const SYMFONY_YAML_PATH: &str = "symfony/config/services.yaml";
const SYMFONY_PHP_PATH: &str = "symfony/config/services.php";
const SYMFONY_CONSUMER_PATH: &str = "symfony/src/Controller/MailController.php";

// ─── WordPress, in `wordpress-plugin/` ──────────────────────────────────────

const WORDPRESS_COMPOSER: &str = r#"{
    "autoload": { "psr-4": { "Acme\\Plugin\\": "src/" } }
}"#;

const WORDPRESS_FIRES: &str = r#"<?php
namespace Acme\Plugin;

final class Checkout
{
    public function place(object $order): void
    {
        do_action('acme_order_placed', $order);
    }
}
"#;

const WORDPRESS_LISTENS: &str = r#"<?php
namespace Acme\Plugin;

final class Mailer
{
    public function register(): void
    {
        add_action('acme_order_placed', [$this, 'sendReceipt']);
    }
}
"#;

const WORDPRESS_FIRES_PATH: &str = "wordpress-plugin/src/Checkout.php";
const WORDPRESS_LISTENS_PATH: &str = "wordpress-plugin/src/Mailer.php";

/// What makes a directory an installed Composer project the workspace
/// indexes beside the root, rather than a package the root installs.
const INSTALLED: &str = r#"{ "packages": [] }"#;

/// The files of the mixed workspace, by path from the workspace root.
fn mixed_files() -> Vec<(String, &'static str)> {
    let mut files: Vec<(String, &'static str)> = vec![
        ("composer.json".to_string(), LARAVEL_COMPOSER),
        ("config/app.php".to_string(), LARAVEL_CONFIG),
        ("app/Settings.php".to_string(), LARAVEL_SETTINGS),
        ("drupal/composer.json".to_string(), DRUPAL_COMPOSER),
        (
            "drupal/vendor/composer/installed.json".to_string(),
            INSTALLED,
        ),
        ("drupal/web/core/lib/Drupal.php".to_string(), DRUPAL_CLASS),
        (
            "drupal/web/modules/custom/acme/src/Greeter.php".to_string(),
            DRUPAL_GREETER,
        ),
        (DRUPAL_SERVICES_PATH.to_string(), DRUPAL_SERVICES),
        (DRUPAL_CONSUMER_PATH.to_string(), DRUPAL_CONSUMER),
        ("symfony/composer.json".to_string(), SYMFONY_COMPOSER),
        (
            "symfony/vendor/composer/installed.json".to_string(),
            INSTALLED,
        ),
        (SYMFONY_YAML_PATH.to_string(), SYMFONY_SERVICES_YAML),
        (SYMFONY_PHP_PATH.to_string(), SYMFONY_SERVICES_PHP),
        (SYMFONY_CONSUMER_PATH.to_string(), SYMFONY_CONSUMER),
        (
            "wordpress-plugin/composer.json".to_string(),
            WORDPRESS_COMPOSER,
        ),
        (
            "wordpress-plugin/vendor/composer/installed.json".to_string(),
            INSTALLED,
        ),
        (WORDPRESS_FIRES_PATH.to_string(), WORDPRESS_FIRES),
        (WORDPRESS_LISTENS_PATH.to_string(), WORDPRESS_LISTENS),
    ];
    files.extend(
        SYMFONY_STUBS
            .iter()
            .map(|(path, content)| (format!("symfony/{path}"), *content)),
    );
    files
}

/// An initialized workspace holding `files`, with every PHP and YAML file
/// among them open.
async fn workspace(files: &[(String, &str)]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    for (path, content) in files {
        let language = if path.ends_with(".php") {
            "php"
        } else if path.ends_with(".yml") || path.ends_with(".yaml") {
            "yaml"
        } else {
            continue;
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

/// The 0-based line of the first occurrence of `needle`.
fn line_of(content: &str, needle: &str) -> u32 {
    position_in(content, needle, 0).line
}

/// The definitions of the name at `position`, as files and lines.
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

/// Every reference to the name at `position`, declarations included, as
/// files and lines.
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

async fn completion_labels(backend: &Backend, uri: &Uri, position: Position) -> Vec<String> {
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

async fn hover_at(backend: &Backend, uri: &Uri, position: Position) -> String {
    hover_text_at(backend, uri, position.line, position.character)
        .await
        .unwrap_or_default()
}

#[tokio::test]
async fn symfony_services_keep_working_beside_a_drupal_site() {
    let (backend, dir) = workspace(&mixed_files()).await;
    let consumer = uri_for(&dir, SYMFONY_CONSUMER_PATH);

    let found = definitions(
        &backend,
        &consumer,
        position_in(SYMFONY_CONSUMER, "get('app.mailer')", 7),
    )
    .await;
    assert_eq!(
        found,
        vec![(
            uri_for(&dir, SYMFONY_YAML_PATH),
            line_of(SYMFONY_SERVICES_YAML, "app.mailer:")
        )]
    );

    let found = definitions(
        &backend,
        &consumer,
        position_in(SYMFONY_CONSUMER, "get('app.php_mailer')", 7),
    )
    .await;
    assert_eq!(
        found,
        vec![(
            uri_for(&dir, SYMFONY_PHP_PATH),
            line_of(SYMFONY_SERVICES_PHP, "set('app.php_mailer'")
        )]
    );

    let hover = hover_at(
        &backend,
        &consumer,
        position_in(SYMFONY_CONSUMER, "$mailer;", 2),
    )
    .await;
    assert!(hover.contains("Mailer"), "{hover}");

    let labels = completion_labels(
        &backend,
        &consumer,
        position_in(SYMFONY_CONSUMER, "get('app.')", 9),
    )
    .await;
    for id in ["app.mailer", "app.php_mailer"] {
        assert!(labels.contains(&id.to_string()), "{id}: {labels:?}");
    }
}

#[tokio::test]
async fn laravel_config_keys_keep_working_beside_a_drupal_site() {
    let (backend, dir) = workspace(&mixed_files()).await;
    let found = definitions(
        &backend,
        &uri_for(&dir, "app/Settings.php"),
        position_in(LARAVEL_SETTINGS, "config('app.name')", 10),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, uri_for(&dir, "config/app.php"));
}

#[tokio::test]
async fn wordpress_hooks_keep_working_beside_a_drupal_site() {
    let (backend, dir) = workspace(&mixed_files()).await;
    let found = definitions(
        &backend,
        &uri_for(&dir, WORDPRESS_LISTENS_PATH),
        position_in(WORDPRESS_LISTENS, "'acme_order_placed'", 3),
    )
    .await;
    assert_eq!(
        found,
        vec![(
            uri_for(&dir, WORDPRESS_FIRES_PATH),
            line_of(WORDPRESS_FIRES, "do_action(")
        )]
    );
}

#[tokio::test]
async fn drupal_services_work_in_the_drupal_site() {
    let (backend, dir) = workspace(&mixed_files()).await;
    let consumer = uri_for(&dir, DRUPAL_CONSUMER_PATH);

    let hover = hover_at(
        &backend,
        &consumer,
        position_in(DRUPAL_CONSUMER, "$greeter;", 2),
    )
    .await;
    assert!(hover.contains("Greeter"), "{hover}");

    let found = definitions(
        &backend,
        &consumer,
        position_in(DRUPAL_CONSUMER, "service('acme.greeter')", 10),
    )
    .await;
    assert_eq!(
        found,
        vec![(
            uri_for(&dir, DRUPAL_SERVICES_PATH),
            line_of(DRUPAL_SERVICES, "acme.greeter:")
        )]
    );
}

// ─── A Symfony application alone ───────────────────────────────────────────

/// The class `\Drupal` a Symfony application may hold without being a
/// Drupal site: a library's, or one of its own.
const LONE_DRUPAL_CLASS: &str =
    "<?php\nclass Drupal {\n    public static function service(string $id): object {}\n}\n";

const LONE_CONSUMER: &str = r#"<?php
namespace Shop\Controller;

use Symfony\Component\DependencyInjection\ContainerInterface;

final class Lookups
{
    public function run(ContainerInterface $container): void
    {
        $mailer = $container->get('app.mailer');
        $mailer;
        $other = \Drupal::service('app.mailer');
        $other;
        \Drupal::service('app.');
    }
}
"#;

const LONE_CONSUMER_PATH: &str = "src/Controller/Lookups.php";

/// A workspace that is a Symfony application and nothing else.
async fn symfony_alone() -> (Backend, tempfile::TempDir) {
    let mut files: Vec<(String, &str)> = vec![
        ("composer.json".to_string(), SYMFONY_COMPOSER),
        ("config/services.yaml".to_string(), SYMFONY_SERVICES_YAML),
        ("stubs/Drupal.php".to_string(), LONE_DRUPAL_CLASS),
        (LONE_CONSUMER_PATH.to_string(), LONE_CONSUMER),
    ];
    files.extend(
        SYMFONY_STUBS
            .iter()
            .map(|(path, content)| (path.to_string(), *content)),
    );
    workspace(&files).await
}

/// Outside a Drupal site, `\Drupal::service()` names no service: nothing
/// completes in it, it is no reference to a service, and it keeps the
/// return type its class declares.
#[tokio::test]
async fn drupal_service_names_nothing_in_a_symfony_application_alone() {
    let (backend, dir) = symfony_alone().await;
    let consumer = uri_for(&dir, LONE_CONSUMER_PATH);

    let labels = completion_labels(
        &backend,
        &consumer,
        position_in(LONE_CONSUMER, "service('app.')", 13),
    )
    .await;
    assert!(!labels.contains(&"app.mailer".to_string()), "{labels:?}");

    let found = definitions(
        &backend,
        &consumer,
        position_in(LONE_CONSUMER, "service('app.mailer')", 10),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");

    // The container lookup is a reference; `\Drupal::service()` is not.
    let found = references(
        &backend,
        &uri_for(&dir, "config/services.yaml"),
        position_in(SYMFONY_SERVICES_YAML, "app.mailer:", 2),
    )
    .await;
    assert!(
        found.contains(&(
            consumer.clone(),
            line_of(LONE_CONSUMER, "get('app.mailer')")
        )),
        "{found:?}"
    );
    assert!(
        !found.contains(&(
            consumer.clone(),
            line_of(LONE_CONSUMER, "service('app.mailer')")
        )),
        "{found:?}"
    );

    let hover = hover_at(
        &backend,
        &consumer,
        position_in(LONE_CONSUMER, "$mailer;", 2),
    )
    .await;
    assert!(hover.contains("Mailer"), "{hover}");
    let hover = hover_at(
        &backend,
        &consumer,
        position_in(LONE_CONSUMER, "$other;", 2),
    )
    .await;
    assert!(!hover.contains("Mailer"), "{hover}");
    assert!(hover.contains("object"), "{hover}");
}
