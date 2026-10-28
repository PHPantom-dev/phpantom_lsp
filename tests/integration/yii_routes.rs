//! Yii route names, from the `Route` definitions of the router's
//! configuration (with the prefixes of their groups) to the URL generator
//! calls that read them, and the route arguments a handler reads.
use crate::common::message_text;

use crate::common::{
    completion_response, create_psr4_workspace, markup_hover_at, offset_position, open_document,
    response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "yiisoft/router": "^4.0" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Yiisoft\\Router\\": "stubs/router/"
    } }
}"#;

/// Just enough of the router for the type engine to tell a URL generator
/// from any other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/router/Route.php",
        "<?php\nnamespace Yiisoft\\Router;\nfinal class Route {\n    public static function get(string $pattern): self {}\n    public static function methods(array $methods, string $pattern): self {}\n    public function name(string $name): self {}\n    public function action(array|callable|string $middlewareDefinition): self {}\n}\n",
    ),
    (
        "stubs/router/Group.php",
        "<?php\nnamespace Yiisoft\\Router;\nfinal class Group {\n    public static function create(?string $prefix = null): self {}\n    public function routes(self|Route ...$routes): self {}\n    public function namePrefix(string $namePrefix): self {}\n}\n",
    ),
    (
        "stubs/router/UrlGeneratorInterface.php",
        "<?php\nnamespace Yiisoft\\Router;\ninterface UrlGeneratorInterface {\n    public function generate(string $name, array $arguments = [], array $queryParameters = [], ?string $hash = null): string;\n    public function generateAbsolute(string $name, array $arguments = []): string;\n}\n",
    ),
    (
        "stubs/router/HydratorAttribute/RouteArgument.php",
        "<?php\nnamespace Yiisoft\\Router\\HydratorAttribute;\n#[\\Attribute]\nfinal class RouteArgument {\n    public function __construct(private readonly ?string $name = null) {}\n}\n",
    ),
];

const ROUTES_PHP: &str = r#"<?php
use App\Web\HomeAction;
use App\Web\PostController;
use Yiisoft\Router\Group;
use Yiisoft\Router\Route;

return [
    Route::get('/')->action(HomeAction::class)->name('home'),
    Group::create('/blog')
        ->namePrefix('blog/')
        ->routes(
            Route::get('[/{page}]')
                ->action([PostController::class, 'index'])
                ->name('index'),
            Route::get('/{slug}')
                ->action([PostController::class, 'view'])
                ->name('post'),
        ),
];
"#;

const CONTROLLER_PHP: &str = r#"<?php
namespace App\Web;

use Yiisoft\Router\HydratorAttribute\RouteArgument;
use Yiisoft\Router\UrlGeneratorInterface;

final class PostController
{
    public function __construct(private UrlGeneratorInterface $urls, private \ArrayObject $other) {}

    public function index(#[RouteArgument('page')] int $page = 1): string
    {
        $this->other->offsetGet('blog/post');
        return $this->urls->generate('blog/post', ['slug' => 'hello']);
    }

    public function view(#[RouteArgument('slug')] string $slug, #[RouteArgument('id')] int $id): string
    {
        return $this->urls->generateAbsolute('blog/missing');
    }
}
"#;

const HOME_PHP: &str = r#"<?php
namespace App\Web;

final class HomeAction
{
    public function __invoke(): void {}
}
"#;

/// A class of the project's own in front of the URL generator, the way
/// `yiisoft/demo-blog` builds its URLs.
const WRAPPER_PHP: &str = r#"<?php
namespace App\Shared;

use Yiisoft\Router\UrlGeneratorInterface;

final class UrlGenerator
{
    public function __construct(private UrlGeneratorInterface $generator) {}

    public function home(): string
    {
        return $this->generate('home');
    }

    public function post(string $slug): string
    {
        return $this->generate('blog/post', ['slug' => $slug]);
    }

    public function generate(string $name, array $arguments = []): string
    {
        return $this->generator->generate($name, $arguments);
    }
}
"#;

/// A workspace holding the stubs and `files`, every one of them open.
async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
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

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, Position)> {
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
        .map(|location| (location.uri, location.range.start))
        .collect()
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

async fn completion_labels(backend: &Backend, uri: &Uri, position: Position) -> Vec<String> {
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

async fn hover_text(backend: &Backend, uri: &Uri, position: Position) -> String {
    markup_hover_at(backend, uri, position.line, position.character).await
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
async fn a_generated_url_goes_to_the_route_under_its_group_prefix() {
    let (backend, dir) = workspace(&[
        ("config/routes.php", ROUTES_PHP),
        ("src/Web/PostController.php", CONTROLLER_PHP),
    ])
    .await;
    let controller = uri_for(&dir, "src/Web/PostController.php");
    let routes = uri_for(&dir, "config/routes.php");

    assert_eq!(
        definitions(
            &backend,
            &controller,
            position_in(CONTROLLER_PHP, "generate('blog/post'", 11)
        )
        .await,
        vec![(routes.clone(), position_in(ROUTES_PHP, "'post'", 1))]
    );
    // An `ArrayObject` is no URL generator.
    assert!(
        definitions(
            &backend,
            &controller,
            position_in(CONTROLLER_PHP, "offsetGet('blog/post'", 12)
        )
        .await
        .is_empty()
    );

    // From the route, every place that generates it.
    assert_eq!(
        references(&backend, &routes, position_in(ROUTES_PHP, "'post'", 2)).await,
        vec![
            (routes.clone(), position_in(ROUTES_PHP, "'post'", 1).line),
            (
                controller.clone(),
                position_in(CONTROLLER_PHP, "generate('blog/post'", 0).line
            ),
        ]
    );
}

#[tokio::test]
async fn route_names_and_their_arguments_complete() {
    let (backend, dir) = workspace(&[
        ("config/routes.php", ROUTES_PHP),
        ("src/Web/PostController.php", CONTROLLER_PHP),
    ])
    .await;
    let controller = uri_for(&dir, "src/Web/PostController.php");

    let typing = CONTROLLER_PHP.replace("generate('blog/post'", "generate('blog/'");
    open_document(&backend, &controller, "php", &typing).await;
    assert_eq!(
        completion_labels(
            &backend,
            &controller,
            position_in(&typing, "generate('blog/", 15)
        )
        .await,
        vec!["blog/index".to_string(), "blog/post".to_string()]
    );

    let typing = CONTROLLER_PHP.replace("['slug' => 'hello']", "['']");
    open_document(&backend, &controller, "php", &typing).await;
    assert_eq!(
        completion_labels(&backend, &controller, position_in(&typing, "['", 2)).await,
        vec!["slug".to_string()]
    );

    let typing = CONTROLLER_PHP.replace("RouteArgument('page')", "RouteArgument('')");
    open_document(&backend, &controller, "php", &typing).await;
    assert_eq!(
        completion_labels(
            &backend,
            &controller,
            position_in(&typing, "RouteArgument('", 15)
        )
        .await,
        vec!["page".to_string()]
    );
}

#[tokio::test]
async fn unknown_route_names_and_arguments_are_reported() {
    let (backend, dir) = workspace(&[
        ("config/routes.php", ROUTES_PHP),
        ("src/Web/PostController.php", CONTROLLER_PHP),
    ])
    .await;
    assert_eq!(
        reported(
            &backend,
            &dir,
            "src/Web/PostController.php",
            CONTROLLER_PHP,
            "invalid_yii_route"
        ),
        vec!["Unknown route: 'blog/missing'".to_string()]
    );
    assert_eq!(
        reported(
            &backend,
            &dir,
            "src/Web/PostController.php",
            CONTROLLER_PHP,
            "invalid_yii_route_argument"
        ),
        vec!["Unknown route argument: 'id'".to_string()]
    );
}

#[tokio::test]
async fn a_route_argument_goes_to_the_placeholder_of_its_route() {
    let (backend, dir) = workspace(&[
        ("config/routes.php", ROUTES_PHP),
        ("src/Web/PostController.php", CONTROLLER_PHP),
    ])
    .await;
    let controller = uri_for(&dir, "src/Web/PostController.php");
    let routes = uri_for(&dir, "config/routes.php");

    // The placeholder sits inside an optional segment.
    assert_eq!(
        definitions(
            &backend,
            &controller,
            position_in(CONTROLLER_PHP, "RouteArgument('page')", 15)
        )
        .await,
        vec![(routes.clone(), position_in(ROUTES_PHP, "{page}", 1))]
    );
    assert_eq!(
        definitions(
            &backend,
            &controller,
            position_in(CONTROLLER_PHP, "RouteArgument('slug')", 15)
        )
        .await,
        vec![(routes, position_in(ROUTES_PHP, "{slug}", 1))]
    );

    let hover = hover_text(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "RouteArgument('slug')", 15),
    )
    .await;
    assert!(hover.contains("`slug`"), "{hover}");
    assert!(hover.contains("Path `/blog/{slug}`"), "{hover}");
}

#[tokio::test]
async fn a_wrapper_around_the_url_generator_names_routes_too() {
    let (backend, dir) = workspace(&[
        ("config/routes.php", ROUTES_PHP),
        ("src/Shared/UrlGenerator.php", WRAPPER_PHP),
        ("src/Web/HomeAction.php", HOME_PHP),
    ])
    .await;
    let wrapper = uri_for(&dir, "src/Shared/UrlGenerator.php");
    let routes = uri_for(&dir, "config/routes.php");

    assert_eq!(
        definitions(
            &backend,
            &wrapper,
            position_in(WRAPPER_PHP, "generate('home'", 11)
        )
        .await,
        vec![(routes.clone(), position_in(ROUTES_PHP, "'home'", 1))]
    );
    assert_eq!(
        references(&backend, &routes, position_in(ROUTES_PHP, "'home'", 2)).await,
        vec![
            (routes, position_in(ROUTES_PHP, "'home'", 1).line),
            (
                wrapper.clone(),
                position_in(WRAPPER_PHP, "generate('home'", 0).line
            ),
        ]
    );

    let typing = WRAPPER_PHP.replace("['slug' => $slug]", "['']");
    open_document(&backend, &wrapper, "php", &typing).await;
    assert_eq!(
        completion_labels(&backend, &wrapper, position_in(&typing, "['", 2)).await,
        vec!["slug".to_string()]
    );
}

#[tokio::test]
async fn a_route_name_hover_shows_its_path() {
    let (backend, dir) = workspace(&[
        ("config/routes.php", ROUTES_PHP),
        ("src/Web/PostController.php", CONTROLLER_PHP),
    ])
    .await;
    let controller = uri_for(&dir, "src/Web/PostController.php");
    let hover = hover_text(
        &backend,
        &controller,
        position_in(CONTROLLER_PHP, "generate('blog/post'", 11),
    )
    .await;
    assert!(hover.contains("**Route** `blog/post`"), "{hover}");
    assert!(hover.contains("Declared in `config/routes.php`"), "{hover}");
    assert!(hover.contains("Path `/blog/{slug}`"), "{hover}");
}
