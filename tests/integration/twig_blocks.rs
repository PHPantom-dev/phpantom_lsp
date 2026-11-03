//! Twig blocks: from a block to the one it overrides, from a block to the
//! ones that override it, and the names a template can override.
use crate::common::{
    completion_response, hover_text, initialize_with_options, offset_position, open_document,
    response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{ "require": { "twig/twig": "^3.4" } }"#;

const BASE: &str = "<title>{% block title %}Site{% endblock %}</title>
<main>{% block body %}{% endblock %}</main>
<h1>{{ block('title') }}</h1>
";
const LAYOUT: &str = "{% extends 'base.html.twig' %}
{% block body %}<div>{% block content %}{% endblock %}</div>{% endblock %}
";
const PAGE: &str = "{% extends 'layout.html.twig' %}
{% use 'blocks.html.twig' with sidebar as base_sidebar %}
{% block title %}Page · {{ parent() }}{% endblock %}
{% block content %}
  {{ block('base_sidebar') }}
  {% embed 'card.html.twig' %}
    {% block body %}Card{% endblock %}
  {% endembed %}
{% endblock content %}
";
const OTHER: &str = "{% extends 'base.html.twig' %}{% block title %}Other{% endblock %}\n";
const UNRELATED: &str = "{% block title %}Alone{% endblock %}\n";
const BLOCKS: &str = "{% block sidebar %}<aside></aside>{% endblock %}\n";
const CARD: &str = "<div class=\"card\">{% block body %}{% endblock %}</div>\n";

const FILES: &[(&str, &str)] = &[
    ("composer.json", COMPOSER),
    ("templates/base.html.twig", BASE),
    ("templates/layout.html.twig", LAYOUT),
    ("templates/page.html.twig", PAGE),
    ("templates/other.html.twig", OTHER),
    ("templates/unrelated.html.twig", UNRELATED),
    ("templates/blocks.html.twig", BLOCKS),
    ("templates/card.html.twig", CARD),
];

/// A session over the templates, with the ones in `open` open: the rest
/// are read from disk.
async fn workspace(open: &[&str]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in FILES {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    for path in open {
        let content = FILES.iter().find(|(file, _)| file == path).unwrap().1;
        open_document(&backend, &uri_for(&dir, path), "twig", content).await;
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

/// Where go-to-definition leads, as each file's name and the line.
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
    named(locations)
}

async fn references(backend: &Backend, uri: &Uri, position: Position) -> Vec<(String, u32)> {
    let locations = backend
        .references(ReferenceParams {
            text_document_position_params: text_document_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default();
    named(locations)
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

fn at(file: &str, line: u32) -> (String, u32) {
    (file.to_string(), line)
}

#[tokio::test]
async fn a_block_leads_to_the_block_it_overrides() {
    let (backend, dir) = workspace(&["templates/page.html.twig"]).await;
    let page = uri_for(&dir, "templates/page.html.twig");

    for (needle, inside, expected) in [
        // Defined two templates up.
        ("block title", 7, at("base.html.twig", 0)),
        ("parent()", 2, at("base.html.twig", 0)),
        ("block content", 7, at("layout.html.twig", 1)),
        ("endblock content", 10, at("layout.html.twig", 1)),
        // Pulled in from a `use`d template under another name.
        ("block('base_sidebar", 10, at("blocks.html.twig", 0)),
        // A block in an embed overrides the embedded template's.
        ("block body", 7, at("card.html.twig", 0)),
    ] {
        let found = definitions(&backend, &page, position_in(PAGE, needle, inside)).await;
        assert_eq!(found, vec![expected], "for {needle}");
    }
}

#[tokio::test]
async fn a_block_that_overrides_none_leads_to_its_overrides() {
    let (backend, dir) = workspace(&["templates/base.html.twig"]).await;
    let base = uri_for(&dir, "templates/base.html.twig");

    // An unrelated template's `title` is a block of its own.
    let found = definitions(&backend, &base, position_in(BASE, "block title", 7)).await;
    assert_eq!(
        found,
        vec![at("other.html.twig", 0), at("page.html.twig", 2)]
    );

    // The embed's `body` overrides the card's, not this one.
    let found = definitions(&backend, &base, position_in(BASE, "block body", 7)).await;
    assert_eq!(found, vec![at("layout.html.twig", 1)]);

    // `block()` renders the template's own block.
    let found = definitions(&backend, &base, position_in(BASE, "block('title", 8)).await;
    assert_eq!(found, vec![at("base.html.twig", 0)]);
}

#[tokio::test]
async fn references_stay_among_the_templates_that_extend_each_other() {
    let (backend, dir) = workspace(&["templates/other.html.twig"]).await;
    let other = uri_for(&dir, "templates/other.html.twig");

    let found = references(&backend, &other, position_in(OTHER, "block title", 7)).await;
    assert_eq!(
        found,
        vec![
            at("base.html.twig", 0),
            at("base.html.twig", 2),
            at("other.html.twig", 0),
            at("page.html.twig", 2),
            at("page.html.twig", 2),
        ]
    );
}

#[tokio::test]
async fn hover_says_what_a_block_overrides() {
    let (backend, dir) = workspace(&["templates/page.html.twig"]).await;
    let page = uri_for(&dir, "templates/page.html.twig");
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: text_document_position(
                &page,
                position_in(PAGE, "block title", 7),
            ),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("a hover");
    let text = hover_text(&hover);
    assert!(
        text.contains("Overrides the block in `templates/base.html.twig`"),
        "{text}"
    );
}

#[tokio::test]
async fn the_blocks_a_template_can_override_complete() {
    let child = "{% extends 'page.html.twig' %}\n{% block title %}{% endblock %}\n{% block \n{{ block('') }}\n";
    let (backend, dir) = workspace(&[]).await;
    let uri = uri_for(&dir, "templates/child.html.twig");
    std::fs::write(dir.path().join("templates/child.html.twig"), child).unwrap();
    open_document(&backend, &uri, "twig", child).await;

    let labels = |position: Position| {
        let backend = &backend;
        let uri = uri.clone();
        async move {
            response_labels(
                completion_response(backend, &uri, position.line, position.character, None).await,
            )
        }
    };

    // `title` is overridden already, and the embed's `body` is the card's.
    let tag = labels(position_in(child, "{% block \n", 9)).await;
    assert_eq!(tag, ["base_sidebar", "body", "content"]);

    let call = labels(position_in(child, "block('')", 7)).await;
    assert_eq!(call, ["base_sidebar", "body", "content", "title"]);
}
