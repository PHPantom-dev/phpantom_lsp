//! Dynamic component selectors use the caller's inferred PHP values.

use crate::common::{
    completion_response, create_psr4_workspace, definition_locations, goto_definition_at, hover_on,
    open_php, position_after, position_of, response_labels, template_diagnostics, workspace_uri,
};
use tower_lsp_server::{LanguageServer, gen_lsp_types::*};

const COMPOSER: &str = r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#;
const BOOK: &str = "<?php namespace App; class Book { public string $title; public function summary(): string {} }";
const CARD: &str = "{{ $entry->title }}";

#[tokio::test]
async fn tempest_dynamic_components_infer_literal_variable_and_constant_selectors() {
    for selector in [
        "is=\"x-card\"",
        ":is=\"'x-card'\"",
        ":is=\" $component \"",
        ":is=\"Names::CARD\"",
        ":is=\"'x-' . 'card'\"",
    ] {
        let source = format!(
            "<?php use App\\Book; use App\\Names; $component = 'x-card'; $private = new Book(); ?>\n<x-component {selector} :entry=\"new Book()\" />"
        );
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Book.php", BOOK),
                (
                    "app/Names.php",
                    "<?php namespace App; class Names { const CARD = 'x-card'; }",
                ),
                ("app/page.view.php", &source),
                ("app/x-card.view.php", CARD),
                (
                    "app/x-component.view.php",
                    "<?php /** @var string $is */ ?>",
                ),
            ],
        );
        let uri = workspace_uri(&backend, "app/page.view.php");
        open_php(&backend, &uri, &source).await;
        let target = workspace_uri(&backend, "app/x-card.view.php");
        open_php(&backend, &target, CARD).await;
        assert_eq!(
            template_diagnostics(&backend, &target),
            Vec::<String>::new(),
            "{selector}"
        );
        assert!(
            hover_on(&backend, &target, CARD, "title")
                .await
                .contains("string"),
            "{selector}"
        );
        let virtual_php = backend.blade_virtual_php(target.as_str()).unwrap();
        assert!(!virtual_php.contains("$private = null"), "{virtual_php}");
        let pos = position_after(CARD, "$entry->");
        let names = response_labels(
            completion_response(&backend, &target, pos.line, pos.character, None).await,
        );
        assert!(names.contains(&"summary()".to_string()), "{names:?}");
    }
}

#[tokio::test]
async fn tempest_dynamic_components_follow_caller_data_and_clear_changed_targets() {
    let source = "<x-component :is=\"$component\" :entry=\"$book\" />";
    let caller = "<?php \\Tempest\\View\\view(__DIR__.'/page.view.php', component: 'x-card', book: new \\App\\Book());";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/page.view.php", source),
            ("app/caller.php", caller),
            ("app/x-card.view.php", CARD),
            ("app/x-panel.view.php", CARD),
        ],
    );
    let caller_uri = workspace_uri(&backend, "app/caller.php");
    let page_uri = workspace_uri(&backend, "app/page.view.php");
    let card_uri = workspace_uri(&backend, "app/x-card.view.php");
    let panel_uri = workspace_uri(&backend, "app/x-panel.view.php");
    open_php(&backend, &page_uri, source).await;
    open_php(&backend, &card_uri, CARD).await;
    open_php(&backend, &panel_uri, CARD).await;
    for (version, (name, card, panel)) in [
        ("x-card", true, false),
        ("x-panel", false, true),
        ("missing", false, false),
    ]
    .into_iter()
    .enumerate()
    {
        backend
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    text_document_identifier: TextDocumentIdentifier {
                        uri: caller_uri.clone(),
                    },
                    version: version as i32 + 1,
                },
                content_changes: vec![
                    TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                        TextDocumentContentChangeWholeDocument {
                            text: caller.replace("x-card", name),
                        },
                    ),
                ],
            })
            .await;
        for (target, expected) in [(&card_uri, card), (&panel_uri, panel)] {
            let php = backend.blade_virtual_php(target.as_str()).unwrap();
            assert_eq!(php.contains("$entry = null"), expected, "{name}: {php}");
        }
    }
}

#[tokio::test]
async fn tempest_dynamic_components_propagate_through_components_and_slot_scopes() {
    let page = "<?php use App\\Book; ?><x-host selected=\"x-card\" :book=\"new Book()\" />";
    let host =
        "<x-component :is=\"$selected\" :entry=\"$book\">{{ $local->summary() }}</x-component>";
    let card = "<?php $local = $entry; ?><x-slot />";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/page.view.php", page),
            ("app/x-host.view.php", host),
            ("app/x-card.view.php", card),
        ],
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "app/page.view.php"),
        page,
    )
    .await;
    let uri = workspace_uri(&backend, "app/x-host.view.php");
    open_php(&backend, &uri, host).await;
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    assert!(
        hover_on(&backend, &uri, host, "summary")
            .await
            .contains("string")
    );
    let pos = position_of(host, "summary");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character + 1).await);
    assert_eq!(locations[0].uri, workspace_uri(&backend, "app/Book.php"));
}

#[tokio::test]
async fn tempest_dynamic_components_complete_selected_attributes_and_navigate_literals() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/page.view.php", ""),
            (
                "app/x-card.view.php",
                "<?php /** @var string $bookTitle */ ?>",
            ),
            (
                "app/x-panel.view.php",
                "<?php /** @var string $bookTitle */ /** @var bool $enabled */ ?>",
            ),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    for selector in ["is=\"x-card\"", ":is=\"'x-card'\"", ":is=\"$name\""] {
        let source = format!("<?php $name = 'x-card'; ?><x-component {selector} :bo />");
        open_php(&backend, &uri, &source).await;
        let pos = position_after(&source, ":bo");
        let names = response_labels(
            completion_response(&backend, &uri, pos.line, pos.character, None).await,
        );
        assert_eq!(names, vec![":book-title"], "{selector}");
        if selector.contains("x-card") {
            let pos = position_of(&source, selector);
            let offset = selector.find("x-card").unwrap() as u32;
            let locations = definition_locations(
                goto_definition_at(&backend, &uri, pos.line, pos.character + offset + 2).await,
            );
            assert_eq!(
                locations[0].uri,
                Uri::from_file_path(
                    dunce::canonicalize(dir.path().join("app/x-card.view.php")).unwrap()
                )
                .unwrap()
            );
        } else {
            let pos = position_after(&source, ":is=\"$na");
            let locations = definition_locations(
                goto_definition_at(&backend, &uri, pos.line, pos.character).await,
            );
            assert_eq!(
                locations[0].uri, uri,
                "PHP variable navigation must remain intact"
            );
        }
    }
    let source = "<?php /** @var 'x-card'|'x-panel' $name */ ?><x-component :is=\"$name\" :bo />";
    open_php(&backend, &uri, source).await;
    let pos = position_after(source, ":bo");
    assert_eq!(
        response_labels(completion_response(&backend, &uri, pos.line, pos.character, None).await),
        vec![":book-title"]
    );
}

#[tokio::test]
async fn tempest_dynamic_components_union_targets_are_known_without_guessing_unknown_names() {
    for (expression, expected) in [
        ("$name", true),
        ("$nullable", true),
        ("$unknown", false),
        ("'missing'", false),
        ("42", false),
        (" ", false),
    ] {
        let source = format!(
            "<?php /** @var 'x-card'|'x-panel' $name */ /** @var ?('x-card'|'x-panel') $nullable */ ?><x-component :is=\"{expression}\" :entry=\"new \\App\\Book()\" />"
        );
        let (backend, _dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("app/Book.php", BOOK),
                ("app/page.view.php", &source),
                ("app/x-card.view.php", CARD),
                ("app/x-panel.view.php", CARD),
            ],
        );
        open_php(
            &backend,
            &workspace_uri(&backend, "app/page.view.php"),
            &source,
        )
        .await;
        for path in ["app/x-card.view.php", "app/x-panel.view.php"] {
            let uri = workspace_uri(&backend, path);
            open_php(&backend, &uri, CARD).await;
            assert_eq!(
                backend
                    .blade_virtual_php(uri.as_str())
                    .unwrap()
                    .contains("$entry = null"),
                expected,
                "{expression}: {path}"
            );
        }
    }
}

#[tokio::test]
async fn tempest_dynamic_components_forward_runtime_attribute_types_and_inner_view_data() {
    let caller = "<?php \\Tempest\\View\\view(__DIR__.'/page.view.php', disabled: 'fallback');";
    let page = "<x-component is=\"x-card\" :amount=\"42\" :enabled=\"true\" :disabled=\"false\" :absent=\"null\" :entry=\"new \\App\\Book()\" :entries=\"[new \\App\\Book()]\" />";
    let card = "{{ $amount }} {{ $enabled }} {{ $disabled }} {{ $entry->title }} {{ $this->entry->summary() }} <x-leaf />";
    let leaf = "{{ $entry->summary() }} <p :foreach=\"$entries as $book\">{{ $book->title }}</p>";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/caller.php", caller),
            ("app/page.view.php", page),
            ("app/x-card.view.php", card),
            ("app/x-leaf.view.php", leaf),
        ],
    );
    open_php(&backend, &workspace_uri(&backend, "app/caller.php"), caller).await;
    open_php(
        &backend,
        &workspace_uri(&backend, "app/page.view.php"),
        page,
    )
    .await;
    let uri = workspace_uri(&backend, "app/x-card.view.php");
    open_php(&backend, &uri, card).await;
    let php = backend.blade_virtual_php(uri.as_str()).unwrap();
    assert!(php.contains("@var '42' $amount"), "{php}");
    assert!(php.contains("@var '' $enabled"), "{php}");
    assert!(php.contains("@var 'fallback' $disabled"), "{php}");
    assert!(!php.contains("$absent = null"), "{php}");
    assert_eq!(template_diagnostics(&backend, &uri), Vec::<String>::new());
    let leaf_uri = workspace_uri(&backend, "app/x-leaf.view.php");
    open_php(&backend, &leaf_uri, leaf).await;
    assert_eq!(
        template_diagnostics(&backend, &leaf_uri),
        Vec::<String>::new()
    );
    assert!(
        hover_on(&backend, &leaf_uri, leaf, "summary")
            .await
            .contains("string")
    );
}

#[tokio::test]
async fn tempest_dynamic_components_unknown_targets_keep_slot_locals_isolated() {
    let source = "<?php $private = new \\App\\Book(); ?><x-component :is=\"$unknown\" :entry=\"new \\App\\Book()\">{{ $entry->title }} {{ $private->title }}</x-component>";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[("app/Book.php", BOOK), ("app/page.view.php", source)],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    let diagnostics = template_diagnostics(&backend, &uri);
    assert!(
        diagnostics
            .iter()
            .any(|message| message.contains("Undefined variable '$private'")),
        "{diagnostics:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|message| message.contains("Undefined variable '$entry'")),
        "{diagnostics:?}"
    );
}

#[tokio::test]
async fn tempest_dynamic_components_preserve_nullable_and_union_attribute_alternatives() {
    let source = r#"<?php
/** @var bool $flag */
/** @var int|float $number */
/** @var ?\App\Book $maybe */
/** @var \App\Book|'label' $choice */
?>
<x-component is="x-card" :flag="$flag" :number="$number" :maybe="$maybe" :choice="$choice" :unknown="$unknown" :this="new \App\Book()" BookTitle="Title" />
"#;
    let target =
        "{{ $flag }} {{ $number }} {{ $maybe }} {{ $choice }} {{ $bookTitle }} {{ $unknown }}";
    let caller = "<?php \\Tempest\\View\\view(__DIR__.'/page.view.php', flag: 'fallback', maybe: 'missing', choice: 'previous');";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/caller.php", caller),
            ("app/page.view.php", source),
            ("app/x-card.view.php", target),
        ],
    );
    open_php(&backend, &workspace_uri(&backend, "app/caller.php"), caller).await;
    open_php(
        &backend,
        &workspace_uri(&backend, "app/page.view.php"),
        source,
    )
    .await;
    let uri = workspace_uri(&backend, "app/x-card.view.php");
    open_php(&backend, &uri, target).await;
    let php = backend.blade_virtual_php(uri.as_str()).unwrap();
    assert!(
        hover_on(&backend, &uri, target, "$flag")
            .await
            .contains("fallback"),
        "{php}"
    );
    assert!(
        hover_on(&backend, &uri, target, "$number")
            .await
            .contains("numeric-string"),
        "{php}"
    );
    let maybe = hover_on(&backend, &uri, target, "$maybe").await;
    assert!(
        maybe.contains("Book") && maybe.contains("missing"),
        "{maybe}"
    );
    let choice = hover_on(&backend, &uri, target, "$choice").await;
    assert!(
        choice.contains("Book") && choice.contains("label") && !choice.contains("previous"),
        "{choice}"
    );
    assert!(php.contains("@var 'Title' $bookTitle"), "{php}");
    assert!(php.contains("@var mixed $unknown"), "{php}");
}

#[tokio::test]
async fn tempest_dynamic_components_replace_targets_when_slot_scope_arrives() {
    let host = "<?php $component = 'x-panel'; ?><x-slot />";
    let page = "<?php $component = 'x-card'; ?><x-host><x-component :is=\"$component\" :entry=\"new \\App\\Book()\" /></x-host>";
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/x-host.view.php", host),
            ("app/page.view.php", page),
            ("app/x-card.view.php", CARD),
            ("app/x-panel.view.php", CARD),
        ],
    );
    let card = workspace_uri(&backend, "app/x-card.view.php");
    let panel = workspace_uri(&backend, "app/x-panel.view.php");
    open_php(&backend, &card, CARD).await;
    open_php(&backend, &panel, CARD).await;
    open_php(
        &backend,
        &workspace_uri(&backend, "app/page.view.php"),
        page,
    )
    .await;
    assert!(
        !backend
            .blade_virtual_php(card.as_str())
            .unwrap()
            .contains("$entry = null")
    );
    assert!(
        hover_on(&backend, &panel, CARD, "title")
            .await
            .contains("string")
    );
}

#[tokio::test]
async fn tempest_dynamic_components_complete_selectors_and_tolerate_incomplete_attributes() {
    let (backend, _dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/page.view.php", ""),
            (
                "app/x-card.view.php",
                "<?php /** @var string $bookTitle */ ?>",
            ),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    for (source, marker, expected) in [
        ("<x-component :i />", ":i", Some(":is")),
        ("<x-component i />", " i", Some("is")),
        ("<x-component is :bo />", ":bo", None),
        ("<x-component :is :bo />", ":bo", None),
        ("<x-component :is=\" \" :bo />", ":bo", None),
        ("<x-component :is=\"$unknown\" :bo />", ":bo", None),
        ("<x-component is=\"missing\" :bo />", ":bo", None),
        ("<x-component ::is=\"'x-card'\" :bo />", ":bo", None),
        ("<x-component is=\"x-card\" i />", "\" i", None),
    ] {
        open_php(&backend, &uri, source).await;
        let pos = position_after(source, marker);
        let names = response_labels(
            completion_response(&backend, &uri, pos.line, pos.character, None).await,
        );
        if let Some(expected) = expected {
            assert!(names.contains(&expected.to_owned()), "{source}: {names:?}");
        } else {
            assert!(names.is_empty(), "{source}: {names:?}");
        }
    }
}

#[tokio::test]
async fn tempest_dynamic_components_resolve_selectors_inside_reordered_loops_and_crlf() {
    let source = "<p>😀</p>\r\n<x-component :is=\"$tag\" :entry=\"new \\App\\Book()\" :foreach=\"['x-card', 'x-panel'] as $tag\" />\r\n<x-component :is=\" 'x-card' \" :entry=\"new \\App\\Book()\" />";
    let target = "{{ $entry->title }}";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Book.php", BOOK),
            ("app/page.view.php", source),
            ("app/x-card.view.php", target),
            ("app/x-panel.view.php", target),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    for name in ["x-card", "x-panel"] {
        let target_uri = workspace_uri(&backend, &format!("app/{name}.view.php"));
        open_php(&backend, &target_uri, target).await;
        assert!(
            hover_on(&backend, &target_uri, target, "title")
                .await
                .contains("string"),
            "{name}"
        );
    }
    let pos = position_after(source, ":is=\" 'x-ca");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await);
    assert_eq!(
        dunce::canonicalize(locations[0].uri.to_file_path().unwrap()).unwrap(),
        dunce::canonicalize(dir.path().join("app/x-card.view.php")).unwrap()
    );
}

#[tokio::test]
async fn tempest_dynamic_components_forward_closures_and_handle_removed_component_files() {
    let source = "<?php $callback = fn (): string => 'ready'; ?><x-component is=\"x-card\" :callback=\"$callback\" :bo />";
    let target = "{{ $callback() }}";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/page.view.php", source),
            ("app/x-card.view.php", target),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    let target_uri = workspace_uri(&backend, "app/x-card.view.php");
    open_php(&backend, &target_uri, target).await;
    let hover = hover_on(&backend, &target_uri, target, "$callback").await;
    assert!(
        hover.contains("Closure") && (hover.contains("string") || hover.contains("ready")),
        "{hover}: {}",
        backend.blade_virtual_php(target_uri.as_str()).unwrap()
    );
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: target_uri },
        })
        .await;
    std::fs::remove_file(dir.path().join("app/x-card.view.php")).unwrap();
    let pos = position_after(source, ":bo");
    assert!(
        response_labels(completion_response(&backend, &uri, pos.line, pos.character, None).await)
            .is_empty()
    );
}
