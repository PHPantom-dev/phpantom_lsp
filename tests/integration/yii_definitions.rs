//! Yii DI definitions: the members and arguments their keys name, and the
//! classes they bind, navigated from the class and from where the container
//! is asked for it.
use crate::common::message_text;

use crate::common::{
    completion_response, create_psr4_workspace, edits_for_uri, markup_hover_at, offset_position,
    open_document, rename, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "yiisoft/router": "^4.0", "yiisoft/di": "^1.4" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Yiisoft\\Definitions\\": "stubs/definitions/"
    } }
}"#;

const STUBS: &[(&str, &str)] = &[(
    "stubs/definitions/Reference.php",
    "<?php\nnamespace Yiisoft\\Definitions;\nfinal class Reference {\n    public static function to(mixed $id): self {}\n}\n",
)];

const INTERFACE_PHP: &str = r#"<?php
namespace App\Blog;

interface PostRepositoryInterface
{
    public function latest(): array;
}
"#;

const REPOSITORY_PHP: &str = r#"<?php
namespace App\Blog;

final class InMemoryPostRepository implements PostRepositoryInterface
{
    public int $pageSize = 10;
    public readonly string $name;
    private ?Logger $logger = null;

    public function __construct(private array $posts, int $limit = 0) {}

    public function setLogger(Logger $logger): void
    {
        $this->logger = $logger;
    }

    public function latest(): array
    {
        return $this->posts;
    }
}
"#;

const LOGGER_PHP: &str = r#"<?php
namespace App\Blog;

class Logger {}
"#;

const WIDGET_PHP: &str = r#"<?php
namespace App\Blog;

final class Card
{
    public function __construct(private string $title) {}

    public function withTitle(string $title): self
    {
        return $this;
    }

    public static function definition(): array
    {
        return ['class' => self::class, '__construct()' => ['titel' => 'x'], 'withTitle()' => ['y']];
    }
}
"#;

const DI_PHP: &str = r#"<?php
use App\Blog\InMemoryPostRepository;
use App\Blog\Logger;
use App\Blog\PostRepositoryInterface;
use Yiisoft\Definitions\Reference;

return [
    PostRepositoryInterface::class => [
        'class' => InMemoryPostRepository::class,
        '__construct()' => ['posts' => [], 'missing' => 1],
        'setLogger()' => [Reference::to(Logger::class)],
        '$pageSize' => 5,
        'setLoger()' => [],
        '$nope' => 1,
    ],
    Logger::class => Logger::class,
];
"#;

/// The `events` group is keyed by classes too, but names listeners.
const EVENTS_PHP: &str = r#"<?php
use App\Blog\Logger;

return [
    Logger::class => [[Logger::class, 'handle']],
];
"#;

/// A workspace holding the stubs and the blog's classes and configuration,
/// every file open.
async fn workspace() -> (Backend, tempfile::TempDir) {
    let files: &[(&str, &str)] = &[
        ("src/Blog/PostRepositoryInterface.php", INTERFACE_PHP),
        ("src/Blog/InMemoryPostRepository.php", REPOSITORY_PHP),
        ("src/Blog/Logger.php", LOGGER_PHP),
        ("src/Blog/Card.php", WIDGET_PHP),
        ("config/di/blog.php", DI_PHP),
        ("config/events.php", EVENTS_PHP),
    ];
    let all: Vec<(&str, &str)> = STUBS.iter().chain(files).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in files {
        open_document(&backend, &uri_for(&dir, path), "php", content).await;
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

fn text_document_position(uri: &Uri, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position,
    }
}

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, Position)> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: text_document_position(uri, position),
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
        .map(|location| (location.uri, location.range.start))
        .collect()
}

async fn implementations(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32)> {
    let response = backend
        .goto_implementation(ImplementationParams {
            text_document_position_params: text_document_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let locations = match response {
        None => Vec::new(),
        Some(ImplementationResponse::Definition(Definition::Location(location))) => vec![location],
        Some(ImplementationResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(ImplementationResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    };
    locations
        .into_iter()
        .map(|location| (location.uri, location.range.start.line))
        .collect()
}

async fn completion_labels(backend: &Backend, uri: &Uri, position: Position) -> Vec<String> {
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

/// The messages of the diagnostics with `code` the file at `rel` gets.
fn reported(
    backend: &Backend,
    dir: &tempfile::TempDir,
    rel: &str,
    content: &str,
    code: &str,
) -> Vec<String> {
    let uri = uri_for(dir, rel);
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    diagnostics
        .iter()
        .filter(|diagnostic| matches!(&diagnostic.code, Some(Code::String(found)) if found == code))
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect()
}

#[tokio::test]
async fn definition_keys_go_to_the_members_and_parameters_they_name() {
    let (backend, dir) = workspace().await;
    let di = uri_for(&dir, "config/di/blog.php");
    let repository = uri_for(&dir, "src/Blog/InMemoryPostRepository.php");

    assert_eq!(
        definitions(&backend, &di, position_in(DI_PHP, "'setLogger()'", 2)).await,
        vec![(
            repository.clone(),
            position_in(REPOSITORY_PHP, "setLogger(", 0)
        )]
    );
    assert_eq!(
        definitions(&backend, &di, position_in(DI_PHP, "'$pageSize'", 3)).await,
        vec![(
            repository.clone(),
            position_in(REPOSITORY_PHP, "$pageSize", 1)
        )]
    );
    assert_eq!(
        definitions(&backend, &di, position_in(DI_PHP, "'posts'", 2)).await,
        vec![(repository, position_in(REPOSITORY_PHP, "$posts", 0))]
    );

    let hover = markup_hover_at(
        &backend,
        &di,
        position_in(DI_PHP, "'posts'", 2).line,
        position_in(DI_PHP, "'posts'", 2).character,
    )
    .await;
    assert!(hover.contains("Parameter `array $posts`"), "{hover}");
}

#[tokio::test]
async fn a_definition_written_in_php_names_its_own_class() {
    let (backend, dir) = workspace().await;
    let card = uri_for(&dir, "src/Blog/Card.php");

    assert_eq!(
        definitions(&backend, &card, position_in(WIDGET_PHP, "'withTitle()'", 2)).await,
        vec![(card.clone(), position_in(WIDGET_PHP, "withTitle(", 0))]
    );
    assert_eq!(
        reported(
            &backend,
            &dir,
            "src/Blog/Card.php",
            WIDGET_PHP,
            "invalid_yii_definition_argument"
        ),
        vec!["Unknown argument: 'titel' is not a parameter of Card::__construct()".to_string()]
    );
}

#[tokio::test]
async fn renaming_a_member_renames_the_key_that_names_it() {
    let (backend, dir) = workspace().await;
    let di = uri_for(&dir, "config/di/blog.php");
    let repository = uri_for(&dir, "src/Blog/InMemoryPostRepository.php");

    let at = position_in(REPOSITORY_PHP, "setLogger(", 1);
    let edit = rename(&backend, &repository, at.line, at.character, "useLogger")
        .await
        .expect("rename should produce an edit");
    let edits = edits_for_uri(&edit, &di);
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(
        edits[0].range.start,
        position_in(DI_PHP, "'setLogger()'", 1)
    );
    assert_eq!(edits[0].new_text, "useLogger");

    let at = position_in(REPOSITORY_PHP, "$pageSize", 1);
    let edit = rename(&backend, &repository, at.line, at.character, "perPage")
        .await
        .expect("rename should produce an edit");
    let edits = edits_for_uri(&edit, &di);
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(edits[0].range.start, position_in(DI_PHP, "'$pageSize'", 2));
    assert_eq!(edits[0].new_text, "perPage");
}

#[tokio::test]
async fn names_the_class_does_not_have_are_reported() {
    let (backend, dir) = workspace().await;
    let mut unknown_members = reported(
        &backend,
        &dir,
        "config/di/blog.php",
        DI_PHP,
        "unknown_member",
    );
    unknown_members.sort();
    assert_eq!(unknown_members.len(), 2, "{unknown_members:?}");
    assert!(unknown_members[0].contains("'nope'"), "{unknown_members:?}");
    assert!(
        unknown_members[1].contains("'setLoger'"),
        "{unknown_members:?}"
    );
    assert_eq!(
        reported(
            &backend,
            &dir,
            "config/di/blog.php",
            DI_PHP,
            "invalid_yii_definition_argument"
        ),
        vec![
            "Unknown argument: 'missing' is not a parameter of InMemoryPostRepository::__construct()"
                .to_string()
        ]
    );
    // The events group names listeners, not definitions.
    assert!(
        reported(
            &backend,
            &dir,
            "config/events.php",
            EVENTS_PHP,
            "unknown_member"
        )
        .is_empty()
    );
}

#[tokio::test]
async fn definition_keys_and_arguments_complete() {
    let (backend, dir) = workspace().await;
    let di = uri_for(&dir, "config/di/blog.php");

    let typing = DI_PHP.replace("'$pageSize' => 5,", "'' => 5,");
    open_document(&backend, &di, "php", &typing).await;
    assert_eq!(
        completion_labels(&backend, &di, position_in(&typing, "'' => 5", 1)).await,
        vec![
            "__construct()".to_string(),
            "setLogger()".to_string(),
            "latest()".to_string(),
            "$pageSize".to_string(),
        ]
    );

    let typing = DI_PHP.replace("'$pageSize' => 5,", "'$' => 5,");
    open_document(&backend, &di, "php", &typing).await;
    assert_eq!(
        completion_labels(&backend, &di, position_in(&typing, "'$' => 5", 2)).await,
        vec!["$pageSize".to_string()]
    );

    let typing = DI_PHP.replace("['posts' => [], 'missing' => 1]", "['']");
    open_document(&backend, &di, "php", &typing).await;
    assert_eq!(
        completion_labels(&backend, &di, position_in(&typing, "['']", 2)).await,
        vec!["posts".to_string(), "limit".to_string()]
    );
}

#[tokio::test]
async fn a_bound_class_goes_to_its_definition() {
    let (backend, dir) = workspace().await;
    let di = uri_for(&dir, "config/di/blog.php");
    let interface = uri_for(&dir, "src/Blog/PostRepositoryInterface.php");
    let repository = uri_for(&dir, "src/Blog/InMemoryPostRepository.php");

    // From the interface: the definition that binds it, then its
    // implementations.
    assert_eq!(
        implementations(
            &backend,
            &interface,
            position_in(INTERFACE_PHP, "PostRepositoryInterface", 2)
        )
        .await,
        vec![
            (
                di.clone(),
                position_in(DI_PHP, "PostRepositoryInterface::class =>", 0).line
            ),
            (
                repository,
                position_in(REPOSITORY_PHP, "final class", 0).line
            ),
        ]
    );
    // From `Reference::to()`, where the container is asked for it.
    assert_eq!(
        implementations(
            &backend,
            &di,
            position_in(DI_PHP, "Reference::to(Logger", 15)
        )
        .await,
        vec![(di.clone(), position_in(DI_PHP, "Logger::class =>", 0).line)]
    );

    let lenses = backend
        .handle_code_lens(interface.as_str(), INTERFACE_PHP)
        .unwrap_or_default();
    let titles: Vec<String> = lenses
        .iter()
        .filter_map(|lens| lens.command.as_ref().map(|command| command.title.clone()))
        .collect();
    assert!(
        titles.contains(&"DI definition: config/di/blog.php".to_string()),
        "{titles:?}"
    );
}
