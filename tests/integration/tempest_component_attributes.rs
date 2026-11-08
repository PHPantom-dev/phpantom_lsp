//! Component attributes navigate and hover through the receiving view.

use crate::common::{
    create_psr4_workspace, definition_locations, goto_definition_at, hover_response, hover_text,
    open_php, position_after, position_of, workspace_path, workspace_uri,
};
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#;
const BOOK: &str = "<?php namespace App; class Book { public string $title; }";

#[tokio::test]
async fn tempest_component_attributes_find_first_reads_and_use_declared_types() {
    let card = "<?php\r\nuse App\\Book;\r\n/**\r\n * @var Book $book\r\n * @var bool $showAuthor\r\n */\r\n?>\r\n<!-- $book is documented here. -->\r\n<p :if=\"$showAuthor\">📚 {{ $book->title }} {{ $book->title }}</p>\r\n";
    let page = r#"<x-card :book="new \App\Book()" show-author />"#;
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/page.view.php", page),
            ("app/x-card.view.php", card),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, page).await;
    for (attribute, read, variable, ty) in [
        (":book", "{{ $book", "$book", "Book"),
        ("show-author", ":if=\"$showAuthor", "$showAuthor", "bool"),
    ] {
        let at = position_of(page, attribute);
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
        let end = position_after(card, read);
        assert_eq!(
            locations,
            vec![Location {
                uri: Uri::from_file_path(
                    dunce::canonicalize(workspace_path(&backend, "app/x-card.view.php")).unwrap()
                )
                .unwrap(),
                range: Range {
                    start: Position::new(end.line, end.character - variable.len() as u32),
                    end,
                },
            }],
            "{attribute}"
        );
        let hover = hover_response(&backend, &uri, at.line, at.character)
            .await
            .expect(attribute);
        assert!(hover_text(&hover).contains(ty), "{hover:?}");
        assert!(hover_text(&hover).contains(variable), "{hover:?}");
        let end = position_after(page, attribute);
        assert_eq!(
            definition_locations(goto_definition_at(&backend, &uri, end.line, end.character).await),
            locations
        );
        assert_eq!(
            hover_response(&backend, &uri, end.line, end.character).await,
            Some(hover.clone())
        );
        assert_eq!(
            hover.range,
            Some(Range {
                start: at,
                end: position_after(page, attribute),
            })
        );
    }
}

#[tokio::test]
async fn tempest_component_attributes_follow_dynamic_targets_and_inferred_data() {
    let card = "{{ $book->title }}";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/page.view.php", ""),
            ("app/x-card.view.php", card),
            (
                "app/x-number.view.php",
                "<?php /** @var int $book */ ?>{{ $book }}",
            ),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    for selector in ["is=\"x-card\"", ":is=\"'x-card'\"", ":is=\"$component\""] {
        let page = format!(
            "<?php $component = 'x-card'; ?><x-component {selector} :book=\"new \\App\\Book()\" />"
        );
        open_php(&backend, &uri, &page).await;
        let at = position_after(&page, ":bo");
        let locations =
            definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
        assert_eq!(locations.len(), 1, "{selector}");
        assert_eq!(
            locations[0].uri,
            Uri::from_file_path(
                dunce::canonicalize(workspace_path(&backend, "app/x-card.view.php")).unwrap()
            )
            .unwrap()
        );
        let hover = hover_response(&backend, &uri, at.line, at.character)
            .await
            .expect(selector);
        assert!(hover_text(&hover).contains("Book"), "{hover:?}");
    }
    let page = r#"<?php /** @var 'x-card'|'x-number' $component */ ?><x-component :is="$component" :book="new \App\Book()" />"#;
    open_php(&backend, &uri, page).await;
    let at = position_after(page, ":bo");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
    assert_eq!(locations.len(), 2);
    let hover = hover_response(&backend, &uri, at.line, at.character)
        .await
        .unwrap();
    let text = hover_text(&hover);
    assert!(text.contains("Book") && text.contains("int"), "{text}");
}

#[tokio::test]
async fn tempest_component_attributes_ignore_directives_values_and_non_reads() {
    let card = r#"<?php
/** @var string $unused */
$written = 'local';
?>
{{ $if }} {{ $foreach }} {{ $is }} {{ $attributes }} {{ $slots }}
{{ $book }}
"#;
    let page = r#"<?php $book = 'Caller'; ?>
<!-- <x-card :book="$book" /> -->
<x-card :book="$book" ::book="frontend" :if="$book" :foreach="[] as $entry" unused="x" written="x" attributes="x" slots="x" unknown="x" />
<x-component is="x-card" :is="'x-card'" />
<x-missing book="x" />
<div book="x" />
</x-card book>
"#;
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[("app/page.view.php", page), ("app/x-card.view.php", card)],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, page).await;
    for needle in [
        "<!-- <x-card :bo",
        "::bo",
        ":if",
        ":foreach",
        "unus",
        "writt",
        "attribut",
        "slots",
        "unkno",
        "<x-component i",
        ":is",
        "<x-missing bo",
        "<div bo",
        "</x-card bo",
    ] {
        let at = position_after(page, needle);
        assert!(
            definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await)
                .is_empty(),
            "{needle}"
        );
        assert!(
            hover_response(&backend, &uri, at.line, at.character)
                .await
                .is_none(),
            "{needle}"
        );
    }
    // Values still use PHP's navigation in the caller.
    let at = position_after(page, "\n<x-card :book=\"$bo");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
    assert_eq!(locations[0].uri, uri);
    assert_eq!(locations[0].range.start, position_of(page, "$book"));
}

#[tokio::test]
async fn tempest_component_attributes_use_unsaved_component_reads() {
    let page = r#"<x-card :book="new \App\Book()" />"#;
    let original = "<?php /** @var string $book */ ?>{{ $book }}";
    let edited = "<?php /** @var int $book */ ?>\n<p>{{ $book }}</p>";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/page.view.php", page),
            ("app/x-card.view.php", original),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, page).await;
    let target = workspace_uri(&backend, "app/x-card.view.php");
    open_php(&backend, &target, edited).await;
    let at = position_after(page, ":bo");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
    assert_eq!(locations[0].uri, target);
    assert_eq!(locations[0].range.start, Position::new(1, 6));
    let hover = hover_response(&backend, &uri, at.line, at.character)
        .await
        .unwrap();
    assert!(hover_text(&hover).contains("int"), "{hover:?}");
}

#[tokio::test]
async fn tempest_component_attributes_choose_the_first_read_in_source_order() {
    let page = "<x-card :enabled=\"true\" />";
    let card = "<p :title=\"$enabled\" :if=\"$enabled\">{{ $enabled }}</p>";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[("app/page.view.php", page), ("app/x-card.view.php", card)],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, page).await;
    let at = position_after(page, ":en");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].range.start, position_of(card, "$enabled"));
}
