//! Twig template variables: the names a template reads, from the renders
//! and templates that pass them, the globals the project declares, and what
//! the template sets itself.
use crate::common::{
    completion_response, create_psr4_workspace, hover_text, offset_position, open_document,
    response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Symfony\\Bundle\\FrameworkBundle\\": "stubs/framework-bundle/",
        "Symfony\\Bridge\\Twig\\": "stubs/twig-bridge/",
        "Twig\\": "stubs/twig/"
    } }
}"#;

/// Just enough of the framework for the type engine to tell what renders a
/// template from any other receiver.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/framework-bundle/Controller/AbstractController.php",
        "<?php\nnamespace Symfony\\Bundle\\FrameworkBundle\\Controller;\nabstract class AbstractController {\n    protected function render(string $view, array $parameters = []): object {}\n}\n",
    ),
    (
        "stubs/twig-bridge/Attribute/Template.php",
        "<?php\nnamespace Symfony\\Bridge\\Twig\\Attribute;\n#[\\Attribute]\nclass Template {\n    public function __construct(public string $template, public ?array $vars = null) {}\n}\n",
    ),
    (
        "stubs/twig/Environment.php",
        "<?php\nnamespace Twig;\nclass Environment {\n    public function render($name, array $context = []): string {}\n    public function addGlobal(string $name, $value): void {}\n}\n",
    ),
    (
        "stubs/twig/Extension/GlobalsInterface.php",
        "<?php\nnamespace Twig\\Extension;\ninterface GlobalsInterface {\n    public function getGlobals(): array;\n}\n",
    ),
];

const CONTROLLER_PHP: &str = r#"<?php
namespace App\Controller;

use Symfony\Bridge\Twig\Attribute\Template;
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Twig\Environment;

final class PostController extends AbstractController
{
    public function show(object $post): object
    {
        return $this->render('post/show.html.twig', ['post' => $post, 'related' => []]);
    }

    public function preview(Environment $twig, \ArrayObject $other, object $post): string
    {
        $other->offsetSet('post/show.html.twig', ['ignored' => 1]);
        return $twig->render('post/show.html.twig', compact('post'));
    }

    #[Template('post/list.html.twig')]
    public function list(int $page): void {}
}
"#;

const GLOBALS_PHP: &str = r#"<?php
namespace App\Twig;

use Twig\Environment;
use Twig\Extension\GlobalsInterface;

final class SiteGlobals implements GlobalsInterface
{
    public function getGlobals(): array
    {
        return ['site_name' => 'Acme'];
    }

    public function boot(Environment $twig): void
    {
        $twig->addGlobal('support_email', 'help@example.com');
    }
}
"#;

const TWIG_YAML: &str = "twig:\n    globals:\n        ga_tracking: 'UA-1'\n";

const BASE_TWIG: &str =
    "<title>{{ site_name }} {{ page_title }}</title>{% block body %}{% endblock %}\n";
const SHOW_TWIG: &str = "{% extends 'base.html.twig' %}
{% types {score?: 'int'} %}
{% set page_title = post.title %}
{% block body %}
  {{ post.body }} {{ ga_tracking }} {{ support_email }} {{ app.user }}
  {% for item in related %}{% include 'post/_item.html.twig' with {highlight: true} %}{% endfor %}
  {{ score }} {{ missing }}
  {% macro row(label) %}{{ label }}{{ post }}{% endmacro %}
{% endblock %}
";
const ITEM_TWIG: &str = "<li>{{ item }} {{ highlight }} {{ post }}</li>\n";
const LIST_TWIG: &str = "{{ page }}\n";

const PROJECT: &[(&str, &str)] = &[
    ("src/Controller/PostController.php", CONTROLLER_PHP),
    ("src/Twig/SiteGlobals.php", GLOBALS_PHP),
    ("config/packages/twig.yaml", TWIG_YAML),
    ("templates/base.html.twig", BASE_TWIG),
    ("templates/post/show.html.twig", SHOW_TWIG),
    ("templates/post/_item.html.twig", ITEM_TWIG),
    ("templates/post/list.html.twig", LIST_TWIG),
];

/// A workspace holding the stubs and the project, every file of it open.
async fn workspace() -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = STUBS.iter().chain(PROJECT).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in PROJECT {
        let language = match path.rsplit('.').next() {
            Some("php") => "php",
            Some("yaml") => "yaml",
            _ => "twig",
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

fn line_of(content: &str, needle: &str) -> u32 {
    position_in(content, needle, 0).line
}

fn text_document_position(uri: &Uri, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position,
    }
}

fn named(locations: Vec<Location>) -> Vec<(String, u32)> {
    let mut found: Vec<(String, u32)> = locations
        .into_iter()
        .map(|location| {
            let path = location.uri.path().to_string();
            let file = path.rsplit('/').next().unwrap().to_string();
            (file, location.range.start.line)
        })
        .collect();
    found.sort();
    found
}

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<(String, u32)> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: text_document_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    named(match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    })
}

async fn hover(backend: &Backend, uri: &Uri, position: Position) -> String {
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: text_document_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("a hover");
    hover_text(&hover).to_string()
}

fn pair(file: &str, line: u32) -> (String, u32) {
    (file.to_string(), line)
}

#[tokio::test]
async fn a_variable_leads_to_the_renders_that_pass_it() {
    let (backend, dir) = workspace().await;
    let show = uri_for(&dir, "templates/post/show.html.twig");

    // Both renders pass `post`, but the `offsetSet()` on another receiver
    // renders nothing.
    assert_eq!(
        definitions(&backend, &show, position_in(SHOW_TWIG, "post.body", 1)).await,
        vec![
            pair("PostController.php", line_of(CONTROLLER_PHP, "'post' =>")),
            pair(
                "PostController.php",
                line_of(CONTROLLER_PHP, "compact('post')")
            ),
        ]
    );
    assert_eq!(
        definitions(&backend, &show, position_in(SHOW_TWIG, "related", 1)).await,
        vec![pair(
            "PostController.php",
            line_of(CONTROLLER_PHP, "'related'")
        )]
    );
    // A `#[Template]` action that returns nothing renders its arguments.
    let list = uri_for(&dir, "templates/post/list.html.twig");
    assert_eq!(
        definitions(&backend, &list, position_in(LIST_TWIG, "page", 1)).await,
        vec![pair(
            "PostController.php",
            line_of(CONTROLLER_PHP, "int $page")
        )]
    );
}

#[tokio::test]
async fn globals_and_local_definitions_resolve() {
    let (backend, dir) = workspace().await;
    let show = uri_for(&dir, "templates/post/show.html.twig");

    for (needle, expected) in [
        ("ga_tracking }}", pair("twig.yaml", 2)),
        (
            "support_email }}",
            pair("SiteGlobals.php", line_of(GLOBALS_PHP, "'support_email'")),
        ),
        // The loop's target and the `types` tag are the template's own.
        (
            "item in",
            pair("show.html.twig", line_of(SHOW_TWIG, "{% for")),
        ),
        (
            "score }}",
            pair("show.html.twig", line_of(SHOW_TWIG, "{% types")),
        ),
        (
            "label }}",
            pair("show.html.twig", line_of(SHOW_TWIG, "{% macro")),
        ),
    ] {
        assert_eq!(
            definitions(&backend, &show, position_in(SHOW_TWIG, needle, 1)).await,
            vec![expected],
            "for {needle}"
        );
    }
    let base = uri_for(&dir, "templates/base.html.twig");
    assert_eq!(
        definitions(&backend, &base, position_in(BASE_TWIG, "site_name", 1)).await,
        vec![pair("SiteGlobals.php", line_of(GLOBALS_PHP, "'site_name'"))]
    );
}

#[tokio::test]
async fn a_macro_sees_none_of_the_template_s_context() {
    let (backend, dir) = workspace().await;
    let show = uri_for(&dir, "templates/post/show.html.twig");
    let inside_macro = SHOW_TWIG.find("{{ post }}").unwrap() + 4;
    assert_eq!(
        definitions(&backend, &show, offset_position(SHOW_TWIG, inside_macro)).await,
        vec![]
    );
}

#[tokio::test]
async fn templates_pass_their_context_on() {
    let (backend, dir) = workspace().await;
    // The partial is included from inside the loop, with a hash of its own,
    // and gets the context of the page that includes it.
    let item = uri_for(&dir, "templates/post/_item.html.twig");
    assert_eq!(
        definitions(&backend, &item, position_in(ITEM_TWIG, "item }}", 1)).await,
        vec![pair("show.html.twig", line_of(SHOW_TWIG, "{% for"))]
    );
    assert_eq!(
        definitions(&backend, &item, position_in(ITEM_TWIG, "highlight", 1)).await,
        vec![pair("show.html.twig", line_of(SHOW_TWIG, "highlight:"))]
    );
    assert_eq!(
        definitions(&backend, &item, position_in(ITEM_TWIG, "post }}", 1)).await,
        vec![
            pair("PostController.php", line_of(CONTROLLER_PHP, "'post' =>")),
            pair(
                "PostController.php",
                line_of(CONTROLLER_PHP, "compact('post')")
            ),
        ]
    );
    // The layout is rendered with the context of the page that extends it.
    let base = uri_for(&dir, "templates/base.html.twig");
    assert_eq!(
        definitions(&backend, &base, position_in(BASE_TWIG, "page_title", 1)).await,
        vec![pair("show.html.twig", line_of(SHOW_TWIG, "{% set"))]
    );
}

#[tokio::test]
async fn hover_says_where_a_variable_comes_from() {
    let (backend, dir) = workspace().await;
    let show = uri_for(&dir, "templates/post/show.html.twig");

    let text = hover(&backend, &show, position_in(SHOW_TWIG, "post.body", 1)).await;
    assert!(text.contains("**Twig variable** `post`"), "{text}");
    assert!(
        text.contains("Passed by `src/Controller/PostController.php`"),
        "{text}"
    );
    let text = hover(&backend, &show, position_in(SHOW_TWIG, "score }}", 1)).await;
    assert!(
        text.contains("Declared as `int`, optional, by this template's `types` tag"),
        "{text}"
    );
    let text = hover(&backend, &show, position_in(SHOW_TWIG, "ga_tracking }}", 1)).await;
    assert!(
        text.contains("A global, declared in `config/packages/twig.yaml`"),
        "{text}"
    );
    let text = hover(&backend, &show, position_in(SHOW_TWIG, "app.user", 1)).await;
    assert!(text.contains("Symfony's `AppVariable`"), "{text}");
    let text = hover(&backend, &show, position_in(SHOW_TWIG, "missing", 1)).await;
    assert!(
        text.contains("Not passed by any render that was found"),
        "{text}"
    );
}

#[tokio::test]
async fn variables_complete_where_an_expression_starts() {
    let content = "{% set local = 1 %}{{  }}{{ x| }}\n";
    let (backend, dir) = workspace().await;
    let path = dir.path().join("templates/post/show.html.twig");
    std::fs::write(&path, content).unwrap();
    let uri = Uri::from_file_path(&path).unwrap();
    open_document(&backend, &uri, "twig", content).await;

    let position = position_in(content, "{{  }}", 3);
    let labels = response_labels(
        completion_response(&backend, &uri, position.line, position.character, None).await,
    );
    for expected in [
        "local",
        "post",
        "related",
        "ga_tracking",
        "site_name",
        "support_email",
        "app",
        "_self",
    ] {
        assert!(
            labels.iter().any(|label| label == expected),
            "{expected} in {labels:?}"
        );
    }
    assert!(!labels.iter().any(|label| label == "ignored"), "{labels:?}");
    // After a `|` only a filter can follow.
    let position = position_in(content, "| }}", 1);
    let labels = response_labels(
        completion_response(&backend, &uri, position.line, position.character, None).await,
    );
    assert!(!labels.iter().any(|label| label == "post"), "{labels:?}");
}

#[tokio::test]
async fn references_stay_in_the_template_and_reach_the_render() {
    let (backend, dir) = workspace().await;
    let show = uri_for(&dir, "templates/post/show.html.twig");
    let locations = backend
        .references(ReferenceParams {
            text_document_position_params: text_document_position(
                &show,
                position_in(SHOW_TWIG, "related", 1),
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
    assert_eq!(
        named(locations),
        vec![
            pair("PostController.php", line_of(CONTROLLER_PHP, "'related'")),
            pair("show.html.twig", line_of(SHOW_TWIG, "related")),
        ]
    );
}
