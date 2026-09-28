use crate::common::create_psr4_workspace;
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

async fn open_doc(backend: &Backend, uri: Url, language_id: &str, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: language_id.to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Url {
    Url::from_file_path(dir.path().join(rel)).unwrap()
}

fn edit_texts_for_uri(edit: &WorkspaceEdit, uri: &Url) -> Vec<String> {
    edit.changes
        .as_ref()
        .and_then(|changes| changes.get(uri))
        .map(|edits| edits.iter().map(|edit| edit.new_text.clone()).collect())
        .unwrap_or_default()
}

fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    let offset = content.find(needle).expect("needle should exist") + inside;
    let prefix = &content[..offset];
    Position::new(
        prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
        prefix
            .rsplit_once('\n')
            .map_or(prefix.len(), |(_, line)| line.len()) as u32,
    )
}

#[tokio::test]
async fn symfony_yaml_service_class_goes_to_php_definition() {
    let service_php = "<?php\nnamespace App\\Service;\nclass Mailer {}\n";
    let services_yaml =
        "services:\n  App\\Service\\Mailer:\n    arguments: ['@App\\Service\\Mailer']\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Service/Mailer.php", service_php),
            ("config/services.yaml", services_yaml),
        ],
    );

    let service_uri = uri_for(&dir, "src/Service/Mailer.php");
    let yaml_uri = uri_for(&dir, "config/services.yaml");
    open_doc(&backend, service_uri.clone(), "php", service_php).await;
    open_doc(&backend, yaml_uri.clone(), "yaml", services_yaml).await;

    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: yaml_uri },
                position: Position::new(1, 15),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("YAML service class should resolve to PHP class");

    let GotoDefinitionResponse::Scalar(location) = result else {
        panic!("expected a single definition location");
    };
    assert!(
        location.uri.path().ends_with("/src/Service/Mailer.php"),
        "expected Mailer.php, got {}",
        location.uri
    );
}

#[tokio::test]
async fn class_references_include_symfony_and_doctrine_yaml_xml() {
    let user_php = "<?php\nnamespace App\\Entity;\nclass User {}\n";
    let repo_php = "<?php\nnamespace App\\Repository;\nclass UserRepository {}\n";
    let services_yaml = "services:\n  app.user_service:\n    class: App\\Entity\\User\n";
    let doctrine_yaml =
        "App\\Entity\\User:\n  type: entity\n  repositoryClass: App\\Repository\\UserRepository\n";
    let doctrine_xml = r#"<doctrine-mapping>
  <entity name="App\Entity\User" repository-class="App\Repository\UserRepository" />
</doctrine-mapping>
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Entity/User.php", user_php),
            ("src/Repository/UserRepository.php", repo_php),
            ("config/services.yaml", services_yaml),
            ("config/doctrine/User.orm.yaml", doctrine_yaml),
            ("config/doctrine/User.orm.xml", doctrine_xml),
        ],
    );

    let user_uri = uri_for(&dir, "src/Entity/User.php");
    open_doc(&backend, user_uri.clone(), "php", user_php).await;
    open_doc(
        &backend,
        uri_for(&dir, "src/Repository/UserRepository.php"),
        "php",
        repo_php,
    )
    .await;
    open_doc(
        &backend,
        uri_for(&dir, "config/services.yaml"),
        "yaml",
        services_yaml,
    )
    .await;
    open_doc(
        &backend,
        uri_for(&dir, "config/doctrine/User.orm.yaml"),
        "yaml",
        doctrine_yaml,
    )
    .await;
    open_doc(
        &backend,
        uri_for(&dir, "config/doctrine/User.orm.xml"),
        "xml",
        doctrine_xml,
    )
    .await;

    let refs = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: user_uri },
                position: Position::new(2, 7),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .expect("class references should include framework resources");

    let paths: Vec<String> = refs.iter().map(|loc| loc.uri.path().to_string()).collect();
    assert!(
        paths.iter().any(|p| p.ends_with("/config/services.yaml")),
        "expected services.yaml reference, got {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|p| p.ends_with("/config/doctrine/User.orm.yaml")),
        "expected Doctrine YAML reference, got {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|p| p.ends_with("/config/doctrine/User.orm.xml")),
        "expected Doctrine XML reference, got {paths:?}"
    );
}

#[tokio::test]
async fn class_rename_updates_symfony_and_doctrine_resources() {
    let user_php = "<?php\nnamespace App\\Entity;\nclass User {}\n";
    let services_yaml = "services:\n  app.user_service:\n    class: App\\Entity\\User\n";
    let doctrine_xml = r#"<doctrine-mapping>
  <entity name="App\Entity\User" />
</doctrine-mapping>
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Entity/User.php", user_php),
            ("config/services.yaml", services_yaml),
            ("config/doctrine/User.orm.xml", doctrine_xml),
        ],
    );

    let user_uri = uri_for(&dir, "src/Entity/User.php");
    let yaml_uri = uri_for(&dir, "config/services.yaml");
    let xml_uri = uri_for(&dir, "config/doctrine/User.orm.xml");
    open_doc(&backend, user_uri.clone(), "php", user_php).await;
    open_doc(&backend, yaml_uri.clone(), "yaml", services_yaml).await;
    open_doc(&backend, xml_uri.clone(), "xml", doctrine_xml).await;

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: user_uri },
                position: Position::new(2, 7),
            },
            new_name: "Customer".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("class rename should produce edits");

    assert!(
        edit_texts_for_uri(&edit, &yaml_uri)
            .iter()
            .any(|text| text == "App\\Entity\\Customer"),
        "expected services.yaml class edit, got {:?}",
        edit_texts_for_uri(&edit, &yaml_uri)
    );
    assert!(
        edit_texts_for_uri(&edit, &xml_uri)
            .iter()
            .any(|text| text == "App\\Entity\\Customer"),
        "expected Doctrine XML class edit, got {:?}",
        edit_texts_for_uri(&edit, &xml_uri)
    );
}

#[tokio::test]
async fn symfony_route_controller_action_resolves_and_renames_method() {
    let controller_php = "<?php\nnamespace App\\Controller;\nclass HomeController {\n    public function index(): void {}\n}\n";
    let routes_yaml = "home:\n  path: /\n  controller: App\\Controller\\HomeController::index\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Controller/HomeController.php", controller_php),
            ("config/routes.yaml", routes_yaml),
        ],
    );

    let controller_uri = uri_for(&dir, "src/Controller/HomeController.php");
    let routes_uri = uri_for(&dir, "config/routes.yaml");
    open_doc(&backend, controller_uri.clone(), "php", controller_php).await;
    open_doc(&backend, routes_uri.clone(), "yaml", routes_yaml).await;

    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: routes_uri.clone(),
                },
                position: Position::new(2, 56),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("controller method should resolve");
    let GotoDefinitionResponse::Scalar(location) = definition else {
        panic!("expected a single method definition");
    };
    assert_eq!(location.range.start.line, 3);

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: routes_uri.clone(),
                },
                position: Position::new(2, 56),
            },
            new_name: "dashboard".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("controller action rename should produce edits");

    assert!(
        edit_texts_for_uri(&edit, &routes_uri)
            .iter()
            .any(|text| text == "dashboard"),
        "expected route controller action edit, got {:?}",
        edit_texts_for_uri(&edit, &routes_uri)
    );
    assert!(
        edit_texts_for_uri(&edit, &controller_uri)
            .iter()
            .any(|text| text == "dashboard"),
        "expected PHP method declaration edit, got {:?}",
        edit_texts_for_uri(&edit, &controller_uri)
    );
}

#[tokio::test]
async fn symfony_php_service_config_links_class_references_and_code_lens() {
    let service_php = "<?php\nnamespace App\\Service;\nclass Mailer {}\n";
    let services_php = r#"<?php
namespace Symfony\Component\DependencyInjection\Loader\Configurator;

use Symfony\Component\DependencyInjection\Loader\Configurator\App;
use App\Service\Mailer;

return App::config([
    'services' => [
        Mailer::class => [],
        'App\\Service\\Mailer' => [],
    ],
]);
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Service/Mailer.php", service_php),
            ("config/services.php", services_php),
        ],
    );

    let service_uri = uri_for(&dir, "src/Service/Mailer.php");
    let config_uri = uri_for(&dir, "config/services.php");
    open_doc(&backend, service_uri.clone(), "php", service_php).await;
    open_doc(&backend, config_uri.clone(), "php", services_php).await;

    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: config_uri.clone(),
                },
                position: position_in(services_php, "App\\\\Service\\\\Mailer", 5),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("PHP service string should resolve to its class");
    let GotoDefinitionResponse::Scalar(location) = definition else {
        panic!("expected a single definition location");
    };
    assert_eq!(location.uri, service_uri);

    let lenses = backend
        .handle_code_lens(service_uri.as_str(), service_php)
        .unwrap_or_default();
    let titles: Vec<&str> = lenses
        .iter()
        .filter_map(|lens| lens.command.as_ref().map(|command| command.title.as_str()))
        .collect();
    assert!(
        titles.contains(&"Symfony/Doctrine config: 2 refs"),
        "expected PHP service references in the class code lens, got {titles:?}"
    );

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: service_uri.clone(),
                },
                position: position_in(service_php, "class Mailer", 7),
            },
            new_name: "MessageMailer".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("class rename should update PHP service config");
    let config_edits = edit_texts_for_uri(&edit, &config_uri);
    assert!(
        config_edits.iter().any(|text| text == "MessageMailer"),
        "expected imported class-constant edit, got {config_edits:?}"
    );
    assert!(
        config_edits
            .iter()
            .any(|text| text == "App\\\\Service\\\\MessageMailer"),
        "expected escaped service class edit, got {config_edits:?}"
    );
    // `Mailer::class` is both a PHP class reference and a framework one;
    // it must be edited once, not by both renames.
    let mut ranges: Vec<Range> = edit.changes.as_ref().unwrap()[&config_uri]
        .iter()
        .map(|edit| edit.range)
        .collect();
    ranges.sort_by_key(|range| (range.start.line, range.start.character));
    assert!(
        ranges.windows(2).all(|pair| pair[0].end <= pair[1].start),
        "config edits must not overlap: {ranges:?}"
    );
}

#[tokio::test]
async fn symfony_php_route_config_links_callable_methods() {
    let controller_php = "<?php\nnamespace App\\Controller;\nclass HomeController {\n    public function index(): void {}\n}\n";
    let routes_php = r#"<?php
namespace Symfony\Component\Routing\Loader\Configurator;

use App\Controller\HomeController;

return static function (RoutingConfigurator $routes): void {
    $routes->add('home', '/')->controller([HomeController::class, 'index']);
    $routes->add('other', '/other')->controller('App\\Controller\\HomeController::index');
    $routes->import('../src/Controller/', 'attribute');
};
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Controller/HomeController.php", controller_php),
            ("config/routes.php", routes_php),
        ],
    );

    let controller_uri = uri_for(&dir, "src/Controller/HomeController.php");
    let routes_uri = uri_for(&dir, "config/routes.php");
    open_doc(&backend, controller_uri.clone(), "php", controller_php).await;
    open_doc(&backend, routes_uri.clone(), "php", routes_php).await;

    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: routes_uri.clone(),
                },
                position: position_in(routes_php, "'index'", 2),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("PHP route callable should resolve to its method");
    let GotoDefinitionResponse::Scalar(location) = definition else {
        panic!("expected a single method definition");
    };
    assert_eq!(location.uri, controller_uri);
    assert_eq!(location.range.start.line, 3);

    let lenses = backend
        .handle_code_lens(controller_uri.as_str(), controller_php)
        .unwrap_or_default();
    let titles: Vec<&str> = lenses
        .iter()
        .filter_map(|lens| lens.command.as_ref().map(|command| command.title.as_str()))
        .collect();
    assert!(
        titles.contains(&"Symfony route config: 2 refs"),
        "expected PHP route references in the method code lens, got {titles:?}"
    );

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: controller_uri,
                },
                position: position_in(controller_php, "function index", 10),
            },
            new_name: "dashboard".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("method rename should update PHP route config");
    let route_edits = edit_texts_for_uri(&edit, &routes_uri);
    assert_eq!(
        route_edits
            .iter()
            .filter(|text| text.as_str() == "dashboard")
            .count(),
        2,
        "expected both PHP route callables to be renamed, got {route_edits:?}"
    );
}

#[tokio::test]
async fn symfony_namespace_prefix_rename_updates_yaml_and_php_namespace() {
    let mailer_php = "<?php\nnamespace App\\Service;\nclass Mailer {}\n";
    let services_yaml = "services:\n  App\\Service\\:\n    resource: '../src/Service/'\n  App\\Service\\Mailer: ~\n";
    let services_php = r#"<?php
namespace Symfony\Component\DependencyInjection\Loader\Configurator;

return static function (ContainerConfigurator $container): void {
    $container->services()->load('App\\Service\\', '../src/Service/');
};
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Service/Mailer.php", mailer_php),
            ("config/services.yaml", services_yaml),
            ("config/services.php", services_php),
        ],
    );

    let mailer_uri = uri_for(&dir, "src/Service/Mailer.php");
    let yaml_uri = uri_for(&dir, "config/services.yaml");
    let php_config_uri = uri_for(&dir, "config/services.php");
    open_doc(&backend, mailer_uri.clone(), "php", mailer_php).await;
    open_doc(&backend, yaml_uri.clone(), "yaml", services_yaml).await;
    open_doc(&backend, php_config_uri.clone(), "php", services_php).await;

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: yaml_uri.clone(),
                },
                position: Position::new(1, 8),
            },
            new_name: "Domain".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("namespace-prefix rename should produce edits");

    let yaml_edits = edit_texts_for_uri(&edit, &yaml_uri);
    assert!(
        yaml_edits.iter().any(|text| text == "App\\Domain\\"),
        "expected YAML namespace-prefix edit, got {yaml_edits:?}"
    );
    assert!(
        yaml_edits.iter().any(|text| text == "App\\Domain\\Mailer"),
        "expected YAML class-reference edit, got {yaml_edits:?}"
    );
    assert!(
        yaml_edits.iter().any(|text| text == "../src/Domain/"),
        "expected YAML resource path edit, got {yaml_edits:?}"
    );
    assert!(
        edit_texts_for_uri(&edit, &mailer_uri)
            .iter()
            .any(|text| text == "App\\Domain"),
        "expected PHP namespace declaration edit, got {:?}",
        edit_texts_for_uri(&edit, &mailer_uri)
    );
    let php_config_edits = edit_texts_for_uri(&edit, &php_config_uri);
    assert!(
        php_config_edits
            .iter()
            .any(|text| text == "App\\\\Domain\\\\"),
        "expected PHP configurator namespace-prefix edit, got {php_config_edits:?}"
    );
    assert!(
        php_config_edits.iter().any(|text| text == "../src/Domain/"),
        "expected PHP configurator resource path edit, got {php_config_edits:?}"
    );
}

#[tokio::test]
async fn symfony_service_ids_and_parameters_work_across_yaml_and_php() {
    let mailer_php = "<?php\nnamespace App\\Service;\nclass Mailer {}\n";
    let services_yaml = r#"parameters:
  app.sender_name: PHPantom
services:
  app.mailer:
    class: App\Service\Mailer
    arguments: ['%app.sender_name%']
  app.mailer_alias: '@app.mailer'
"#;
    let consumer_php = r#"<?php
namespace App\Controller;

use Symfony\Component\DependencyInjection\Attribute\Autowire;
use Symfony\Component\DependencyInjection\ContainerInterface;

final class MailController
{
    public function __construct(
        #[Autowire(param: 'app.sender_name')]
        private string $sender,
    ) {}

    public function send(ContainerInterface $container): void
    {
        $container->get('app.mailer');
    }
}
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Service/Mailer.php", mailer_php),
            ("src/Controller/MailController.php", consumer_php),
            ("config/services.yaml", services_yaml),
        ],
    );
    let mailer_uri = uri_for(&dir, "src/Service/Mailer.php");
    let consumer_uri = uri_for(&dir, "src/Controller/MailController.php");
    let yaml_uri = uri_for(&dir, "config/services.yaml");
    open_doc(&backend, mailer_uri.clone(), "php", mailer_php).await;
    open_doc(&backend, yaml_uri.clone(), "yaml", services_yaml).await;
    open_doc(&backend, consumer_uri.clone(), "php", consumer_php).await;

    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: consumer_uri.clone(),
                },
                position: position_in(consumer_php, "app.mailer", 5),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("service ID usage should resolve to its declaration");
    let locations = match definition {
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
    };
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].uri, yaml_uri);
    assert_eq!(locations[0].range.start.line, 3);

    let lenses = backend
        .handle_code_lens(yaml_uri.as_str(), services_yaml)
        .unwrap_or_default();
    let titles = lenses
        .iter()
        .filter_map(|lens| lens.command.as_ref().map(|command| command.title.as_str()))
        .collect::<Vec<_>>();
    assert!(
        titles.contains(&"Symfony service: 2 refs"),
        "expected declaration-side service reference lens, got {titles:?}"
    );
    assert!(
        titles.contains(&"Symfony service class: Mailer"),
        "expected service declaration to link to its PHP class, got {titles:?}"
    );

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: yaml_uri.clone(),
                },
                position: position_in(services_yaml, "app.mailer:", 5),
            },
            new_name: "app.message_mailer".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("service ID rename should update its usages");
    assert!(
        edit_texts_for_uri(&edit, &consumer_uri)
            .iter()
            .any(|text| text == "app.message_mailer"),
        "expected PHP container lookup edit"
    );
    assert!(
        edit_texts_for_uri(&edit, &yaml_uri)
            .iter()
            .filter(|text| text.as_str() == "app.message_mailer")
            .count()
            >= 2,
        "expected YAML declaration and alias edits"
    );
}

#[tokio::test]
async fn symfony_service_and_parameter_completion_uses_workspace_declarations() {
    let services_yaml = "parameters:\n  app.sender_name: PHPantom\nservices:\n  app.mailer: ~\n";
    let consumer_php = r#"<?php
use Symfony\Component\DependencyInjection\Attribute\Autowire;
use Symfony\Component\DependencyInjection\ContainerInterface;

function send(ContainerInterface $container): void {
    $container->get('app.m');
}

#[Autowire(param: 'app.s')]
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("config/services.yaml", services_yaml),
            ("src/consumer.php", consumer_php),
        ],
    );
    let yaml_uri = uri_for(&dir, "config/services.yaml");
    let consumer_uri = uri_for(&dir, "src/consumer.php");
    open_doc(&backend, yaml_uri, "yaml", services_yaml).await;
    open_doc(&backend, consumer_uri.clone(), "php", consumer_php).await;

    for (needle, expected) in [("app.m", "app.mailer"), ("app.s", "app.sender_name")] {
        let response = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: consumer_uri.clone(),
                    },
                    position: position_in(consumer_php, needle, needle.len()),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .unwrap()
            .expect("Symfony completion should return candidates");
        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        assert!(
            items.iter().any(|item| item.label == expected),
            "expected {expected} completion, got {:?}",
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn symfony_xml_service_alias_resolves_to_service_declaration() {
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
    let (backend, dir) = create_psr4_workspace(COMPOSER, &[("config/services.xml", services_xml)]);
    let xml_uri = uri_for(&dir, "config/services.xml");
    open_doc(&backend, xml_uri.clone(), "xml", services_xml).await;

    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: xml_uri.clone(),
                },
                position: position_in(services_xml, "alias=\"app.mailer\"", 10),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("XML service alias should resolve");
    let location = match definition {
        GotoDefinitionResponse::Scalar(location) => location,
        GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
        GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
    };
    assert_eq!(location.uri, xml_uri);
    assert_eq!(location.range.start.line, 6);
}

#[tokio::test]
async fn symfony_reports_only_missing_project_local_container_symbols() {
    let services_yaml = "parameters:\n  app.sender: PHPantom\nservices:\n  app.mailer: ~\n";
    let consumer_php = r#"<?php
use Symfony\Component\DependencyInjection\ContainerInterface;

function send(ContainerInterface $container): void {
    $container->get('app.mailer');
    $container->get('app.missing');
    $container->getParameter('app.sender');
    $container->getParameter('app.missing_parameter');
    $container->get('vendor.dynamic_service');
}
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("config/services.yaml", services_yaml),
            ("src/consumer.php", consumer_php),
        ],
    );
    let yaml_uri = uri_for(&dir, "config/services.yaml");
    let consumer_uri = uri_for(&dir, "src/consumer.php");
    open_doc(&backend, yaml_uri, "yaml", services_yaml).await;
    open_doc(&backend, consumer_uri.clone(), "php", consumer_php).await;

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(consumer_uri.as_str(), consumer_php, &mut diagnostics);
    let symfony = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code.starts_with("unknown_symfony_")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        symfony.len(),
        2,
        "expected only missing app-local symbols, got {symfony:?}"
    );
    assert!(
        symfony
            .iter()
            .any(|diagnostic| diagnostic.message.contains("app.missing'"))
    );
    assert!(
        symfony
            .iter()
            .any(|diagnostic| diagnostic.message.contains("app.missing_parameter'"))
    );
}

#[tokio::test]
async fn symfony_php_configurator_declares_services_and_parameters() {
    let mailer_php = "<?php\nnamespace App\\Service;\nclass Mailer {}\n";
    let services_php = r#"<?php
namespace Symfony\Component\DependencyInjection\Loader\Configurator;

use App\Service\Mailer;

return static function (ContainerConfigurator $container): void {
    $services = $container->services();
    $parameters = $container->parameters();
    $services->set('app.php_mailer', Mailer::class);
    $parameters->set('app.php_sender', 'PHPantom');
    $services->alias('app.php_mailer_alias', 'app.php_mailer');
};
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Service/Mailer.php", mailer_php),
            ("config/services.php", services_php),
        ],
    );
    let mailer_uri = uri_for(&dir, "src/Service/Mailer.php");
    let config_uri = uri_for(&dir, "config/services.php");
    open_doc(&backend, mailer_uri, "php", mailer_php).await;
    open_doc(&backend, config_uri.clone(), "php", services_php).await;

    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: config_uri.clone(),
                },
                position: position_in(services_php, "'app.php_mailer');", "'app.php_".len()),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("PHP service alias target should resolve");
    let location = match definition {
        GotoDefinitionResponse::Scalar(location) => location,
        GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
        GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
    };
    assert_eq!(location.uri, config_uri);
    assert_eq!(location.range.start.line, 8);

    let lenses = backend
        .handle_code_lens(config_uri.as_str(), services_php)
        .unwrap_or_default();
    let titles = lenses
        .iter()
        .filter_map(|lens| lens.command.as_ref().map(|command| command.title.as_str()))
        .collect::<Vec<_>>();
    assert!(
        titles.contains(&"Symfony service: 1 ref"),
        "expected PHP declaration-side service lens, got {titles:?}"
    );
    assert!(
        titles.contains(&"Symfony service class: Mailer"),
        "expected PHP service declaration class lens, got {titles:?}"
    );
}

#[tokio::test]
async fn renaming_a_controller_action_from_php_updates_the_route() {
    let controller_php = "<?php\nnamespace App\\Controller;\nclass HomeController {\n    public function index(): void {}\n}\n";
    let routes_yaml = "home:\n  path: /\n  controller: App\\Controller\\HomeController::index\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Controller/HomeController.php", controller_php),
            ("config/routes.yaml", routes_yaml),
        ],
    );

    let controller_uri = uri_for(&dir, "src/Controller/HomeController.php");
    let routes_uri = uri_for(&dir, "config/routes.yaml");
    open_doc(&backend, controller_uri.clone(), "php", controller_php).await;
    open_doc(&backend, routes_uri.clone(), "yaml", routes_yaml).await;

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: controller_uri.clone(),
                },
                position: Position::new(3, 21),
            },
            new_name: "dashboard".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("method rename should produce edits");

    assert_eq!(edit_texts_for_uri(&edit, &routes_uri), vec!["dashboard"]);
    assert_eq!(
        edit_texts_for_uri(&edit, &controller_uri),
        vec!["dashboard"]
    );
}

#[tokio::test]
async fn symfony_route_names_work_across_yaml_php_and_twig() {
    let controller_php = "<?php\nnamespace App\\Controller;\nclass HomeController {\n    public function index(): void {}\n}\n";
    let routes_yaml = r#"app_home:
  path: /users/{userId}
  controller: App\Controller\HomeController::index
"#;
    let consumer_php = r#"<?php
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;

final class Consumer extends AbstractController
{
    public function run(): void
    {
        $this->redirectToRoute('app_home', ['userId' => 1]);
        $this->generateUrl('app_home');
        $this->redirectToRoute('app_missing');
    }
}
"#;
    let template = "<a href=\"{{ path('app_home', {'userId': 1}) }}\">Home</a>\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("src/Controller/HomeController.php", controller_php),
            ("src/Consumer.php", consumer_php),
            ("config/routes.yaml", routes_yaml),
            ("templates/home.html.twig", template),
        ],
    );
    let controller_uri = uri_for(&dir, "src/Controller/HomeController.php");
    let consumer_uri = uri_for(&dir, "src/Consumer.php");
    let routes_uri = uri_for(&dir, "config/routes.yaml");
    let template_uri = uri_for(&dir, "templates/home.html.twig");
    open_doc(&backend, controller_uri, "php", controller_php).await;
    open_doc(&backend, routes_uri.clone(), "yaml", routes_yaml).await;
    open_doc(&backend, consumer_uri.clone(), "php", consumer_php).await;
    open_doc(&backend, template_uri.clone(), "twig", template).await;

    for (uri, content) in [(&consumer_uri, consumer_php), (&template_uri, template)] {
        let definition = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: position_in(content, "app_home", 4),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap()
            .expect("route usage should resolve to YAML declaration");
        let location = match definition {
            GotoDefinitionResponse::Scalar(location) => location,
            GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
            GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
        };
        assert_eq!(location.uri, routes_uri);
        assert_eq!(location.range.start.line, 0);
    }

    for (uri, content) in [(&consumer_uri, consumer_php), (&template_uri, template)] {
        let response = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: position_in(content, "app_home", 5),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .unwrap()
            .expect("route completion should return candidates");
        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        assert!(
            items.iter().any(|item| item.label == "app_home"),
            "expected app_home completion"
        );
    }

    let lenses = backend
        .handle_code_lens(routes_uri.as_str(), routes_yaml)
        .unwrap_or_default();
    let titles = lenses
        .iter()
        .filter_map(|lens| lens.command.as_ref().map(|command| command.title.as_str()))
        .collect::<Vec<_>>();
    assert!(
        titles.contains(&"Symfony route: 3 refs"),
        "expected route reference lens, got {titles:?}"
    );
    assert!(
        titles.contains(&"Symfony controller: HomeController::index"),
        "expected route-to-controller lens, got {titles:?}"
    );

    for (uri, content) in [(&consumer_uri, consumer_php), (&template_uri, template)] {
        let response = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: position_in(content, "userId", 4),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .unwrap()
            .expect("route parameter completion should return candidates");
        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        assert!(
            items.iter().any(|item| item.label == "userId"),
            "expected userId route parameter completion"
        );
    }

    let parameter_definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: consumer_uri.clone(),
                },
                position: position_in(consumer_php, "userId", 3),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("route parameter should resolve to its path placeholder");
    let parameter_location = match parameter_definition {
        GotoDefinitionResponse::Scalar(location) => location,
        GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
        GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
    };
    assert_eq!(parameter_location.uri, routes_uri);
    assert_eq!(parameter_location.range.start.line, 1);

    let parameter_edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: routes_uri.clone(),
                },
                position: position_in(routes_yaml, "userId", 3),
            },
            new_name: "accountId".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("route parameter rename should update call sites");
    assert!(
        edit_texts_for_uri(&parameter_edit, &consumer_uri)
            .iter()
            .any(|text| text == "accountId")
    );
    assert!(
        edit_texts_for_uri(&parameter_edit, &template_uri)
            .iter()
            .any(|text| text == "accountId")
    );

    let edit = backend
        .rename(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: routes_uri.clone(),
                },
                position: position_in(routes_yaml, "app_home", 4),
            },
            new_name: "app_dashboard".to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("route rename should update PHP and Twig usages");
    assert_eq!(
        edit_texts_for_uri(&edit, &consumer_uri)
            .iter()
            .filter(|text| text.as_str() == "app_dashboard")
            .count(),
        2
    );
    assert!(
        edit_texts_for_uri(&edit, &template_uri)
            .iter()
            .any(|text| text == "app_dashboard")
    );

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(consumer_uri.as_str(), consumer_php, &mut diagnostics);
    assert!(
        diagnostics.iter().any(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code == "unknown_symfony_route"
            ) && diagnostic.message.contains("app_missing")
        }),
        "expected unknown project-local route diagnostic"
    );
}

#[tokio::test]
async fn symfony_routes_are_declared_by_xml_php_and_attributes() {
    let routes_xml = r#"<?xml version="1.0"?>
<routes>
  <route id="app_xml" path="/xml/{xmlId}"/>
</routes>
"#;
    let routes_php = r#"<?php
namespace Symfony\Component\Routing\Loader\Configurator;

return static function (RoutingConfigurator $routes): void {
    $routes->add('app_php', '/php/{phpId}');
};
"#;
    let controller_php = r#"<?php
namespace App\Controller;

use Symfony\Component\Routing\Attribute\Route;

final class AttributeController
{
    #[Route('/attribute/{attributeId}', name: 'app_attribute')]
    public function index(): void {}
}
"#;
    let consumer_php = r#"<?php
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;

final class Consumer extends AbstractController
{
    public function run(): void
    {
        $this->generateUrl('app_xml', ['xmlId' => 1]);
        $this->generateUrl('app_php', ['phpId' => 1]);
        $this->generateUrl('app_attribute', ['attributeId' => 1]);
    }
}

"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("config/routes.xml", routes_xml),
            ("config/routes.php", routes_php),
            ("src/Controller/AttributeController.php", controller_php),
            ("src/Consumer.php", consumer_php),
        ],
    );
    let xml_uri = uri_for(&dir, "config/routes.xml");
    let php_routes_uri = uri_for(&dir, "config/routes.php");
    let controller_uri = uri_for(&dir, "src/Controller/AttributeController.php");
    let consumer_uri = uri_for(&dir, "src/Consumer.php");
    open_doc(&backend, xml_uri.clone(), "xml", routes_xml).await;
    open_doc(&backend, php_routes_uri.clone(), "php", routes_php).await;
    open_doc(&backend, controller_uri.clone(), "php", controller_php).await;
    open_doc(&backend, consumer_uri.clone(), "php", consumer_php).await;

    for (name, expected_uri) in [
        ("app_xml", &xml_uri),
        ("app_php", &php_routes_uri),
        ("app_attribute", &controller_uri),
    ] {
        let definition = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: consumer_uri.clone(),
                    },
                    position: position_in(consumer_php, name, 4),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap()
            .expect("route reference should resolve");
        let location = match definition {
            GotoDefinitionResponse::Scalar(location) => location,
            GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
            GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
        };
        assert_eq!(&location.uri, expected_uri, "wrong definition for {name}");
    }

    for (name, expected_uri) in [
        ("xmlId", &xml_uri),
        ("phpId", &php_routes_uri),
        ("attributeId", &controller_uri),
    ] {
        let definition = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: consumer_uri.clone(),
                    },
                    position: position_in(consumer_php, name, 3),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap()
            .expect("route parameter reference should resolve");
        let location = match definition {
            GotoDefinitionResponse::Scalar(location) => location,
            GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
            GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
        };
        assert_eq!(
            &location.uri, expected_uri,
            "wrong parameter definition for {name}"
        );
    }
}
#[tokio::test]
async fn symfony_twig_templates_complete_navigate_reference_and_show_lenses() {
    let base_template = "<main>{% block body %}{% endblock %}</main>\n";
    let card_template = "<article>Card</article>\n";
    let page_template = r#"{% extends 'base.html.twig' %}
{% block body %}
  {% include 'partials/card.html.twig' %}
{% endblock %}
"#;
    let controller_php = r#"<?php
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Bridge\Twig\Mime\TemplatedEmail;

final class PageController extends AbstractController
{
    public function show(): void
    {
        $this->render('page.html.twig');
        (new TemplatedEmail())->htmlTemplate('partials/card.html.twig');
    }
}
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("templates/base.html.twig", base_template),
            ("templates/partials/card.html.twig", card_template),
            ("templates/page.html.twig", page_template),
            ("src/PageController.php", controller_php),
        ],
    );
    let base_uri = uri_for(&dir, "templates/base.html.twig");
    let card_uri = uri_for(&dir, "templates/partials/card.html.twig");
    let page_uri = uri_for(&dir, "templates/page.html.twig");
    let controller_uri = uri_for(&dir, "src/PageController.php");
    open_doc(&backend, base_uri.clone(), "twig", base_template).await;
    open_doc(&backend, card_uri.clone(), "twig", card_template).await;
    open_doc(&backend, page_uri.clone(), "twig", page_template).await;
    open_doc(&backend, controller_uri.clone(), "php", controller_php).await;

    for (uri, content, name, expected_uri) in [
        (&page_uri, page_template, "base.html.twig", &base_uri),
        (
            &page_uri,
            page_template,
            "partials/card.html.twig",
            &card_uri,
        ),
        (&controller_uri, controller_php, "page.html.twig", &page_uri),
    ] {
        let definition = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: position_in(content, name, 3),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap()
            .expect("template reference should resolve");
        let location = match definition {
            GotoDefinitionResponse::Scalar(location) => location,
            GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
            GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
        };
        assert_eq!(&location.uri, expected_uri, "wrong definition for {name}");
        assert_eq!(location.range.start, Position::new(0, 0));
    }

    for (uri, content, name) in [
        (&page_uri, page_template, "base.html.twig"),
        (&controller_uri, controller_php, "page.html.twig"),
    ] {
        let response = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: position_in(content, name, 5),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .unwrap()
            .expect("template completion should return candidates");
        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        assert!(
            items.iter().any(|item| item.label == name),
            "expected {name} completion, got {:?}",
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>()
        );
    }

    let references = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: controller_uri.clone(),
                },
                position: position_in(controller_php, "page.html.twig", 4),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .expect("template references should be returned");
    assert!(references.iter().any(|location| location.uri == page_uri));

    let lenses = backend
        .handle_code_lens(base_uri.as_str(), base_template)
        .unwrap_or_default();
    assert!(
        lenses.iter().any(|lens| {
            lens.command
                .as_ref()
                .is_some_and(|command| command.title == "Symfony template: 1 ref")
        }),
        "expected a declaration-side Twig reference lens, got {lenses:?}"
    );
}

#[tokio::test]
async fn symfony_missing_template_diagnostic_offers_create_template_action() {
    let controller_php = r#"<?php
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;

final class PageController extends AbstractController
{
    public function show(): void
    {
        $this->render('missing/page.html.twig');
        $this->render('@Vendor/external.html.twig');
    }
}
"#;
    let (backend, dir) =
        create_psr4_workspace(COMPOSER, &[("src/PageController.php", controller_php)]);
    let controller_uri = uri_for(&dir, "src/PageController.php");
    open_doc(&backend, controller_uri.clone(), "php", controller_php).await;

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(controller_uri.as_str(), controller_php, &mut diagnostics);
    let template_diagnostics = diagnostics
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code == "unknown_symfony_template"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        template_diagnostics.len(),
        1,
        "namespaced vendor templates should not be diagnosed"
    );
    assert!(
        template_diagnostics[0]
            .message
            .contains("missing/page.html.twig")
    );

    let actions = backend.handle_code_action(
        controller_uri.as_str(),
        controller_php,
        &CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: controller_uri.clone(),
            },
            range: template_diagnostics[0].range,
            context: CodeActionContext {
                diagnostics: template_diagnostics,
                only: Some(vec![CodeActionKind::QUICKFIX]),
                trigger_kind: Some(CodeActionTriggerKind::INVOKED),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    );
    let action = actions
        .iter()
        .find_map(|action| match action {
            CodeActionOrCommand::CodeAction(action)
                if action.title == "Create Twig template 'missing/page.html.twig'" =>
            {
                Some(action)
            }
            _ => None,
        })
        .expect("missing template should offer a create-file quick fix");
    let Some(DocumentChanges::Operations(operations)) = action
        .edit
        .as_ref()
        .and_then(|edit| edit.document_changes.as_ref())
    else {
        panic!("expected resource operations");
    };
    assert!(operations.iter().any(|operation| {
        matches!(
            operation,
            DocumentChangeOperation::Op(ResourceOp::Create(create))
                if create.uri.path().ends_with("/templates/missing/page.html.twig")
        )
    }));
}

#[tokio::test]
async fn symfony_bundle_override_templates_use_twig_namespaces() {
    let template = "<div>Widget</div>\n";
    let consumer = "{% include '@Acme/widget.html.twig' %}\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("templates/bundles/AcmeBundle/widget.html.twig", template),
            ("templates/consumer.html.twig", consumer),
        ],
    );
    let template_uri = uri_for(&dir, "templates/bundles/AcmeBundle/widget.html.twig");
    let consumer_uri = uri_for(&dir, "templates/consumer.html.twig");
    open_doc(&backend, template_uri.clone(), "twig", template).await;
    open_doc(&backend, consumer_uri.clone(), "twig", consumer).await;

    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: consumer_uri },
                position: position_in(consumer, "@Acme/widget.html.twig", 8),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("Twig bundle namespace should resolve");
    let location = match definition {
        GotoDefinitionResponse::Scalar(location) => location,
        GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
        GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
    };
    assert_eq!(location.uri, template_uri);
}

#[tokio::test]
async fn symfony_translations_complete_navigate_reference_and_show_lenses() {
    let messages_yaml = "navigation:\n  welcome: Welcome\n";
    let validators_xlf = r#"<?xml version="1.0"?>
<xliff version="1.2">
  <file>
    <body>
      <trans-unit id="hash" resname="app.invalid">
        <source>app.invalid</source>
        <target>Invalid</target>
      </trans-unit>
      <trans-unit id="source-hash">
        <source>source.only</source>
        <target>Source fallback</target>
      </trans-unit>
    </body>
  </file>
</xliff>
"#;
    let admin_php = "<?php\nreturn ['dashboard' => ['title' => 'Dashboard']];\n";
    let consumer_php = r#"<?php
use Symfony\Contracts\Translation\TranslatorInterface;
use Symfony\Component\Translation\TranslatableMessage;

function translate(TranslatorInterface $translator): void
{
    $translator->trans('navigation.welcome');
    $translator->trans('app.invalid', [], 'validators');
    $translator->trans('source.only', domain: 'validators');
    new TranslatableMessage('dashboard.title', [], 'admin');
}
"#;
    let template = r#"{% trans_default_domain 'validators' %}
{{ 'app.invalid'|trans }}
{{ 'navigation.welcome'|trans({}, 'messages') }}
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("translations/messages.en.yaml", messages_yaml),
            ("translations/validators.en.xlf", validators_xlf),
            ("translations/admin.en.php", admin_php),
            ("src/translate.php", consumer_php),
            ("templates/translated.html.twig", template),
        ],
    );
    let messages_uri = uri_for(&dir, "translations/messages.en.yaml");
    let validators_uri = uri_for(&dir, "translations/validators.en.xlf");
    let admin_uri = uri_for(&dir, "translations/admin.en.php");
    let consumer_uri = uri_for(&dir, "src/translate.php");
    let template_uri = uri_for(&dir, "templates/translated.html.twig");
    open_doc(&backend, messages_uri.clone(), "yaml", messages_yaml).await;
    open_doc(&backend, validators_uri.clone(), "xml", validators_xlf).await;
    open_doc(&backend, admin_uri.clone(), "php", admin_php).await;
    open_doc(&backend, consumer_uri.clone(), "php", consumer_php).await;
    open_doc(&backend, template_uri.clone(), "twig", template).await;

    for (uri, content, name, occurrence, expected_uri) in [
        (
            &consumer_uri,
            consumer_php,
            "navigation.welcome",
            0,
            &messages_uri,
        ),
        (
            &consumer_uri,
            consumer_php,
            "app.invalid",
            0,
            &validators_uri,
        ),
        (
            &consumer_uri,
            consumer_php,
            "dashboard.title",
            0,
            &admin_uri,
        ),
        (
            &consumer_uri,
            consumer_php,
            "source.only",
            0,
            &validators_uri,
        ),
        (
            &template_uri,
            template,
            "navigation.welcome",
            0,
            &messages_uri,
        ),
    ] {
        let offset = content
            .match_indices(name)
            .nth(occurrence)
            .expect("translation occurrence")
            .0;
        let position = position_in(content, &content[offset..offset + name.len()], 3);
        let definition = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("translation reference '{name}' should resolve"));
        let location = match definition {
            GotoDefinitionResponse::Scalar(location) => location,
            GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
            GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
        };
        assert_eq!(&location.uri, expected_uri, "wrong definition for {name}");
    }

    for (uri, content, name) in [
        (&consumer_uri, consumer_php, "navigation.welcome"),
        (&consumer_uri, consumer_php, "app.invalid"),
        (&consumer_uri, consumer_php, "dashboard.title"),
        (&consumer_uri, consumer_php, "source.only"),
        (&template_uri, template, "app.invalid"),
    ] {
        let response = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: position_in(content, name, 4),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: None,
            })
            .await
            .unwrap()
            .expect("translation completion should return candidates");
        let items = match response {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        assert!(
            items.iter().any(|item| item.label == name),
            "expected {name} completion, got {:?}",
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>()
        );
    }

    let references = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: consumer_uri.clone(),
                },
                position: position_in(consumer_php, "navigation.welcome", 4),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .expect("translation references should be returned");
    assert!(
        references
            .iter()
            .any(|location| location.uri == messages_uri)
    );
    assert!(
        references
            .iter()
            .any(|location| location.uri == template_uri)
    );

    let lenses = backend
        .handle_code_lens(messages_uri.as_str(), messages_yaml)
        .unwrap_or_default();
    assert!(
        lenses.iter().any(|lens| {
            lens.command
                .as_ref()
                .is_some_and(|command| command.title == "Symfony translation: 2 refs")
        }),
        "expected a translation reference lens, got {lenses:?}"
    );
}

#[tokio::test]
async fn symfony_translation_diagnostics_are_scoped_to_known_domains() {
    let messages_yaml = "known.message: Known\n";
    let consumer_php = r#"<?php
use Symfony\Contracts\Translation\TranslatorInterface;

function translate(TranslatorInterface $translator): void
{
    $translator->trans('missing.message');
    $translator->trans('dynamic.vendor.message', [], 'vendor');
}
"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("translations/messages.en.yaml", messages_yaml),
            ("src/translate.php", consumer_php),
        ],
    );
    let messages_uri = uri_for(&dir, "translations/messages.en.yaml");
    let consumer_uri = uri_for(&dir, "src/translate.php");
    open_doc(&backend, messages_uri, "yaml", messages_yaml).await;
    open_doc(&backend, consumer_uri.clone(), "php", consumer_php).await;

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(consumer_uri.as_str(), consumer_php, &mut diagnostics);
    let translations = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code == "unknown_symfony_translation"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        translations.len(),
        1,
        "only missing keys in known domains should be diagnosed"
    );
    assert!(translations[0].message.contains("missing.message"));
    assert!(translations[0].message.contains("'messages' domain"));
}
