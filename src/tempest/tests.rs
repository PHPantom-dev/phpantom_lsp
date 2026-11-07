use std::collections::BTreeMap;

use super::{
    inference::Scope,
    preprocessor::preprocess,
    syntax::{Token, tokens},
};
use crate::text_position::LineIndex;
use tower_lsp_server::gen_lsp_types::Position;

#[test]
fn tempest_php_regions_round_trip_after_reordering_unicode_and_crlf() {
    let source = "<?php /** @var list<Book> $books */ ?>\r\n<div :title=\"$book->title\"\r\n:foreach=\"$books as $book\">📚 {{ $book->summary() }}\r\n</div>";
    let (php, map) = preprocess(source);
    assert!(map.lowered_from(source));
    assert!(php.find("foreach").unwrap() < php.find("$book->title").unwrap());
    let original_lines = LineIndex::new(source);
    let php_lines = LineIndex::new(&php);
    for needle in [
        "list<Book>",
        "$book->title",
        "$books as $book",
        "$book->summary()",
    ] {
        let original = source.find(needle).unwrap();
        let generated = php.find(needle).unwrap();
        for offset in 0..=needle.len() {
            let source_position = original_lines.position(original + offset);
            let php_position = php_lines.position(generated + offset);
            assert_eq!(
                map.blade_to_php(source_position),
                php_position,
                "{needle} at {offset}"
            );
            assert_eq!(
                map.try_php_to_blade(php_position),
                Some(source_position),
                "{needle} at {offset}"
            );
        }
    }
    assert_eq!(map.try_php_to_blade(Position::default()), None);
    assert_eq!(
        map.blade_to_php(original_lines.position(source.find("<div").unwrap())),
        Position::default()
    );
}

#[test]
fn tempest_lexer_keeps_php_delimiters_inside_literals_and_comments() {
    let source = r#"<!-- {{ hidden() }} -->{{-- {!! hidden() !!} --}}<?xml version="1.0"?>
<?php $a = "?>"; /* ?> */ $b = `?>`; // ?>
<x-card <?= $a ?> class='<?= $b ?>' :title="$a" flag />
{{ ['}}', "}}"] /* }} */ }} {!! '!!}' !!}
"#;
    let parsed = tokens(source);
    assert_eq!(parsed.len(), 4);
    let Token::Tag(tag) = &parsed[1] else {
        panic!("missing tag")
    };
    assert!(tag.self_closing);
    assert_eq!(tag.attributes.len(), 4);
    assert_eq!(tag.attributes[3].name, "flag");
    assert!(tag.attributes[3].value.is_none());
    for token in parsed {
        if let Token::Php { code, .. } = token {
            assert!(!source[code].contains("hidden"));
        }
    }
}

#[test]
fn tempest_incomplete_markup_and_unclosed_php_are_recoverable() {
    for source in [
        "<",
        "<p <span>",
        "<div :if=\"$yes\">",
        "<div :isset=\" $x || true \" />",
        "<x-card",
        "<x-card / ! = attr=unquoted",
        "<p :if>",
        "<p :foreach=\"$xs as $x\"><i>{{ $x }}</p></missing>",
        "<input :if=\"$x\" :value=\"$x\">",
        "<!-- unfinished",
        "{{-- unfinished",
        "<p <?php $x = 1",
        "{{ 'unfinished\\",
        "<p :title='unterminated",
        "<div :isset=\"$x['key']->name || $y\">{{ $x }}</div>",
        "<p :if=\"$x\">Yes</p><p :elseif=\"$y\">Maybe</p><p :else>No</p>",
        "<p :forelse />",
        "<div :foreach=\"$a as $x\" /><p :forelse />",
    ] {
        let (php, map) = preprocess(source);
        assert!(php.starts_with("<?php\nfunction "), "{source}");
        assert!(map.lowered_from(source));
    }
}

#[test]
fn tempest_native_php_terminates_statements_but_keeps_docblocks_attached() {
    let (php, _) = preprocess(
        "<?php /** @var string $name */ ?>{{ $name }}<?php 'text' // tail ?><?= $name ?><?php if (true) { ?><?php } ?>",
    );
    assert!(php.contains("/** @var string $name */ \necho"), "{php}");
    assert!(php.contains("'text' // tail \n;"), "{php}");
    assert!(php.contains("if (true) { \n }"), "{php}");
    let (php, _) = preprocess("<?php $s = <<<'HTML'\n?> {{ not_php }}\nHTML\n?>{{ $s }}");
    assert!(php.contains("HTML\n\n;\necho"), "{php}");
}

#[test]
fn view_files_outside_a_tempest_project_stay_plain_php() {
    let backend = crate::Backend::new_test();
    let uri = crate::util::path_to_uri(&std::env::temp_dir().join("plain/page.view.php"));
    let uri = uri.as_str();
    let source = "<?php $title = 'Home'; ?>\n<div :class=\"{ open: isOpen }\">{{ title }}</div>\n";

    backend.set_is_tempest(false);
    backend.update_ast(uri, source);
    assert!(!backend.is_php_template(uri));
    assert!(backend.template_virtual_php_arc(uri).is_none());
    assert!(
        backend
            .tempest_source_completion(uri, Position::new(1, 6))
            .is_none()
    );

    backend.set_is_tempest(true);
    backend.update_ast(uri, source);
    assert!(backend.is_php_template(uri));
    assert!(backend.template_virtual_php_arc(uri).is_some());
}

#[test]
fn only_views_and_the_files_that_feed_them_refresh_inference() {
    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| crate::util::path_to_uri(&dir.path().join(name));
    let (view, caller, other) = (path("shop.view.php"), path("caller.php"), path("other.php"));
    let backend = crate::Backend::new_test();
    std::fs::write(dir.path().join("shop.view.php"), "{{ $count }}").unwrap();
    backend.update_ast(&view, "{{ $count }}");
    backend.update_ast(
        &caller,
        "<?php \\Tempest\\View\\view(__DIR__.'/shop.view.php', count: 3);",
    );
    backend.update_ast(&other, "<?php function unrelated(): int { return 1; }");
    backend.refresh_tempest_inference();
    assert!(backend.affects_tempest_inference(&view));
    assert!(backend.affects_tempest_inference(&caller));
    assert!(!backend.affects_tempest_inference(&other));

    // A caller that stops rendering the view still refreshes once, so the
    // data it supplied is withdrawn.
    backend.update_ast(&caller, "<?php echo 1;");
    assert!(backend.affects_tempest_inference(&caller));

    backend.set_is_tempest(false);
    assert!(!backend.affects_tempest_inference(&view));
    assert!(!backend.affects_tempest_inference(&caller));
}

#[test]
fn edits_reuse_inference_with_the_same_result_as_a_full_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let files = [
        (
            "composer.json",
            r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        ),
        (
            "app/Book.php",
            "<?php namespace App; class Book { public string $title; }",
        ),
        (
            "app/x-card.view.php",
            "<div><x-slot name=\"header\" /><?php $local = new \\App\\Book(); ?><x-slot /></div>",
        ),
        ("app/x-panel.view.php", "<p>{{ $item }}<x-slot /></p>"),
        (
            "app/page.view.php",
            "<x-card :entry=\"$book\"><x-slot name=\"header\">{{ $local->title }}</x-slot>{{ $entry }}</x-card>",
        ),
        (
            "app/Controller.php",
            "<?php namespace App; use function Tempest\\View\\view; function page(Book $b) { return view(__DIR__.'/page.view.php', book: $b); }",
        ),
    ];
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let (mappings, _) = crate::composer::parse_composer_json(&root);
    let backend = crate::Backend::new_test_with_workspace(root.clone(), mappings);
    let uri = |name: &str| crate::util::path_to_uri(&root.join(name));
    for (name, text) in &files[1..] {
        backend.update_ast(&uri(name), text);
    }
    let snapshot = || -> BTreeMap<String, Scope> {
        let scopes = backend.tempest.scopes.read();
        scopes
            .iter()
            .map(|(uri, scope)| (uri.clone(), (**scope).clone()))
            .collect()
    };
    let edit = |name: &str, text: &str| {
        let uri = uri(name);
        backend
            .open_files
            .write()
            .insert(uri.clone(), std::sync::Arc::new(text.to_owned()));
        backend.update_ast(&uri, text);
        assert!(backend.affects_tempest_inference(&uri), "{name}");
        backend.refresh_tempest_inference_after_edit(&uri);
        let incremental = snapshot();
        backend.refresh_tempest_inference();
        assert_eq!(incremental, snapshot(), "after editing {name} to {text}");
        incremental
    };
    backend.refresh_tempest_inference();
    let page = &uri("app/page.view.php");
    assert!(!snapshot()[page].regions.is_empty());

    let edits = [
        // Shifts every slot offset in the page.
        (
            "app/page.view.php",
            "<h1>Books</h1>\n<x-card :entry=\"$book\"><x-slot name=\"header\">{{ $local->title }}</x-slot>{{ $entry }}</x-card>",
        ),
        // A component nested in a slot reads the slot's scope.
        (
            "app/page.view.php",
            "<x-card :entry=\"$book\"><x-panel :item=\"$local\">{{ $item->title }}</x-panel></x-card>",
        ),
        (
            "app/page.view.php",
            "<x-card :entry=\"$book->title\"><x-panel :item=\"$entry\" /></x-card>",
        ),
        // The component's outlets and the locals they expose change.
        (
            "app/x-card.view.php",
            "<div><?php $local = 'text'; ?><x-slot /><x-slot name=\"footer\" /></div>",
        ),
        (
            "app/Controller.php",
            "<?php namespace App; use function Tempest\\View\\view; function page(Book $b) { return view(__DIR__.'/page.view.php', book: [$b]); }",
        ),
        (
            "app/Controller.php",
            "<?php namespace App; function page() {}",
        ),
    ];
    for (name, text) in edits {
        edit(name, text);
    }

    // Closing a view discards its buffer for the text on disk.
    backend.open_files.write().remove(page);
    let on_disk = std::fs::read_to_string(root.join("app/page.view.php")).unwrap();
    backend.update_ast(page, &on_disk);
    backend.refresh_tempest_inference_after_edit(page);
    let incremental = snapshot();
    backend.refresh_tempest_inference();
    assert_eq!(incremental, snapshot());
}
