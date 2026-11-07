use super::{
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
