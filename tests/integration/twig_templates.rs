//! Twig template names in a project that uses Twig without Symfony: the
//! roots its loader reads are found from the names its templates
//! reference, and its templates are `.html` files.
use crate::common::message_text;

use crate::common::{
    completion_response, initialize_with_options, offset_position, open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "twig/twig": "^3.4" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Twig\\": "stubs/twig/"
    } }
}"#;

const ENVIRONMENT_PHP: &str = "<?php\nnamespace Twig;\nclass Environment {\n    public function render($name, array $context = []): string {}\n}\n";

const DEFAULT_INDEX: &str =
    "<html>{% block content %}{% endblock %}{% include 'menu.html' %}</html>\n";
const DEFAULT_LIST: &str =
    "{% extends \"index.html\" %}\n{% block content %}{{ path('app_home') }}{% endblock %}\n";
const ADMIN_LIST: &str = "{% extends \"admin/index.html\" %}\n";
const OTHER_INDEX: &str = "{% extends 'index.html' %}{% include 'menu.html' %}\n";

const PAGE_PHP: &str = r#"<?php
namespace App;

use Twig\Environment;

final class Page
{
    public function show(Environment $twig, \ArrayObject $other): string
    {
        $other->offsetGet('list.html');
        $twig->render('missing.html');
        $twig->render('style.css');
        return $twig->render('list.html');
    }
}
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        ("stubs/twig/Environment.php", ENVIRONMENT_PHP),
        ("src/Page.php", PAGE_PHP),
        ("theme/default/index.html", DEFAULT_INDEX),
        ("theme/default/menu.html", "<nav></nav>\n"),
        ("theme/default/list.html", DEFAULT_LIST),
        ("theme/default/admin/index.html", "<main></main>\n"),
        ("theme/default/admin/list.html", ADMIN_LIST),
        ("theme/other/index.html", OTHER_INDEX),
        ("theme/other/menu.html", "<nav class=\"other\"></nav>\n"),
        // Not a template: nothing references into its directory.
        (
            "docs/page.html",
            "<p>{% raw %}{% include 'x.html' %}{% endraw %}</p>\n",
        ),
    ]
}

/// A session over the workspace with every template open, in an order that
/// names a template before the file it names is seen, and `open` open too.
async fn workspace(open: &[&str]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files() {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    let templates = files()
        .into_iter()
        .map(|(path, _)| path)
        .filter(|path| path.starts_with("theme/"));
    for path in templates.chain(open.iter().copied()) {
        let content = files()
            .into_iter()
            .find(|(file, _)| *file == path)
            .unwrap()
            .1;
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

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<Uri> {
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
    let mut found: Vec<Uri> = match response {
        None => Vec::new(),
        Some(DefinitionResponse::Definition(Definition::Location(location))) => vec![location.uri],
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => {
            locations.into_iter().map(|location| location.uri).collect()
        }
        Some(DefinitionResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    };
    found.sort();
    found
}

#[tokio::test]
async fn template_names_resolve_against_the_discovered_roots() {
    let (backend, dir) = workspace(&["src/Page.php"]).await;
    let page = uri_for(&dir, "src/Page.php");
    let default_index = uri_for(&dir, "theme/default/index.html");
    let other_index = uri_for(&dir, "theme/other/index.html");

    for (uri, content, needle, expected) in [
        (
            uri_for(&dir, "theme/default/list.html"),
            DEFAULT_LIST,
            "\"index.html",
            // A name two roots hold is either file, as the loader decides.
            vec![default_index.clone(), other_index.clone()],
        ),
        (
            uri_for(&dir, "theme/default/admin/list.html"),
            ADMIN_LIST,
            "\"admin/index.html",
            vec![uri_for(&dir, "theme/default/admin/index.html")],
        ),
        (
            other_index.clone(),
            OTHER_INDEX,
            "'menu.html",
            vec![
                uri_for(&dir, "theme/default/menu.html"),
                uri_for(&dir, "theme/other/menu.html"),
            ],
        ),
        (
            page.clone(),
            PAGE_PHP,
            "render('list.html",
            vec![uri_for(&dir, "theme/default/list.html")],
        ),
    ] {
        let inside = needle.find(['\'', '"']).unwrap() + 2;
        let found = definitions(&backend, &uri, position_in(content, needle, inside)).await;
        assert_eq!(found, expected, "for {needle}");
    }

    // `offsetGet()` on something that renders nothing names nothing.
    let found = definitions(
        &backend,
        &page,
        position_in(PAGE_PHP, "offsetGet('list", 12),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn template_names_complete_in_php_and_templates() {
    let php =
        "<?php\nnamespace App;\nfunction f(\\Twig\\Environment $twig) { $twig->render('me'); }\n";
    let twig = "{% include 'me' %}\n";
    let (backend, dir) = workspace(&["src/Page.php"]).await;
    let php_uri = uri_for(&dir, "src/Render.php");
    let twig_uri = uri_for(&dir, "theme/default/new.html");
    open_document(&backend, &php_uri, "php", php).await;
    open_document(&backend, &twig_uri, "twig", twig).await;

    for (uri, content, needle) in [(&php_uri, php, "render('me'"), (&twig_uri, twig, "'me'")] {
        let position = position_in(content, needle, needle.len() - 1);
        let labels = response_labels(
            completion_response(&backend, uri, position.line, position.character, None).await,
        );
        assert_eq!(labels, vec!["menu.html".to_string()], "for {needle}");
    }
}

#[tokio::test]
async fn only_missing_names_of_a_template_extension_are_reported() {
    let (backend, dir) = workspace(&["src/Page.php"]).await;
    let uri = uri_for(&dir, "src/Page.php");

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), PAGE_PHP, &mut diagnostics);
    let reported: Vec<String> = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(Code::String(code)) if code == "invalid_symfony_template"
            )
        })
        .map(|diagnostic| message_text(&diagnostic.message).to_string())
        .collect();
    assert_eq!(
        reported,
        vec!["Unknown template: 'missing.html'".to_string()]
    );
}

/// Route names are Symfony's, so outside Symfony a template's `path()` is
/// not one, and an `.html` file outside every root is not a template.
#[tokio::test]
async fn symfony_names_and_other_html_are_left_alone() {
    let (backend, dir) = workspace(&[]).await;
    let found = definitions(
        &backend,
        &uri_for(&dir, "theme/default/list.html"),
        position_in(DEFAULT_LIST, "app_home", 2),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");

    let docs = "<p>{% raw %}{% include 'x.html' %}{% endraw %}</p>\n";
    let docs_uri = uri_for(&dir, "docs/page.html");
    open_document(&backend, &docs_uri, "html", docs).await;
    let position = position_in(docs, "'x.html", 2);
    let labels = response_labels(
        completion_response(&backend, &docs_uri, position.line, position.character, None).await,
    );
    assert!(!labels.contains(&"menu.html".to_string()), "{labels:?}");
}
