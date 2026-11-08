//! Yii DI definitions: the members and arguments their keys name, and the
//! classes they bind, navigated from the class and from where the container
//! is asked for it.
use crate::common::message_text;

use crate::common::{
    completion_response, create_psr4_workspace, edits_for_uri, get_code_actions_at,
    markup_hover_at, offset_position, open_document, rename, response_labels,
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

const STUBS: &[(&str, &str)] = &[
    (
        "stubs/definitions/Contract/DefinitionInterface.php",
        "<?php\nnamespace Yiisoft\\Definitions\\Contract;\ninterface DefinitionInterface {}\n",
    ),
    (
        "stubs/definitions/Reference.php",
        "<?php\nnamespace Yiisoft\\Definitions;\nfinal class Reference implements Contract\\DefinitionInterface {\n    public static function to(mixed $id): self {}\n}\n",
    ),
    (
        "stubs/definitions/DynamicReference.php",
        "<?php\nnamespace Yiisoft\\Definitions;\nfinal class DynamicReference implements Contract\\DefinitionInterface {\n    public static function to(mixed $id): self {}\n}\n",
    ),
    (
        "stubs/definitions/ReferencesArray.php",
        "<?php\nnamespace Yiisoft\\Definitions;\nfinal class ReferencesArray {\n    /** @return Reference[] */\n    public static function from(array $ids): array {}\n}\n",
    ),
];

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

const MAILER_PHP: &str = r#"<?php
namespace App\Blog;

final class Mailer
{
    public int $retries = 3;
    public readonly string $from;
    protected string $secret = '';
    private int $sent = 0;

    public function __construct(private Logger $logger, string ...$headers) {}

    public function setTimeout(int $seconds): void {}

    /** @param list<Logger> $loggers */
    public function setLoggers(array $loggers): void {}

    protected function reset(): void {}
}
"#;

/// Definitions that name the members of the class they build wrongly.
const CHECKED_PHP: &str = r#"<?php
use App\Blog\InMemoryPostRepository;
use App\Blog\Logger;
use App\Blog\Mailer;
use Yiisoft\Definitions\DynamicReference;
use Yiisoft\Definitions\Reference;
use Yiisoft\Definitions\ReferencesArray;

return [
    'mailer' => [
        'class' => Mailer::class,
        '__construct()' => [Reference::to(Logger::class)],
        'setTimeout' => [5],
        'retries' => 1,
        "retries" => 2,
        'logger' => 1,
        'tags' => ['mail'],
        'reset' => static function (): void {},
        'setTimeout()-again' => [6],
        '$secret' => 'x',
        '$sent' => 1,
        'reset()' => [],
        '$from' => 'me',
        '$retries' => 'three',
        'setTimeout()' => ['soon'],
        'setLoggers()' => [ReferencesArray::from([Logger::class])],
    ],
    'mailer-by-name' => [
        'class' => Mailer::class,
        '__construct()' => [
            'logger' => Reference::to(InMemoryPostRepository::class),
            'headers' => ['a'],
        ],
    ],
    'mailer-fine' => [
        'class' => Mailer::class,
        '__construct()' => [
            'logger' => PHP_SAPI === 'cli' ? DynamicReference::to(Logger::class) : Reference::to(Logger::class),
        ],
        'setTimeout()' => ['5'],
        '$retries' => 5,
    ],
];
"#;

const NOTIFIER_PHP: &str = r#"<?php
namespace App\Blog;

abstract class Notifier
{
    public function __construct(protected Logger $logger) {}
}
"#;

const CLOCK_PHP: &str = r#"<?php
namespace App\Blog;

final class Clock {}
"#;

const SMS_NOTIFIER_PHP: &str = r#"<?php
namespace App\Blog;

final class SmsNotifier extends Notifier {}
"#;

/// Definitions passing arguments to a constructor that is declared, one
/// that is inherited, and one that is not there at all.
const CONSTRUCTORS_PHP: &str = r#"<?php
use App\Blog\Clock;
use App\Blog\Logger;
use App\Blog\Mailer;
use App\Blog\SmsNotifier;
use Yiisoft\Definitions\Reference;

return [
    Mailer::class => ['__construct()' => [Reference::to(Logger::class)]],
    SmsNotifier::class => ['__construct()' => [Reference::to(Logger::class)]],
    Clock::class => ['__construct()' => []],
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
        ("src/Blog/Mailer.php", MAILER_PHP),
        ("src/Blog/Notifier.php", NOTIFIER_PHP),
        ("src/Blog/SmsNotifier.php", SMS_NOTIFIER_PHP),
        ("src/Blog/Clock.php", CLOCK_PHP),
        ("config/di/blog.php", DI_PHP),
        ("config/di/constructors.php", CONSTRUCTORS_PHP),
        ("config/di/mailer.php", CHECKED_PHP),
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
async fn a_constructor_key_goes_to_the_constructor_when_there_is_one() {
    let (backend, dir) = workspace().await;
    let config = uri_for(&dir, "config/di/constructors.php");

    let declared = position_in(CONSTRUCTORS_PHP, "Mailer::class => ['__construct()'", 19);
    assert_eq!(
        definitions(&backend, &config, declared).await,
        vec![(
            uri_for(&dir, "src/Blog/Mailer.php"),
            position_in(MAILER_PHP, "__construct(", 0)
        )]
    );
    let hover = markup_hover_at(&backend, &config, declared.line, declared.character).await;
    assert!(hover.contains("__construct"), "{hover}");

    let inherited = position_in(
        CONSTRUCTORS_PHP,
        "SmsNotifier::class => ['__construct()'",
        24,
    );
    assert_eq!(
        definitions(&backend, &config, inherited).await,
        vec![(
            uri_for(&dir, "src/Blog/Notifier.php"),
            position_in(NOTIFIER_PHP, "__construct(", 0)
        )]
    );

    let missing = position_in(CONSTRUCTORS_PHP, "Clock::class => ['__construct()'", 18);
    assert!(definitions(&backend, &config, missing).await.is_empty());
    // A class without a constructor still takes the key.
    assert!(
        reported(
            &backend,
            &dir,
            "config/di/constructors.php",
            CONSTRUCTORS_PHP,
            "unknown_member"
        )
        .is_empty()
    );
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
        vec!["Method App\\Blog\\Card::__construct() has no parameter $titel".to_string()]
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
    assert!(
        unknown_members[1].contains("::$nope "),
        "{unknown_members:?}"
    );
    assert!(
        unknown_members[0].contains("::setLoger("),
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
            "Method App\\Blog\\InMemoryPostRepository::__construct() has no parameter $missing"
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
async fn keys_a_definition_does_not_take_are_reported() {
    let (backend, dir) = workspace().await;
    let report = |code| reported(&backend, &dir, "config/di/mailer.php", CHECKED_PHP, code);

    assert_eq!(
        report("invalid_yii_definition_key"),
        vec![
            "Unknown definition key 'setTimeout'; call the method as 'setTimeout()'".to_string(),
            "Unknown definition key 'retries'; set the property as '$retries'".to_string(),
            "Unknown definition key 'retries'; set the property as '$retries'".to_string(),
            "Unknown definition key 'logger'".to_string(),
        ]
    );
    assert_eq!(
        report("inaccessible_member"),
        vec![
            "Cannot set protected property App\\Blog\\Mailer::$secret from a definition"
                .to_string(),
            "Cannot set private property App\\Blog\\Mailer::$sent from a definition".to_string(),
            "Cannot call protected method App\\Blog\\Mailer::reset() from a definition".to_string(),
        ]
    );
    assert_eq!(
        report("invalid_readonly_write"),
        vec![
            "Cannot modify readonly property App\\Blog\\Mailer::$from from a definition"
                .to_string()
        ]
    );
}

#[tokio::test]
async fn values_a_member_does_not_accept_are_reported() {
    let (backend, dir) = workspace().await;
    let report = |code| reported(&backend, &dir, "config/di/mailer.php", CHECKED_PHP, code);

    // A method is called without `strict_types`, so `'5'` passes for an
    // `int`; a property is set and a constructor called with it.  A
    // reference is the object of the class it names, and one that cannot
    // be told apart from another is not judged.
    assert_eq!(
        report("type_mismatch_property"),
        vec!["Property App\\Blog\\Mailer::$retries expects int, got 'three'".to_string()]
    );
    assert_eq!(
        report("type_mismatch_argument"),
        vec![
            "Argument 1 ($seconds) expects int, got 'soon'".to_string(),
            "Argument 1 ($logger) expects App\\Blog\\Logger, got App\\Blog\\InMemoryPostRepository"
                .to_string(),
        ]
    );
}

#[tokio::test]
async fn a_key_naming_a_member_without_its_kind_is_fixed() {
    let (backend, dir) = workspace().await;
    let uri = uri_for(&dir, "config/di/mailer.php");

    let fixes = |needle: &str, inside: usize| {
        let at = position_in(CHECKED_PHP, needle, inside);
        let actions =
            get_code_actions_at(&backend, uri.as_str(), CHECKED_PHP, at.line, at.character);
        actions
            .iter()
            .filter_map(|action| match action {
                CodeActionResponse::CodeAction(action)
                    if action.title.starts_with("Change key") =>
                {
                    let edit = action.edit.as_ref()?;
                    let edits = edits_for_uri(edit, &uri);
                    Some((action.title.clone(), edits[0].new_text.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(
        fixes("'setTimeout' =>", 1),
        vec![(
            "Change key to 'setTimeout()'".to_string(),
            "setTimeout()".to_string()
        )]
    );
    assert_eq!(
        fixes("'retries' =>", 1),
        vec![(
            "Change key to '$retries'".to_string(),
            "$retries".to_string()
        )]
    );
    // `$` starts a variable in a double-quoted string.
    assert_eq!(
        fixes("\"retries\" =>", 1),
        vec![(
            "Change key to '$retries'".to_string(),
            "\\$retries".to_string()
        )]
    );
    assert!(fixes("'logger' =>", 1).is_empty());
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
