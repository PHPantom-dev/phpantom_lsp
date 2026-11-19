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
pub(crate) const STUBS: &[(&str, &str)] = &[
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
pub(crate) async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    workspace_in(tempfile::tempdir().unwrap(), files).await
}

/// [`workspace`] in `dir`, which may hold files already.
pub(crate) async fn workspace_in(
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

/// [`workspace_in`] with `files` opened while the workspace walk runs, the
/// way an editor restores its tabs: after the handshake, before indexing
/// ends.
pub(crate) async fn workspace_opened_early(
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
    #[allow(deprecated)] // `root_uri` is what this generation of clients sends
    let params = InitializeParams {
        root_uri: Some(Uri::from_file_path(dir.path()).unwrap()),
        ..Default::default()
    };
    backend.initialize(params).await.unwrap();
    for (path, content) in files {
        open_document(&backend, &uri_for(&dir, path), "php", content).await;
    }
    backend.initialized(InitializedParams {}).await;
    (backend, dir)
}

pub(crate) fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dir.path().join(rel)).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
pub(crate) fn position_in(content: &str, needle: &str, inside: usize) -> Position {
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
pub(crate) async fn references_at(
    backend: &Backend,
    uri: &Uri,
    position: Position,
) -> Vec<(Uri, u32)> {
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
pub(crate) async fn definitions_at(
    backend: &Backend,
    uri: &Uri,
    position: Position,
) -> Vec<Location> {
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
    let custom_class = position_in(consumer, "new Greeter", 5);
    assert!(
        crate::common::prepare_rename(&backend, &php, custom_class.line, custom_class.character)
            .await
            .is_some(),
        "a custom class could not be renamed"
    );
}

/// A call chained straight off `\Drupal::service()`, with no variable in
/// between, is typed the same way.
#[tokio::test]
async fn a_chained_drupal_service_call_is_typed() {
    let chained = STATIC_CONSUMER.replace(
        "$greeter = \\Drupal::service('acme.greeter');",
        "\\Drupal::service('acme.greeter')->greet();",
    );
    let (backend, dir) = workspace(&[
        (SERVICES_PATH, SERVICES),
        (STATIC_CONSUMER_PATH, chained.as_str()),
    ])
    .await;
    let found = definitions_at(
        &backend,
        &uri_for(&dir, STATIC_CONSUMER_PATH),
        position_in(&chained, "->greet()", 3),
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        found[0].uri,
        uri_for(&dir, "web/modules/custom/acme/src/Greeter.php")
    );
}

/// A call chained straight off a container lookup, with no variable in
/// between, is typed by the service, and is a reference to the method.
#[tokio::test]
async fn a_chained_container_lookup_call_is_typed() {
    let chained = CONSUMER.replace(
        "$greeter = $container->get('acme.greeter');",
        "$container->get('acme.greeter')->greet();",
    );
    let (backend, dir) =
        workspace(&[(SERVICES_PATH, SERVICES), (CONSUMER_PATH, chained.as_str())]).await;
    let php = uri_for(&dir, CONSUMER_PATH);
    let found = definitions_at(&backend, &php, position_in(&chained, "->greet()", 3)).await;
    assert_eq!(found.len(), 1, "{found:?}");
    let greeter = uri_for(&dir, "web/modules/custom/acme/src/Greeter.php");
    assert_eq!(found[0].uri, greeter);

    let found = references_at(&backend, &greeter, found[0].range.start).await;
    assert!(found.iter().any(|(uri, _)| *uri == php), "{found:?}");
}

/// A service that decorates another takes over its ID, so a lookup of the
/// decorated ID is the decorator too, and the `decorates:` value is a
/// reference to the service it names.
#[tokio::test]
async fn a_decorated_service_is_typed_as_its_decorator_too() {
    let services = format!(
        "{SERVICES}  acme.cached_greeter:\n    class: Drupal\\acme\\CachedGreeter\n    decorates: acme.greeter\n"
    );
    let consumer = STATIC_CONSUMER.replace("$greeter;", "$greeter->greetFromCache();");
    let (backend, dir) = workspace(&[
        (SERVICES_PATH, services.as_str()),
        (
            "web/modules/custom/acme/src/CachedGreeter.php",
            "<?php\nnamespace Drupal\\acme;\nclass CachedGreeter extends Greeter {\n    public function greetFromCache(): string { return ''; }\n}\n",
        ),
        (STATIC_CONSUMER_PATH, consumer.as_str()),
    ])
    .await;
    let php = uri_for(&dir, STATIC_CONSUMER_PATH);
    let found = definitions_at(&backend, &php, position_in(&consumer, "greetFromCache", 1)).await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        found[0].uri,
        uri_for(&dir, "web/modules/custom/acme/src/CachedGreeter.php")
    );
    let diagnostics = crate::common::slow_diagnostic_messages(
        &backend,
        php.as_str(),
        &consumer,
        "unknown_member",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let yaml = uri_for(&dir, SERVICES_PATH);
    let found = references_at(
        &backend,
        &yaml,
        position_in(&services, "decorates: acme", 12),
    )
    .await;
    assert!(found.contains(&(yaml.clone(), 1)), "{found:?}");
    assert!(found.contains(&(yaml, 8)), "{found:?}");
}

/// `\Drupal::service(id: 'x')` names the same service as the positional
/// form, assigned or chained.
#[tokio::test]
async fn a_named_service_id_is_typed() {
    let named = STATIC_CONSUMER.replace("service('acme.greeter')", "service(id: 'acme.greeter')");
    let (backend, dir) = workspace(&[
        (SERVICES_PATH, SERVICES),
        (STATIC_CONSUMER_PATH, named.as_str()),
    ])
    .await;
    let position = position_in(&named, "$greeter;", 2);
    let hover = hover_text_at(
        &backend,
        &uri_for(&dir, STATIC_CONSUMER_PATH),
        position.line,
        position.character,
    )
    .await
    .unwrap_or_default();
    assert!(hover.contains("Greeter"), "{hover}");
}

/// Core's `\Drupal::service()`, with the conditional return type that types
/// a lookup by class name as that class.
const DRUPAL_WITH_CONDITIONAL_RETURN: &str = r#"<?php
class Drupal {
    /**
     * @param class-string<T>|string $id
     *
     * @template T of object
     *
     * @return ($id is class-string<T> ? T : object)
     */
    public static function service(string $id): object {}
}
"#;

const GREETER_INTERFACE: &str = "<?php\nnamespace Drupal\\acme;\ninterface GreeterInterface {\n    public function greetFormally(): string;\n}\n";

const INTERFACE_CONSUMER: &str = r#"<?php
namespace Drupal\acme;

final class InterfaceConsumer {
    public function run(): void {
        $greeter = \Drupal::service(GreeterInterface::class);
        $greeter;
        \Drupal::service(GreeterInterface::class)->
    }
}
"#;

const INTERFACE_CONSUMER_PATH: &str = "web/modules/custom/acme/src/InterfaceConsumer.php";

/// A service registered under its interface is fetched by the interface's
/// class name, and typed as that interface rather than as the class the
/// alias leads to.
#[tokio::test]
async fn a_service_fetched_by_its_interface_is_typed_as_the_interface() {
    let services = format!("{SERVICES}  Drupal\\acme\\GreeterInterface: '@acme.greeter'\n");
    let (backend, dir) = workspace(&[
        ("web/core/lib/Drupal.php", DRUPAL_WITH_CONDITIONAL_RETURN),
        (
            "web/modules/custom/acme/src/GreeterInterface.php",
            GREETER_INTERFACE,
        ),
        (SERVICES_PATH, services.as_str()),
        (INTERFACE_CONSUMER_PATH, INTERFACE_CONSUMER),
    ])
    .await;
    let consumer = uri_for(&dir, INTERFACE_CONSUMER_PATH);

    let position = position_in(INTERFACE_CONSUMER, "$greeter;", 2);
    let hover = hover_text_at(&backend, &consumer, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("GreeterInterface"), "{hover}");

    let needle = "GreeterInterface::class)->";
    let position = position_in(INTERFACE_CONSUMER, needle, needle.len());
    let labels = response_labels(
        completion_response(&backend, &consumer, position.line, position.character, None).await,
    );
    assert!(
        labels
            .iter()
            .any(|label| label.starts_with("greetFormally")),
        "{labels:?}"
    );
}

const LANGUAGE_MANAGER_SERVICES: &str =
    "services:\n  language_manager:\n    class: Drupal\\Core\\Language\\LanguageManager\n";

const LANGUAGE_SERVICE_PROVIDER: &str = r#"<?php
namespace Drupal\language;

use Drupal\Core\DependencyInjection\ServiceProviderBase;

class LanguageServiceProvider extends ServiceProviderBase {
    public function alter($container) {
        $definition = $container->getDefinition('language_manager');
        $definition->setClass('Drupal\language\ConfigurableLanguageManager');
    }
}
"#;

const LANGUAGE_CONSUMER: &str = r#"<?php
namespace Drupal\acme;

final class Translated {
    public function run(): void {
        $manager = \Drupal::service('language_manager');
        $manager->getLanguageConfigOverride('fr', 'system.site');
    }
}
"#;

const LANGUAGE_CONSUMER_PATH: &str = "web/modules/custom/acme/src/Translated.php";

/// A module's service provider may build a service another module declares
/// from its own class, so the service is either class.
#[tokio::test]
async fn a_service_a_provider_alters_is_typed_as_either_class() {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in [
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "/web/core/\n"),
        ("web/core/core.services.yml", LANGUAGE_MANAGER_SERVICES),
        (
            "web/core/lib/Drupal/Core/Language/LanguageManager.php",
            "<?php\nnamespace Drupal\\Core\\Language;\nclass LanguageManager {\n    public function getCurrentLanguage(): object {}\n}\n",
        ),
        (
            "web/core/modules/language/src/ConfigurableLanguageManager.php",
            "<?php\nnamespace Drupal\\language;\nclass ConfigurableLanguageManager extends \\Drupal\\Core\\Language\\LanguageManager {\n    public function getLanguageConfigOverride(string $langcode, string $name): object {}\n}\n",
        ),
        (
            "web/core/modules/language/src/LanguageServiceProvider.php",
            LANGUAGE_SERVICE_PROVIDER,
        ),
        // A test module's provider is not one any site loads.
        (
            "web/core/modules/system/tests/modules/fixture/src/FixtureServiceProvider.php",
            "<?php\nnamespace Drupal\\fixture;\nclass FixtureServiceProvider {\n    public function alter($container) {\n        $container->getDefinition('language_manager')->setClass('Drupal\\fixture\\Fake');\n    }\n}\n",
        ),
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_in(dir, &[(LANGUAGE_CONSUMER_PATH, LANGUAGE_CONSUMER)]).await;
    let php = uri_for(&dir, LANGUAGE_CONSUMER_PATH);
    let position = position_in(LANGUAGE_CONSUMER, "$manager->", 2);

    // Finding references indexes the workspace, core's YAML included.
    references_at(
        &backend,
        &php,
        position_in(LANGUAGE_CONSUMER, "service('language_manager')", 10),
    )
    .await;

    let hover = manager_hover(&backend, &php, position).await;
    assert!(hover.contains("ConfigurableLanguageManager"), "{hover}");
    assert!(hover.contains("$manager = LanguageManager"), "{hover}");
    assert!(!hover.contains("Fake"), "{hover}");

    let diagnostics = crate::common::slow_diagnostic_messages(
        &backend,
        php.as_str(),
        LANGUAGE_CONSUMER,
        "unknown_member",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    // A custom module's provider is read again as it is edited.
    let provider_path = "web/modules/custom/acme/src/AcmeServiceProvider.php";
    let provider = "<?php\nnamespace Drupal\\acme;\nclass AcmeServiceProvider {\n    public function alter($container) {\n        $container->getDefinition('language_manager')->setClass(Greeter::class);\n    }\n}\n";
    open_document(&backend, &uri_for(&dir, provider_path), "php", provider).await;
    let hover = manager_hover(&backend, &php, position).await;
    assert!(hover.contains("Greeter"), "{hover}");
    let edited = provider.replace("setClass(Greeter::class)", "setPublic(true)");
    backend.update_ast(uri_for(&dir, provider_path).as_str(), &edited);
    let hover = manager_hover(&backend, &php, position).await;
    assert!(!hover.contains("Greeter"), "{hover}");
}

async fn manager_hover(backend: &Backend, php: &Uri, position: Position) -> String {
    hover_text_at(backend, php, position.line, position.character)
        .await
        .unwrap_or_default()
}

/// Core as a site holds it, with what PHP code adds to the container and a
/// test module's services, and the custom module's services and hooks.
const UNKNOWN_SERVICE_SITE: &[(&str, &str)] = &[
    (".git/HEAD", "ref: refs/heads/main\n"),
    (".gitignore", "/web/core/\n"),
    (
        "web/core/core.services.yml",
        "services:\n  config.factory:\n    class: Drupal\\Core\\Config\\ConfigFactory\n",
    ),
    (
        "web/core/modules/system/tests/modules/fixture/fixture.services.yml",
        "services:\n  config.fixture:\n    class: Drupal\\Core\\Config\\ConfigFactory\n",
    ),
    (
        "web/core/lib/Drupal/Component/DependencyInjection/ContainerInterface.php",
        "<?php\nnamespace Drupal\\Component\\DependencyInjection;\ninterface ContainerInterface extends \\Symfony\\Component\\DependencyInjection\\ContainerInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/CoreServiceProvider.php",
        r#"<?php
namespace Drupal\Core;

use Symfony\Component\DependencyInjection\ContainerBuilder;

class CoreServiceProvider {
    public function register(ContainerBuilder $container) {
        $container->register('core.registered', \stdClass::class);
        $ids = ['core.listed'];
        foreach (['render', 'data'] as $bin) {
            $container->setAlias("cache.$bin", 'core.registered');
        }
    }
}
"#,
    ),
    (
        "web/core/lib/Drupal/Core/Installer.php",
        "<?php\nnamespace Drupal\\Core;\nclass Installer {\n    public function run($container) {\n        $container->set('core.set_elsewhere', new \\stdClass());\n        $message = 'core.not_a_service';\n    }\n}\n",
    ),
    (
        "web/modules/custom/acme/src/Hook/AcmeHooks.php",
        "<?php\nnamespace Drupal\\acme\\Hook;\nclass AcmeHooks {}\n",
    ),
    (SERVICES_PATH, SERVICES),
];

const LOOKUPS: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Component\DependencyInjection\ContainerInterface;
use Psr\Container\ContainerInterface as AnyContainer;

final class Lookups {
    public function run(ContainerInterface $container, AnyContainer $any): void {
        \Drupal::service('acme.greeter');
        \Drupal::service('acme.greeterr');
        $container->get('acme.typo');
        $container->get('config.factory');
        $container->get('config.fixture');
        $container->get('core.registered');
        $container->get('core.listed');
        $container->get('cache.render');
        $container->get('core.set_elsewhere');
        $container->get('core.not_a_service');
        $container->get('service_container');
        $container->get('Drupal\acme\Hook\AcmeHooks');
        $container->get('Drupal\acme\Missing');
        $container->get('acme.soft', 2);
        $any->get('acme.elsewhere');
        \Drupal::hasService('acme.maybe');
        $container->has('acme.maybe_too');
        if ($container->has('acme.checked')) {
            $container->get('acme.checked');
        }
    }
}
"#;

const LOOKUPS_PATH: &str = "web/modules/custom/acme/src/Lookups.php";

/// The unknown-service messages the diagnostics of `content`, open at
/// `path`, report.
fn unknown_services(
    backend: &Backend,
    dir: &tempfile::TempDir,
    path: &str,
    content: &str,
) -> Vec<String> {
    crate::common::slow_diagnostic_messages(
        backend,
        uri_for(dir, path).as_str(),
        content,
        "invalid_drupal_service",
    )
}

/// [`workspace_in`] holding [`UNKNOWN_SERVICE_SITE`] and `files`.
async fn unknown_service_site(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in UNKNOWN_SERVICE_SITE {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    workspace_in(dir, files).await
}

#[tokio::test]
async fn a_service_id_nothing_may_define_is_reported() {
    let (backend, dir) = unknown_service_site(&[(LOOKUPS_PATH, LOOKUPS)]).await;
    assert_eq!(
        unknown_services(&backend, &dir, LOOKUPS_PATH, LOOKUPS),
        vec![
            "Unknown service 'acme.greeterr'".to_string(),
            "Unknown service 'acme.typo'".to_string(),
            "Unknown service 'core.not_a_service'".to_string(),
            "Unknown service 'Drupal\\acme\\Missing'".to_string(),
        ]
    );
}

#[tokio::test]
async fn a_lookup_that_may_catch_a_missing_service_is_not_checked() {
    let lookups = LOOKUPS.replace(
        "final class Lookups {",
        "use Symfony\\Component\\DependencyInjection\\Exception\\ServiceNotFoundException;\n\nfinal class Lookups {",
    );
    let (backend, dir) = unknown_service_site(&[(LOOKUPS_PATH, lookups.as_str())]).await;
    assert!(unknown_services(&backend, &dir, LOOKUPS_PATH, &lookups).is_empty());
}

/// Code that runs only when a module is installed may look up its
/// services: a module the site lacks declares none it can see, and one it
/// has declares them all.
#[tokio::test]
async fn a_lookup_a_missing_module_guards_is_not_checked() {
    const GUARDED: &str = r#"<?php
namespace Drupal\acme;

final class Optional {
    public function inside(): void {
        if (\Drupal::moduleHandler()->moduleExists('absent_module')) {
            \Drupal::service('absent_module.helper');
        }
        \Drupal::service('absent_module.unguarded');
    }

    public function after(): void {
        if (!\Drupal::moduleHandler()->moduleExists('absent_module')) {
            return;
        }
        \Drupal::service('absent_module.after');
    }

    public function present(): void {
        if (\Drupal::moduleHandler()->moduleExists('acme')) {
            \Drupal::service('acme.typo_guarded');
        }
    }
}
"#;
    const GUARDED_PATH: &str = "web/modules/custom/acme/src/Optional.php";
    let (backend, dir) = unknown_service_site(&[
        (
            "web/modules/custom/acme/acme.info.yml",
            "name: Acme\ntype: module\n",
        ),
        (GUARDED_PATH, GUARDED),
    ])
    .await;
    assert_eq!(
        unknown_services(&backend, &dir, GUARDED_PATH, GUARDED),
        vec![
            "Unknown service 'absent_module.unguarded'".to_string(),
            "Unknown service 'acme.typo_guarded'".to_string(),
        ]
    );
}

#[tokio::test]
async fn test_code_is_not_checked() {
    let path = "web/modules/custom/acme/tests/src/Kernel/LookupsTest.php";
    let (backend, dir) = unknown_service_site(&[(path, LOOKUPS)]).await;
    assert!(unknown_services(&backend, &dir, path, LOOKUPS).is_empty());
}

#[tokio::test]
async fn a_service_provider_is_read_again_as_it_is_edited() {
    let provider_path = "web/modules/custom/acme/src/AcmeServiceProvider.php";
    let provider = "<?php\nnamespace Drupal\\acme;\nuse Drupal\\Core\\DependencyInjection\\ServiceProviderBase;\nclass AcmeServiceProvider extends ServiceProviderBase {\n    public function register($container) {\n        $container->register('acme.typo', Greeter::class);\n    }\n}\n";
    let (backend, dir) =
        unknown_service_site(&[(LOOKUPS_PATH, LOOKUPS), (provider_path, provider)]).await;
    let reported = unknown_services(&backend, &dir, LOOKUPS_PATH, LOOKUPS);
    assert!(
        !reported.contains(&"Unknown service 'acme.typo'".to_string()),
        "{reported:?}"
    );

    let edited = provider.replace("'acme.typo'", "'acme.other'");
    backend.update_ast(uri_for(&dir, provider_path).as_str(), &edited);
    let reported = unknown_services(&backend, &dir, LOOKUPS_PATH, LOOKUPS);
    assert!(
        reported.contains(&"Unknown service 'acme.typo'".to_string()),
        "{reported:?}"
    );
}
