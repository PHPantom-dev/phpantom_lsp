//! The types of Twig template variables: what the renders pass them, what
//! the template sets them to, and the attributes read off them.
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
        "Twig\\": "stubs/twig/"
    } }
}"#;

const STUBS: &[(&str, &str)] = &[
    (
        "stubs/framework-bundle/Controller/AbstractController.php",
        "<?php\nnamespace Symfony\\Bundle\\FrameworkBundle\\Controller;\nabstract class AbstractController {\n    protected function render(string $view, array $parameters = []): object {}\n}\n",
    ),
    (
        "stubs/twig/Environment.php",
        "<?php\nnamespace Twig;\nclass Environment {\n    public function render($name, array $context = []): string {}\n}\n",
    ),
    (
        "stubs/twig/Extension/GlobalsInterface.php",
        "<?php\nnamespace Twig\\Extension;\ninterface GlobalsInterface {\n    public function getGlobals(): array;\n}\n",
    ),
];

const POST_PHP: &str = r#"<?php
namespace App\Entity;

class Post
{
    public string $slug = '';

    public function getTitle(): string { return ''; }

    public function isPublished(): bool { return true; }

    public function getAuthor(): Author { return new Author(); }

    /** @return list<Comment> */
    public function getComments(): array { return []; }
}
"#;

const AUTHOR_PHP: &str = r#"<?php
namespace App\Entity;

class Author
{
    public function getName(): string { return ''; }
}
"#;

const COMMENT_PHP: &str = r#"<?php
namespace App\Entity;

class Comment
{
    public function getBody(): string { return ''; }
}
"#;

const CONTROLLER_PHP: &str = r#"<?php
namespace App\Controller;

use App\Entity\Author;
use App\Entity\Post;
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Twig\Environment;

final class PostController extends AbstractController
{
    public function show(Post $post): object
    {
        return $this->render('post/show.html.twig', ['post' => $post, 'count' => 3]);
    }

    public function author(Environment $twig, Author $author): string
    {
        $params = ['author' => $author, 'posts' => [new Post()]];
        return $twig->render('author/show.html.twig', $params);
    }
}
"#;

const GLOBALS_PHP: &str = r#"<?php
namespace App\Twig;

use App\Entity\Author;
use Twig\Extension\GlobalsInterface;

final class SiteGlobals implements GlobalsInterface
{
    public function getGlobals(): array
    {
        return ['editor' => new Author()];
    }
}
"#;

const SHOW_TWIG: &str = "{% set headline = post.title %}
<h1>{{ post.title }}</h1>
<p>{{ post.author.name }} {{ count }} {{ headline }} {{ editor.name }}</p>
{% for comment in post.comments %}{{ comment.body }}{% include 'post/_comment.html.twig' with {text: comment.body} %}{% endfor %}
{{ post. }}
";
const COMMENT_TWIG: &str = "{{ text }} {{ comment.body }}\n";
const AUTHOR_TWIG: &str = "{% types {featured?: '\\\\App\\\\Entity\\\\Post'} %}
{{ author.name }} {% for post in posts %}{{ post.title }}{% endfor %} {{ featured.title }}
";

const PROJECT: &[(&str, &str)] = &[
    ("src/Entity/Post.php", POST_PHP),
    ("src/Entity/Author.php", AUTHOR_PHP),
    ("src/Entity/Comment.php", COMMENT_PHP),
    ("src/Controller/PostController.php", CONTROLLER_PHP),
    ("src/Twig/SiteGlobals.php", GLOBALS_PHP),
    ("templates/post/show.html.twig", SHOW_TWIG),
    ("templates/post/_comment.html.twig", COMMENT_TWIG),
    ("templates/author/show.html.twig", AUTHOR_TWIG),
];

async fn workspace() -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = STUBS.iter().chain(PROJECT).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in PROJECT {
        let language = if path.ends_with(".php") {
            "php"
        } else {
            "twig"
        };
        open_document(&backend, &uri_for(&dir, path), language, content).await;
    }
    (backend, dir)
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dir.path().join(rel)).unwrap()
}

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

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<(String, u32)> {
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
        .map(|location| {
            let path = location.uri.path().to_string();
            let file = path.rsplit('/').next().unwrap().to_string();
            (file, location.range.start.line)
        })
        .collect()
}

fn line_of(content: &str, needle: &str) -> u32 {
    position_in(content, needle, 0).line
}

#[tokio::test]
async fn a_variable_holds_what_the_renders_pass() {
    let (backend, dir) = workspace().await;
    let show = uri_for(&dir, "templates/post/show.html.twig");
    for (needle, expected) in [
        ("post.title }}", "`App\\Entity\\Post`"),
        ("count }}", "`3`"),
        // Set from an attribute, and iterated out of a list.
        ("headline }}", "`string`"),
        ("comment.body }}{%", "`App\\Entity\\Comment`"),
        // A global holds what the extension returns.
        ("editor.name", "`App\\Entity\\Author`"),
    ] {
        let text = hover(&backend, &show, position_in(SHOW_TWIG, needle, 1)).await;
        assert!(text.contains(expected), "{needle}: {text}");
    }
    // An included template holds what the one including it passes on.
    let partial = uri_for(&dir, "templates/post/_comment.html.twig");
    let text = hover(&backend, &partial, position_in(COMMENT_TWIG, "text", 1)).await;
    assert!(text.contains("`string`"), "{text}");
    let text = hover(&backend, &partial, position_in(COMMENT_TWIG, "comment", 1)).await;
    assert!(text.contains("`App\\Entity\\Comment`"), "{text}");
}

#[tokio::test]
async fn a_context_built_in_a_variable_names_its_entries() {
    let (backend, dir) = workspace().await;
    let author = uri_for(&dir, "templates/author/show.html.twig");
    let text = hover(
        &backend,
        &author,
        position_in(AUTHOR_TWIG, "author.name", 1),
    )
    .await;
    assert!(text.contains("`App\\Entity\\Author`"), "{text}");
    assert!(
        text.contains("Passed by `src/Controller/PostController.php`"),
        "{text}"
    );
    let text = hover(&backend, &author, position_in(AUTHOR_TWIG, "post.title", 1)).await;
    assert!(text.contains("`App\\Entity\\Post`"), "{text}");
    // The `types` tag's declaration is what the template holds.
    let text = hover(
        &backend,
        &author,
        position_in(AUTHOR_TWIG, "featured.title", 1),
    )
    .await;
    assert!(text.contains("`App\\Entity\\Post`"), "{text}");
}

#[tokio::test]
async fn attributes_lead_to_the_members_they_read() {
    let (backend, dir) = workspace().await;
    let show = uri_for(&dir, "templates/post/show.html.twig");
    assert_eq!(
        definitions(&backend, &show, position_in(SHOW_TWIG, "title }}", 0)).await,
        vec![("Post.php".to_string(), line_of(POST_PHP, "getTitle"))]
    );
    assert_eq!(
        definitions(&backend, &show, position_in(SHOW_TWIG, "name }}", 2)).await,
        vec![("Author.php".to_string(), line_of(AUTHOR_PHP, "getName"))]
    );
    let author = uri_for(&dir, "templates/author/show.html.twig");
    assert_eq!(
        definitions(&backend, &author, position_in(AUTHOR_TWIG, "title }}{%", 1)).await,
        vec![("Post.php".to_string(), line_of(POST_PHP, "getTitle"))]
    );
    let text = hover(&backend, &show, position_in(SHOW_TWIG, "title }}", 0)).await;
    assert!(text.contains("function getTitle(): string"), "{text}");
    // The variable itself still hovers as the variable.
    let text = hover(&backend, &show, position_in(SHOW_TWIG, "post.author", 1)).await;
    assert!(text.contains("**Twig variable** `post`"), "{text}");
}

#[tokio::test]
async fn attributes_complete_by_the_names_twig_reads() {
    let (backend, dir) = workspace().await;
    let show = uri_for(&dir, "templates/post/show.html.twig");
    let position = position_in(SHOW_TWIG, "post. }}", 5);
    let labels = response_labels(
        completion_response(&backend, &show, position.line, position.character, None).await,
    );
    for expected in ["slug", "title", "published", "author", "comments"] {
        assert!(
            labels.iter().any(|label| label == expected),
            "{expected} in {labels:?}"
        );
    }
    assert!(
        !labels.iter().any(|label| label == "getTitle"),
        "{labels:?}"
    );
}
