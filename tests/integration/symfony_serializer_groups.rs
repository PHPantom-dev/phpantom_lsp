//! Symfony serializer groups: declared by `#[Groups]` and the `groups` of a
//! serializer mapping, read by the `groups` of a serialization context.

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
        "Symfony\\Component\\Serializer\\": "stubs/serializer/",
        "Symfony\\Bundle\\FrameworkBundle\\": "stubs/framework-bundle/"
    } }
}"#;

const STUBS: &[(&str, &str)] = &[
    (
        "stubs/serializer/SerializerInterface.php",
        "<?php\nnamespace Symfony\\Component\\Serializer;\ninterface SerializerInterface {\n    public function serialize(mixed $data, string $format, array $context = []): string;\n}\n",
    ),
    (
        "stubs/serializer/Attribute/Groups.php",
        "<?php\nnamespace Symfony\\Component\\Serializer\\Attribute;\n#[\\Attribute]\nclass Groups {\n    public function __construct(string|array $groups) {}\n}\n",
    ),
    (
        "stubs/framework-bundle/Controller/AbstractController.php",
        "<?php\nnamespace Symfony\\Bundle\\FrameworkBundle\\Controller;\nabstract class AbstractController {\n    protected function json(mixed $data, int $status = 200, array $headers = [], array $context = []): object { return new \\stdClass(); }\n}\n",
    ),
];

const USER_PHP: &str = r#"<?php
namespace App\Entity;

use Symfony\Component\Serializer\Attribute\Groups;

final class User
{
    #[Groups(['user:read', 'user:write'])]
    public string $email = '';

    #[Groups('user:read')]
    public string $name = '';
}
"#;

const CONTROLLER_PHP: &str = r#"<?php
namespace App\Controller;

use App\Entity\User;
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\Serializer\SerializerInterface;

final class UserController extends AbstractController
{
    public function show(SerializerInterface $serializer, User $user): object
    {
        $serializer->serialize($user, 'json', ['groups' => ['user:read']]);
        $other->serialize($user, 'json', ['groups' => 'user:write']);

        return $this->json($user, context: ['groups' => '']);
    }
}
"#;

const MAPPING_YAML: &str =
    "App\\Entity\\User:\n    attributes:\n        email:\n            groups: ['admin']\n";

async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = STUBS.iter().chain(files).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
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

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("src/Entity/User.php", USER_PHP),
        ("src/Controller/UserController.php", CONTROLLER_PHP),
        ("config/serializer/User.yaml", MAPPING_YAML),
    ]
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
    let mut locations: Vec<(Uri, u32)> = match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    }
    .into_iter()
    .map(|location| (location.uri, location.range.start.line))
    .collect();
    locations.sort();
    locations
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

#[tokio::test]
async fn a_context_group_goes_to_the_members_in_it() {
    let (backend, dir) = workspace(&files()).await;
    let user = uri_for(&dir, "src/Entity/User.php");
    let controller = uri_for(&dir, "src/Controller/UserController.php");
    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "'user:read'", 2),
    )
    .await;
    assert_eq!(found, vec![(user.clone(), 7), (user, 10)]);

    // A receiver the type engine cannot confirm names no group.
    let found = definitions(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "'user:write'", 2),
    )
    .await;
    assert_eq!(found, Vec::new());
}

#[tokio::test]
async fn group_references_reach_attributes_mappings_and_contexts() {
    let (backend, dir) = workspace(&files()).await;
    let user = uri_for(&dir, "src/Entity/User.php");
    let controller = uri_for(&dir, "src/Controller/UserController.php");
    let found = references(&backend, &user, position_in(USER_PHP, "'user:read'", 2)).await;
    assert_eq!(
        found,
        vec![(controller, 11), (user.clone(), 7), (user.clone(), 10)]
    );

    let yaml = uri_for(&dir, "config/serializer/User.yaml");
    let found = references(&backend, &yaml, position_in(MAPPING_YAML, "admin", 1)).await;
    assert_eq!(found, vec![(yaml, 3)]);
}

#[tokio::test]
async fn groups_complete_in_a_controller_context() {
    let (backend, dir) = workspace(&files()).await;
    let controller = uri_for(&dir, "src/Controller/UserController.php");
    let position = position_in(CONTROLLER_PHP, "'groups' => ''", 13);
    let labels = response_labels(
        completion_response(
            &backend,
            &controller,
            position.line,
            position.character,
            None,
        )
        .await,
    );
    assert_eq!(labels, vec!["admin", "user:read", "user:write"]);
}
