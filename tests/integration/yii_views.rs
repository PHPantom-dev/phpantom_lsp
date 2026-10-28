//! Yii view names: a name a renderer resolves against the view path its
//! class gives it, a path built from `__DIR__`, and a view rendered from
//! inside a template, each navigating to its template, completing from the
//! template directory, and reported when no template is there.
use crate::common::{
    completion_response, hover_response, hover_text, initialize_with_options, message_text,
    offset_position, open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/view": "^12.0",
        "yiisoft/yii-view-renderer": "^7.0"
    },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Yiisoft\\Yii\\View\\Renderer\\": "stubs/renderer/",
        "Yiisoft\\View\\": "stubs/view/"
    } }
}"#;

const RENDERER_PHP: &str = r#"<?php
namespace Yiisoft\Yii\View\Renderer;

final class WebViewRenderer
{
    public function withViewPath(string $viewPath): self { return $this; }
    public function withController(object $controller): self { return $this; }
    public function render(string $view, array $parameters = []): object { return $this; }
    public function renderPartial(string $view, array $parameters = []): object { return $this; }
}
"#;

const VIEW_INTERFACE_PHP: &str = r#"<?php
namespace Yiisoft\View;

interface ViewInterface
{
    public function render(string $view, array $parameters = []): string;
}
"#;

const WEB_VIEW_PHP: &str = r#"<?php
namespace Yiisoft\View;

final class WebView implements ViewInterface
{
    public function render(string $view, array $parameters = []): string { return ''; }
}
"#;

const ALIASES_PHP: &str = r#"<?php
return [
    '@root' => dirname(__DIR__, 2),
    '@views' => '@root/resources/views',
];
"#;

const PARAMS_PHP: &str = r#"<?php
return [
    'yiisoft/aliases' => [
        'aliases' => require __DIR__ . '/aliases.php',
    ],
];
"#;

const POST_CONTROLLER_PHP: &str = r#"<?php
namespace App\Web\Blog;

use Yiisoft\Yii\View\Renderer\WebViewRenderer;

final class PostController
{
    private WebViewRenderer $viewRenderer;

    public function __construct(WebViewRenderer $viewRenderer, private \ArrayObject $other)
    {
        $this->viewRenderer = $viewRenderer->withViewPath('@views/blog');
    }

    public function index(): object
    {
        $this->other->offsetGet('index');
        return $this->viewRenderer->render('index');
    }

    public function missing(): object
    {
        return $this->viewRenderer->renderPartial('missing');
    }
}
"#;

const SHOP_CONTROLLER_PHP: &str = r#"<?php
namespace App\Web\Shop;

use Yiisoft\Yii\View\Renderer\WebViewRenderer;

final class ShopController
{
    public function __construct(private WebViewRenderer $viewRenderer) {}

    public function list(): object
    {
        return $this->viewRenderer->withViewPath('@views')->withController($this)->render('list');
    }
}
"#;

const ACTION_PHP: &str = r#"<?php
namespace App\Web\HomePage;

use Yiisoft\Yii\View\Renderer\WebViewRenderer;

final class Action
{
    public function __construct(private WebViewRenderer $viewRenderer) {}

    public function __invoke(): object
    {
        $this->viewRenderer->render(__DIR__ . '/nope');
        return $this->viewRenderer->render(__DIR__ . '/template');
    }
}
"#;

const INDEX_PHP: &str = r#"<?php
/** @var \Yiisoft\View\WebView $this */
echo $this->render('_card');
echo $this->render('./_card');
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("stubs/renderer/WebViewRenderer.php", RENDERER_PHP),
        ("stubs/view/ViewInterface.php", VIEW_INTERFACE_PHP),
        ("stubs/view/WebView.php", WEB_VIEW_PHP),
        ("config/common/aliases.php", ALIASES_PHP),
        ("config/common/params.php", PARAMS_PHP),
        ("src/Web/Blog/PostController.php", POST_CONTROLLER_PHP),
        ("src/Web/Shop/ShopController.php", SHOP_CONTROLLER_PHP),
        ("src/Web/HomePage/Action.php", ACTION_PHP),
        ("src/Web/HomePage/template.php", "<h1>Hello</h1>\n"),
        ("resources/views/blog/index.php", INDEX_PHP),
        ("resources/views/blog/_card.php", "<article></article>\n"),
        ("resources/views/blog/admin/edit.php", "<form></form>\n"),
        ("resources/views/shop/list.php", "<ul></ul>\n"),
    ]
}

/// A session over the workspace with `open` open.
async fn workspace(open: &[&str]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files() {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    for path in open {
        open_document(&backend, &uri_for(&dir, path), "php", content_of(path)).await;
    }
    (backend, dir)
}

fn content_of(path: &str) -> &'static str {
    files()
        .into_iter()
        .find(|(file, _)| *file == path)
        .unwrap()
        .1
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

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<Uri> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: text_document_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location.uri],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => {
            locations.into_iter().map(|location| location.uri).collect()
        }
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    }
}

#[tokio::test]
async fn each_way_of_naming_a_view_resolves_to_its_template() {
    let (backend, dir) = workspace(&[
        "src/Web/Blog/PostController.php",
        "src/Web/Shop/ShopController.php",
        "src/Web/HomePage/Action.php",
        "resources/views/blog/index.php",
    ])
    .await;
    for (file, needle, expected) in [
        (
            "src/Web/Blog/PostController.php",
            "render('index",
            "resources/views/blog/index.php",
        ),
        (
            "src/Web/Shop/ShopController.php",
            "render('list",
            "resources/views/shop/list.php",
        ),
        (
            "src/Web/HomePage/Action.php",
            "'/template",
            "src/Web/HomePage/template.php",
        ),
        (
            "resources/views/blog/index.php",
            "render('_card",
            "resources/views/blog/_card.php",
        ),
        (
            "resources/views/blog/index.php",
            "render('./_card",
            "resources/views/blog/_card.php",
        ),
    ] {
        let inside = needle.find('\'').unwrap() + 2;
        let found = definitions(
            &backend,
            &uri_for(&dir, file),
            position_in(content_of(file), needle, inside),
        )
        .await;
        assert_eq!(
            found,
            vec![uri_for(&dir, expected)],
            "for {needle} in {file}"
        );
    }

    // `offsetGet()` on something that renders nothing names nothing.
    let found = definitions(
        &backend,
        &uri_for(&dir, "src/Web/Blog/PostController.php"),
        position_in(POST_CONTROLLER_PHP, "offsetGet('index", 12),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn a_template_lists_the_calls_that_render_it() {
    let (backend, dir) = workspace(&["resources/views/blog/index.php"]).await;
    let card = uri_for(&dir, "resources/views/blog/_card.php");
    let index = uri_for(&dir, "resources/views/blog/index.php");

    let references = backend
        .references(ReferenceParams {
            text_document_position_params: text_document_position(
                &index,
                position_in(INDEX_PHP, "render('_card", 9),
            ),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default();
    let mut found: Vec<(String, u32)> = references
        .iter()
        .map(|location| (location.uri.to_string(), location.range.start.line))
        .collect();
    found.sort();
    assert_eq!(
        found,
        vec![
            (card.to_string(), 0),
            (index.to_string(), 2),
            (index.to_string(), 3)
        ]
    );

    let lenses = backend
        .handle_code_lens(card.as_str(), "<article></article>\n")
        .unwrap_or_default();
    let titles: Vec<String> = lenses
        .iter()
        .filter_map(|lens| lens.command.as_ref().map(|command| command.title.clone()))
        .collect();
    assert_eq!(titles, vec!["2 renders".to_string()]);
}

#[tokio::test]
async fn a_view_no_template_matches_is_reported() {
    let (backend, dir) = workspace(&[
        "src/Web/Blog/PostController.php",
        "src/Web/HomePage/Action.php",
    ])
    .await;
    let mut reported = Vec::new();
    for file in [
        "src/Web/Blog/PostController.php",
        "src/Web/HomePage/Action.php",
    ] {
        let mut diagnostics = Vec::new();
        backend.collect_slow_diagnostics(
            uri_for(&dir, file).as_str(),
            content_of(file),
            &mut diagnostics,
        );
        reported.extend(
            diagnostics
                .iter()
                .filter(|diagnostic| {
                    matches!(
                        &diagnostic.code,
                        Some(Code::String(code)) if code == "invalid_yii_view"
                    )
                })
                .map(|diagnostic| message_text(&diagnostic.message).to_string()),
        );
    }
    assert_eq!(
        reported,
        vec![
            "Unknown view: 'resources/views/blog/missing'".to_string(),
            "Unknown view: 'src/Web/HomePage/nope'".to_string(),
        ]
    );
}

#[tokio::test]
async fn views_complete_from_the_template_directory() {
    let (backend, dir) = workspace(&[]).await;
    for (file, content, needle, expected) in [
        (
            "src/Web/Blog/Draft.php",
            "<?php\nnamespace App\\Web\\Blog;\nfinal class Draft {\n    public function __construct(private \\Yiisoft\\Yii\\View\\Renderer\\WebViewRenderer $r) { $this->r = $r->withViewPath('@views/blog'); }\n    public function a(): object { return $this->r->render(''); }\n}\n",
            "render('",
            vec!["_card", "admin/", "index"],
        ),
        (
            "src/Web/Blog/Admin.php",
            "<?php\nnamespace App\\Web\\Blog;\nfinal class Admin {\n    public function __construct(private \\Yiisoft\\Yii\\View\\Renderer\\WebViewRenderer $r) { $this->r = $r->withViewPath('@views/blog'); }\n    public function a(): object { return $this->r->render('admin/'); }\n}\n",
            "render('admin/",
            vec!["admin/edit"],
        ),
        (
            "src/Web/HomePage/Other.php",
            "<?php\nnamespace App\\Web\\HomePage;\nfinal class Other {\n    public function a(object $r): object { return $r->render(__DIR__ . '/'); }\n}\n",
            "'/",
            vec!["/Action", "/template"],
        ),
    ] {
        let uri = uri_for(&dir, file);
        open_document(&backend, &uri, "php", content).await;
        let position = position_in(content, needle, needle.len());
        let labels = response_labels(
            completion_response(&backend, &uri, position.line, position.character, None).await,
        );
        let expected: Vec<String> = expected.into_iter().map(str::to_string).collect();
        assert_eq!(labels, expected, "for {needle} in {file}");
    }
}

#[tokio::test]
async fn hovering_a_view_shows_its_template() {
    let (backend, dir) = workspace(&["src/Web/Blog/PostController.php"]).await;
    let uri = uri_for(&dir, "src/Web/Blog/PostController.php");
    let position = position_in(POST_CONTROLLER_PHP, "render('index", 9);
    let hover = hover_response(&backend, &uri, position.line, position.character)
        .await
        .expect("a hover");
    let markup = hover_text(&hover);
    assert!(
        markup.contains("**View** `resources/views/blog/index`"),
        "{markup}"
    );
    assert!(
        markup.contains("`resources/views/blog/index.php`"),
        "{markup}"
    );
}
