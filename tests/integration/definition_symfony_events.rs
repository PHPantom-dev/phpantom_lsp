use crate::common::create_psr4_workspace;
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const CONFIG: &str = r#"
[indexing]
strategy = "none"

[symfony.container]
environment = "dev"

[symfony.events]
ignored-prefixes = ["use_case."]
ignored-suffixes = [".async"]

[[symfony.events.publishers]]
attribute = 'Acme\Event\Publish'
name-argument = "name"
name-position = 2
dispatch-argument = "dispatch"
dispatch-position = 4
default-dispatch = ["post"]
dispatch-cases = { PRE = "pre", POST = "post" }
name-template = "{dispatch}.{class_snake}{method_suffix_snake}"
explicit-name-template = "{name}"
default-methods = ["execute", "__invoke"]

[[symfony.events.subscribers]]
attribute = 'Acme\Event\Listen'
name-argument = "name"
name-position = 0
"#;

const PUBLISHER: &str = r#"<?php
namespace App\UseCase;

use Acme\Event\Publish;

final class PublishCourse
{
    #[Publish]
    public function execute(): void {}
}
"#;

const COMPILED_LISTENER: &str = r#"<?php
namespace App\Listener;

class CourseListener
{
    public function onPublished(): void {}
}
"#;

const CONFIGURED_LISTENER: &str = r#"<?php
namespace App\Listener;

use Acme\Event\Listen;

final class AuditListener
{
    #[Listen('post.publish_course')]
    public function audit(): void {}
}
"#;

const CONTAINER: &str = r#"<?php
$dispatcher->addListener(
    'use_case.post.publish_course.async',
    [#[\Closure(name: 'App\\Listener\\CourseListener')] fn () => ($container->privates['App\\Listener\\CourseListener'] ?? null), 'onPublished'],
    0,
);
$factory->createProxy(new \App\UseCase\PublishCourse());
"#;

async fn open_php(backend: &Backend, uri: Url, content: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;
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

fn lens<'a>(lenses: &'a [CodeLens], title: &str) -> &'a CodeLens {
    lenses
        .iter()
        .find(|lens| {
            lens.command
                .as_ref()
                .is_some_and(|command| command.title == title)
        })
        .unwrap_or_else(|| panic!("missing {title:?} in {lenses:#?}"))
}

async fn definition(backend: &Backend, uri: Url, content: &str, needle: &str) -> Vec<Location> {
    let response = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: position_in(content, needle, 2),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("event should navigate");
    match response {
        GotoDefinitionResponse::Scalar(location) => vec![location],
        GotoDefinitionResponse::Array(locations) => locations,
        GotoDefinitionResponse::Link(_) => panic!("unexpected location links"),
    }
}

#[tokio::test]
async fn compiled_container_and_configured_attributes_drive_symfony_event_navigation() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            (".phpantom.toml", CONFIG),
            ("src/UseCase/PublishCourse.php", PUBLISHER),
            ("src/Listener/CourseListener.php", COMPILED_LISTENER),
            ("src/Listener/AuditListener.php", CONFIGURED_LISTENER),
            (
                "var/cache/dev/ContainerAbc/KernelDevDebugContainer.php",
                CONTAINER,
            ),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let publisher_uri =
        Url::from_file_path(dir.path().join("src/UseCase/PublishCourse.php")).unwrap();
    let compiled_uri =
        Url::from_file_path(dir.path().join("src/Listener/CourseListener.php")).unwrap();
    let configured_uri =
        Url::from_file_path(dir.path().join("src/Listener/AuditListener.php")).unwrap();
    open_php(&backend, publisher_uri.clone(), PUBLISHER).await;
    open_php(&backend, compiled_uri.clone(), COMPILED_LISTENER).await;
    open_php(&backend, configured_uri.clone(), CONFIGURED_LISTENER).await;

    let publisher_lenses = backend
        .handle_code_lens(publisher_uri.as_str(), PUBLISHER)
        .unwrap_or_default();
    let publisher_lens = lens(&publisher_lenses, "Symfony event: 2 subscribers");
    let locations: Vec<Location> = serde_json::from_value(
        publisher_lens
            .command
            .as_ref()
            .unwrap()
            .arguments
            .as_ref()
            .unwrap()[2]
            .clone(),
    )
    .unwrap();
    assert_eq!(locations.len(), 2);

    let compiled_lenses = backend
        .handle_code_lens(compiled_uri.as_str(), COMPILED_LISTENER)
        .unwrap_or_default();
    lens(&compiled_lenses, "Symfony event: 1 publisher");
    let configured_lenses = backend
        .handle_code_lens(configured_uri.as_str(), CONFIGURED_LISTENER)
        .unwrap_or_default();
    lens(&configured_lenses, "Symfony event: 1 publisher");

    let publisher_targets = definition(&backend, publisher_uri.clone(), PUBLISHER, "execute").await;
    assert_eq!(publisher_targets.len(), 2);
    assert!(
        publisher_targets
            .iter()
            .any(|location| location.uri == compiled_uri)
    );
    assert!(
        publisher_targets
            .iter()
            .any(|location| location.uri == configured_uri)
    );

    let subscriber_targets = definition(
        &backend,
        compiled_uri.clone(),
        COMPILED_LISTENER,
        "onPublished",
    )
    .await;
    assert_eq!(subscriber_targets.len(), 1);
    assert_eq!(subscriber_targets[0].uri, publisher_uri);

    let references = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: compiled_uri },
                position: position_in(COMPILED_LISTENER, "onPublished", 2),
            },
            context: ReferenceContext {
                include_declaration: true,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .expect("event references should resolve");
    assert_eq!(references.len(), 3);
}
