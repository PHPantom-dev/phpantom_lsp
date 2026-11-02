//! WordPress hook names, from the listeners that use them to the places
//! that fire them, and the names core builds at runtime.

use crate::common::{
    completion_response, create_psr4_workspace, offset_position, open_document, response_items,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "autoload": { "psr-4": { "Acme\\Shop\\": "src/" } }
}"#;

const FIRES_PHP: &str = r#"<?php
namespace Acme\Shop;

final class Checkout
{
    public function place(object $order, \WP_Post $post): void
    {
        do_action('acme_order_placed', $order);
        $total = apply_filters('acme_order_total', 0, $order);
        do_action("save_post_{$post->post_type}", $post->ID, $post);
        do_action('acme_status_' . $order->status, $order);
    }
}
"#;

const LISTENS_PHP: &str = r#"<?php
namespace Acme\Shop;

final class Mailer
{
    public function register(): void
    {
        add_action('acme_order_placed', [$this, 'sendReceipt']);
        add_filter('acme_order_total', [$this, 'addShipping'], 10, 2);
        add_action('save_post_product', [$this, 'reindex'], 10, 3);
        add_action('acme_status_paid', [$this, 'thank']);
        remove_action('acme_order_placed', [$this, 'sendReceipt']);
        has_action('save_post');
    }
}
"#;

/// A workspace holding `files`, every one of them open.
async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let (backend, dir) = create_psr4_workspace(COMPOSER, files);
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

fn text_position(uri: &Uri, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position,
    }
}

async fn definitions(backend: &Backend, uri: &Uri, position: Position) -> Vec<(Uri, u32)> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: text_position(uri, position),
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

async fn references(
    backend: &Backend,
    uri: &Uri,
    position: Position,
    include_declaration: bool,
) -> Vec<(Uri, u32)> {
    let mut found: Vec<(Uri, u32)> = backend
        .references(ReferenceParams {
            text_document_position_params: text_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration,
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

fn sorted(mut locations: Vec<(Uri, u32)>) -> Vec<(Uri, u32)> {
    locations.sort();
    locations
}

#[tokio::test]
async fn a_used_hook_goes_to_every_place_that_fires_it() {
    let (backend, dir) = workspace(&[
        ("src/Checkout.php", FIRES_PHP),
        ("src/Mailer.php", LISTENS_PHP),
    ])
    .await;
    let fires = uri_for(&dir, "src/Checkout.php");
    let listens = uri_for(&dir, "src/Mailer.php");

    let found = definitions(
        &backend,
        &listens,
        position_in(LISTENS_PHP, "'acme_order_placed'", 3),
    )
    .await;
    assert_eq!(found, vec![(fires.clone(), 7)]);

    let found = definitions(
        &backend,
        &listens,
        position_in(LISTENS_PHP, "'acme_order_total'", 3),
    )
    .await;
    assert_eq!(found, vec![(fires.clone(), 8)]);

    // A hook nothing in the project fires has nowhere to go.
    let found = definitions(
        &backend,
        &listens,
        position_in(LISTENS_PHP, "'save_post'", 3),
    )
    .await;
    assert!(found.is_empty(), "{found:?}");
}

#[tokio::test]
async fn a_name_built_at_runtime_matches_by_its_literal_parts() {
    let (backend, dir) = workspace(&[
        ("src/Checkout.php", FIRES_PHP),
        ("src/Mailer.php", LISTENS_PHP),
    ])
    .await;
    let fires = uri_for(&dir, "src/Checkout.php");
    let listens = uri_for(&dir, "src/Mailer.php");

    // Interpolated.
    let found = definitions(
        &backend,
        &listens,
        position_in(LISTENS_PHP, "'save_post_product'", 3),
    )
    .await;
    assert_eq!(found, vec![(fires.clone(), 9)]);

    // Concatenated.
    let found = definitions(
        &backend,
        &listens,
        position_in(LISTENS_PHP, "'acme_status_paid'", 3),
    )
    .await;
    assert_eq!(found, vec![(fires.clone(), 10)]);

    // From the place that fires it, a pattern finds every name it matches.
    let found = references(
        &backend,
        &fires,
        position_in(FIRES_PHP, "save_post_{", 2),
        true,
    )
    .await;
    assert_eq!(
        found,
        sorted(vec![(fires.clone(), 9), (listens.clone(), 9)])
    );
}

#[tokio::test]
async fn references_find_the_uses_and_optionally_the_places_that_fire() {
    let (backend, dir) = workspace(&[
        ("src/Checkout.php", FIRES_PHP),
        ("src/Mailer.php", LISTENS_PHP),
    ])
    .await;
    let fires = uri_for(&dir, "src/Checkout.php");
    let listens = uri_for(&dir, "src/Mailer.php");

    let position = position_in(LISTENS_PHP, "'acme_order_placed'", 3);
    let found = references(&backend, &listens, position, true).await;
    assert_eq!(
        found,
        sorted(vec![
            (fires.clone(), 7),
            (listens.clone(), 7),
            (listens.clone(), 11)
        ])
    );
    let found = references(&backend, &listens, position, false).await;
    assert_eq!(found, vec![(listens.clone(), 7), (listens.clone(), 11)]);
}

#[tokio::test]
async fn hook_names_complete_in_every_hook_function() {
    let typing = r#"<?php
add_action('acme_');
do_action('save_');
add_filter(hook_name: 'save_post_p');
other_function('acme_');
"#;
    let (backend, dir) = workspace(&[
        ("src/Checkout.php", FIRES_PHP),
        ("src/Mailer.php", LISTENS_PHP),
        ("src/typing.php", typing),
    ])
    .await;
    let uri = uri_for(&dir, "src/typing.php");

    let items = |position: Position| {
        let backend = &backend;
        let uri = &uri;
        async move {
            response_items(
                completion_response(backend, uri, position.line, position.character, None).await,
            )
            .unwrap_or_default()
        }
    };

    let found = items(position_in(typing, "acme_')", 5)).await;
    let labels: Vec<&str> = found.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "acme_order_placed",
            "acme_order_total",
            "acme_status_paid",
            "acme_status_{$order->status}",
        ]
    );

    // A pattern is inserted as a snippet with a placeholder for its
    // computed part.
    let found = items(position_in(typing, "save_')", 5)).await;
    let pattern = found
        .iter()
        .find(|item| item.label == "save_post_{$post->post_type}")
        .expect("the pattern should be offered");
    assert_eq!(pattern.insert_text_format, Some(InsertTextFormat::Snippet));
    let Some(CompletionItemTextEdit::TextEdit(edit)) = &pattern.text_edit else {
        panic!("expected a text edit");
    };
    assert_eq!(edit.new_text, "save_post_${1:post_type}");
    assert!(found.iter().any(|item| item.label == "save_post_product"));

    let found = items(position_in(typing, "save_post_p')", 11)).await;
    let labels: Vec<&str> = found.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["save_post_product"]);

    // Any other function's string is not a hook name.
    let found = items(position_in(typing, "other_function('acme_", 21)).await;
    assert!(
        found.iter().all(|item| item.label != "acme_order_placed"),
        "{found:?}"
    );
}

#[tokio::test]
async fn renaming_a_hook_rewrites_every_literal_that_spells_it() {
    let (backend, dir) = workspace(&[
        ("src/Checkout.php", FIRES_PHP),
        ("src/Mailer.php", LISTENS_PHP),
    ])
    .await;
    let fires = uri_for(&dir, "src/Checkout.php");
    let listens = uri_for(&dir, "src/Mailer.php");

    let rename = |uri: &Uri, position: Position, new_name: &str| {
        backend.rename(RenameParams {
            text_document_position_params: text_position(uri, position),
            new_name: new_name.to_string(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
    };

    let edit = rename(
        &listens,
        position_in(LISTENS_PHP, "'acme_order_placed'", 3),
        "acme/order/placed",
    )
    .await
    .unwrap()
    .expect("the hook should be renamed");
    let changes = edit.changes.expect("plain text edits");
    let mut edited: Vec<(Uri, u32, String)> = changes
        .into_iter()
        .flat_map(|(uri, edits)| {
            edits
                .into_iter()
                .map(move |edit| (uri.clone(), edit.range.start.line, edit.new_text))
        })
        .collect();
    edited.sort();
    let new = "acme/order/placed".to_string();
    assert_eq!(
        edited,
        sorted_edits(vec![
            (fires.clone(), 7, new.clone()),
            (listens.clone(), 7, new.clone()),
            (listens.clone(), 11, new.clone()),
        ])
    );

    // A use that a pattern matches is renamed without the pattern.
    let edit = rename(
        &listens,
        position_in(LISTENS_PHP, "'save_post_product'", 3),
        "save_post_page",
    )
    .await
    .unwrap()
    .expect("the hook should be renamed");
    let changes = edit.changes.expect("plain text edits");
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[&listens].len(), 1);

    // A pattern spells no name to rename, and a name with a quote in it
    // cannot be written back.
    let prepared = backend
        .prepare_rename(text_position(
            &fires,
            position_in(FIRES_PHP, "save_post_{", 2),
        ))
        .await
        .unwrap();
    assert!(prepared.is_none(), "{prepared:?}");
    let refused = rename(
        &listens,
        position_in(LISTENS_PHP, "'acme_order_total'", 3),
        "it's",
    )
    .await;
    assert!(refused.is_err(), "{refused:?}");
}

fn sorted_edits(mut edits: Vec<(Uri, u32, String)>) -> Vec<(Uri, u32, String)> {
    edits.sort();
    edits
}

#[tokio::test]
async fn a_hook_is_highlighted_where_the_file_fires_and_uses_it() {
    let both = r#"<?php
add_action('init', 'acme_boot');
do_action('init');
add_action('wp_loaded', 'acme_boot');
do_action("init_{$type}");
"#;
    let (backend, dir) = workspace(&[("src/both.php", both)]).await;
    let uri = uri_for(&dir, "src/both.php");

    let highlights = backend
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: text_position(&uri, position_in(both, "'init'", 2)),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default();
    let found: Vec<(u32, Option<DocumentHighlightKind>)> = highlights
        .into_iter()
        .map(|highlight| (highlight.range.start.line, highlight.kind))
        .collect();
    assert_eq!(
        found,
        [
            (1, Some(DocumentHighlightKind::Read)),
            (2, Some(DocumentHighlightKind::Write)),
        ]
    );
}

#[tokio::test]
async fn hovering_a_hook_says_where_it_fires() {
    let (backend, dir) = workspace(&[
        ("src/Checkout.php", FIRES_PHP),
        ("src/Mailer.php", LISTENS_PHP),
    ])
    .await;
    let listens = uri_for(&dir, "src/Mailer.php");

    let hover = |position: Position| {
        backend.hover(HoverParams {
            text_document_position_params: text_position(&listens, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
    };
    let text = |hover: Option<Hover>| match hover.map(|hover| hover.contents) {
        Some(Contents::MarkupContent(markup)) => markup.value,
        other => panic!("unexpected hover {other:?}"),
    };

    let shown = text(
        hover(position_in(LISTENS_PHP, "'acme_order_placed'", 3))
            .await
            .unwrap(),
    );
    assert!(shown.contains("**Hook** `acme_order_placed`"), "{shown}");
    assert!(shown.contains("Fired in 1 place, 2 uses"), "{shown}");

    let shown = text(
        hover(position_in(LISTENS_PHP, "'save_post'", 3))
            .await
            .unwrap(),
    );
    assert!(shown.contains("Not fired in the project, 1 use"), "{shown}");
}
