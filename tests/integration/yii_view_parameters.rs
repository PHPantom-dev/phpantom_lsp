//! Yii view parameters: the keys a render call passes complete from the
//! `@var` block the template opens with, navigate to it, show its type on
//! hover, and are checked against it, as is what the call leaves out.
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
    public function render(string $view, array $parameters = []): object { return $this; }
}
"#;

const INJECTION_INTERFACE_PHP: &str = r#"<?php
namespace Yiisoft\Yii\View\Renderer;

interface CommonParametersInjectionInterface
{
    public function getCommonParameters(): array;
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
    public function setParameters(array $parameters): static { return $this; }
    public function setParameter(string $id, mixed $value): static { return $this; }
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

const VIEW_DI_PHP: &str = r#"<?php
use Yiisoft\View\WebView;

return [
    WebView::class => [
        'setParameters()' => [
            ['urlGenerator' => 'urls'],
        ],
    ],
];
"#;

const INJECTION_PHP: &str = r#"<?php
namespace App\Web;

use Yiisoft\Yii\View\Renderer\CommonParametersInjectionInterface;

final class UserInjection implements CommonParametersInjectionInterface
{
    public function getCommonParameters(): array
    {
        return ['currentUser' => 'guest'];
    }
}
"#;

const LOCALE_MIDDLEWARE_PHP: &str = r#"<?php
namespace App\Web;

use Yiisoft\View\WebView;

final class LocaleMiddleware
{
    public function __construct(private WebView $view) {}

    public function process(): void
    {
        $this->view->setParameter('locale', 'en');
    }
}
"#;

const POST_CONTROLLER_PHP: &str = r#"<?php
namespace App\Web\Blog;

use App\Blog\Post;
use Yiisoft\Yii\View\Renderer\WebViewRenderer;

final class PostController
{
    private WebViewRenderer $viewRenderer;

    public function __construct(WebViewRenderer $viewRenderer)
    {
        $this->viewRenderer = $viewRenderer->withViewPath('@views/blog');
    }

    public function view(Post $post): object
    {
        return $this->viewRenderer->render('post', ['post' => $post, 'title' => 'Post']);
    }

    public function wrong(Post $post): object
    {
        return $this->viewRenderer->render('post', ['post' => 'not a post', 'title' => 'Post', 'extra' => 1]);
    }

    public function short(): object
    {
        return $this->viewRenderer->render('post', ['title' => 'Post']);
    }

    public function unread(array $parameters): object
    {
        return $this->viewRenderer->render('post', $parameters);
    }

    public function plain(): object
    {
        return $this->viewRenderer->render('plain', ['anything' => 1]);
    }

    public function draft(): object
    {
        return $this->viewRenderer->render('post', ['']);
    }
}
"#;

const POST_PHP: &str = r#"<?php
namespace App\Blog;

final class Post
{
    public string $title = '';
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
        return $this->viewRenderer->render(__DIR__ . '/template', ['greeting' => 'Hello', 'count' => 'many']);
    }
}
"#;

const HOME_TEMPLATE_PHP: &str = r#"<?php

declare(strict_types=1);

use Yiisoft\View\WebView;

/**
 * @var WebView $this
 * @var string $greeting
 * @var int $count
 * @var string $urlGenerator
 * @var string $currentUser
 * @var string $locale
 * @var \Yiisoft\Yii\View\Renderer\Csrf $csrf
 */
?>
<h1><?= $greeting ?></h1>
"#;

const POST_TEMPLATE_PHP: &str = r#"<?php

declare(strict_types=1);

use App\Blog\Post;
use Yiisoft\View\WebView;

/**
 * @var WebView $this
 * @var Post $post
 * @var string $title
 */
?>
<h1><?= $title ?></h1>
<?= $this->render('_card', ['post' => $post, 'unused' => 1]) ?>
"#;

const CARD_TEMPLATE_PHP: &str = r#"<?php
/**
 * @var \App\Blog\Post $post
 */
?>
<article><?= $post->title ?></article>
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("stubs/renderer/WebViewRenderer.php", RENDERER_PHP),
        (
            "stubs/renderer/CommonParametersInjectionInterface.php",
            INJECTION_INTERFACE_PHP,
        ),
        ("stubs/view/ViewInterface.php", VIEW_INTERFACE_PHP),
        ("stubs/view/WebView.php", WEB_VIEW_PHP),
        ("config/common/aliases.php", ALIASES_PHP),
        ("config/common/params.php", PARAMS_PHP),
        ("config/web/di/view.php", VIEW_DI_PHP),
        ("src/Web/UserInjection.php", INJECTION_PHP),
        ("src/Web/LocaleMiddleware.php", LOCALE_MIDDLEWARE_PHP),
        ("src/Blog/Post.php", POST_PHP),
        ("src/Web/Blog/PostController.php", POST_CONTROLLER_PHP),
        ("src/Web/HomePage/Action.php", ACTION_PHP),
        ("src/Web/HomePage/template.php", HOME_TEMPLATE_PHP),
        ("resources/views/blog/post.php", POST_TEMPLATE_PHP),
        ("resources/views/blog/_card.php", CARD_TEMPLATE_PHP),
        ("resources/views/blog/plain.php", "<p>Plain</p>\n"),
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
    initialize_with_options(&backend, &dunce::canonicalize(dir.path()).unwrap(), None).await;
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
    let path = dir.path().join(rel);
    let path = dunce::canonicalize(&path)
        .unwrap_or_else(|_| dunce::canonicalize(dir.path()).unwrap().join(rel));
    Uri::from_file_path(path).unwrap()
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

/// The view parameter diagnostics `file` gets, as `code: message`.
fn diagnostics(backend: &Backend, dir: &tempfile::TempDir, file: &str) -> Vec<String> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(
        uri_for(dir, file).as_str(),
        content_of(file),
        &mut diagnostics,
    );
    diagnostics
        .iter()
        .filter_map(|diagnostic| match &diagnostic.code {
            Some(Code::String(code)) if code.contains("yii_view_parameter") => {
                Some(format!("{code}: {}", message_text(&diagnostic.message)))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn parameters_complete_from_the_templates_var_block() {
    let (backend, dir) = workspace(&["src/Web/Blog/PostController.php"]).await;
    let uri = uri_for(&dir, "src/Web/Blog/PostController.php");
    let position = position_in(POST_CONTROLLER_PHP, "render('post', ['']", 17);
    let response =
        completion_response(&backend, &uri, position.line, position.character, None).await;
    assert_eq!(response_labels(response), vec!["post", "title"]);

    let content = ACTION_PHP.replace("['greeting' => 'Hello', 'count' => 'many']", "['gr']");
    let uri = uri_for(&dir, "src/Web/HomePage/Action.php");
    open_document(&backend, &uri, "php", &content).await;
    let position = position_in(&content, "['gr']", 4);
    let response =
        completion_response(&backend, &uri, position.line, position.character, None).await;
    assert_eq!(response_labels(response), vec!["greeting"]);
}

#[tokio::test]
async fn a_parameter_navigates_to_its_declaration_and_hovers_its_type() {
    let (backend, dir) = workspace(&["src/Web/Blog/PostController.php"]).await;
    let uri = uri_for(&dir, "src/Web/Blog/PostController.php");
    let position = position_in(POST_CONTROLLER_PHP, "['post' => $post, 'title'", 3);
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: text_document_position(&uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let Some(DefinitionResponse::Definition(Definition::Location(location))) = response else {
        panic!("expected one location, got {response:?}");
    };
    assert_eq!(location.uri, uri_for(&dir, "resources/views/blog/post.php"));
    assert_eq!(
        location.range.start,
        position_in(POST_TEMPLATE_PHP, "$post\n", 0)
    );

    let hover = hover_response(&backend, &uri, position.line, position.character)
        .await
        .expect("a hover");
    let markup = hover_text(&hover);
    assert!(markup.contains("Post"), "{markup}");
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_templates_use_unsaved_parameters_from_the_canonical_file() {
    let (backend, dir) = workspace(&[]).await;
    let source = dir.path().join("resources/views");
    let target = dir.path().join("resources/templates");
    std::fs::rename(&source, &target).unwrap();
    std::os::unix::fs::symlink(&target, &source).unwrap();
    let template = uri_for(&dir, "resources/views/blog/post.php");
    let updated = POST_TEMPLATE_PHP.replace("@var Post $post", "@var string $post");
    open_document(&backend, &template, "php", &updated).await;
    let controller = uri_for(&dir, "src/Web/Blog/PostController.php");
    open_document(&backend, &controller, "php", POST_CONTROLLER_PHP).await;
    let position = position_in(POST_CONTROLLER_PHP, "['post' => $post, 'title'", 3);
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: text_document_position(&controller, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let Some(DefinitionResponse::Definition(Definition::Location(location))) = response else {
        panic!("expected one location, got {response:?}");
    };
    assert_eq!(location.uri, template);
    assert_eq!(location.range.start, position_in(&updated, "$post\n", 0));
    let hover = hover_response(&backend, &controller, position.line, position.character)
        .await
        .unwrap();
    assert!(hover_text(&hover).contains("string"), "{hover:?}");
}

#[tokio::test]
async fn parameters_are_checked_against_the_template() {
    let (backend, dir) = workspace(&[
        "src/Web/Blog/PostController.php",
        "src/Web/HomePage/Action.php",
        "resources/views/blog/post.php",
    ])
    .await;
    assert_eq!(
        diagnostics(&backend, &dir, "src/Web/Blog/PostController.php"),
        vec![
            "type_mismatch_yii_view_parameter: View 'resources/views/blog/post' expects $post of type App\\Blog\\Post, got 'not a post'".to_string(),
            "invalid_yii_view_parameter: View 'resources/views/blog/post' has no parameter $extra".to_string(),
            "missing_yii_view_parameter: View 'resources/views/blog/post' expects $post of type App\\Blog\\Post, which is not passed".to_string(),
        ]
    );
    // What the DI configuration, code, and the injections hand every view
    // is not the render's to pass.
    assert_eq!(
        diagnostics(&backend, &dir, "src/Web/HomePage/Action.php"),
        vec![
            "type_mismatch_yii_view_parameter: View 'src/Web/HomePage/template' expects $count of type int, got 'many'".to_string(),
        ]
    );
    // A view rendered from inside a template is checked the same way.
    assert_eq!(
        diagnostics(&backend, &dir, "resources/views/blog/post.php"),
        vec![
            "invalid_yii_view_parameter: View 'resources/views/blog/_card' has no parameter $unused".to_string(),
        ]
    );
}

#[tokio::test]
async fn parameters_set_in_a_way_that_cannot_be_read_leave_nothing_missing() {
    let (backend, dir) = workspace(&["src/Web/Blog/PostController.php"]).await;
    let missing = |backend: &Backend| {
        diagnostics(backend, &dir, "src/Web/Blog/PostController.php")
            .into_iter()
            .filter(|diagnostic| diagnostic.starts_with("missing_"))
            .count()
    };
    assert_eq!(missing(&backend), 1);

    let shared = "<?php\nnamespace App\\Web;\n\nfinal class Shared\n{\n    public function __construct(private \\Yiisoft\\View\\WebView $view) {}\n\n    public function share(array $parameters): void\n    {\n        $this->view->setParameters($parameters);\n    }\n}\n";
    open_document(
        &backend,
        &uri_for(&dir, "src/Web/Shared.php"),
        "php",
        shared,
    )
    .await;
    assert_eq!(missing(&backend), 0);
}
