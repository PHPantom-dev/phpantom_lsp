//! Unit tests for [`super::namespace_at_offset`], [`super::NamespaceDecls`]
//! and the top-level scanners.

use super::{
    NamespaceDecls, ScanStep, find_semicolon_balanced, is_whole_string_literal,
    namespace_at_offset, namespace_decl_at, scan_top_level_backward, split_top_level_commas,
    top_level_commas, unquote_php_string,
};

/// Return the namespace in force where `|` sits in `src` (the marker is
/// stripped before scanning).
///
/// Also checks that the precomputed index agrees with the backward scan at
/// every offset of the fixture, not just the marked one.
fn ns_at_marker(src: &str) -> Option<String> {
    let offset = src.find('|').expect("fixture needs a `|` cursor marker");
    let stripped = src.replace('|', "");
    let decls = NamespaceDecls::scan(&stripped);
    for at in 0..=stripped.len() + 1 {
        assert_eq!(
            decls.at(&stripped, at),
            namespace_at_offset(&stripped, at),
            "index and scan disagree at offset {at}"
        );
        assert_eq!(
            decls.decl_at(at),
            namespace_decl_at(&stripped, at),
            "index and scan disagree on the declaration at offset {at}"
        );
    }
    namespace_at_offset(&stripped, offset).map(str::to_string)
}

#[test]
fn semicolon_form_applies_to_the_rest_of_the_file() {
    assert_eq!(
        ns_at_marker("<?php\nnamespace App\\Models;\n\nfunction test() { | }\n").as_deref(),
        Some("App\\Models")
    );
}

#[test]
fn opening_tag_on_the_same_line_still_matches() {
    assert_eq!(
        ns_at_marker("<?php namespace App; function test() { | }").as_deref(),
        Some("App")
    );
}

#[test]
fn file_without_a_declaration_is_global() {
    assert_eq!(ns_at_marker("<?php\nfunction test() { | }\n"), None);
}

#[test]
fn code_before_the_declaration_is_global() {
    assert_eq!(ns_at_marker("<?php\n| \nnamespace App;\n"), None);
}

#[test]
fn braced_blocks_each_win_inside_their_own_body() {
    let src = "<?php\nnamespace {\n    class Aborter {}\n}\n\nnamespace App {\n    function test() { | }\n}\n";
    assert_eq!(ns_at_marker(src).as_deref(), Some("App"));
}

#[test]
fn an_explicit_global_block_after_a_named_one_is_global() {
    let src = "<?php\nnamespace App {\n    class Foo {}\n}\n\nnamespace {\n    function test() { | }\n}\n";
    assert_eq!(ns_at_marker(src), None);
}

#[test]
fn the_keyword_in_a_comment_is_not_a_declaration() {
    let src =
        "<?php\nnamespace App;\n// namespace Other;\n/* namespace Third; */\nfunction t() { | }\n";
    assert_eq!(ns_at_marker(src).as_deref(), Some("App"));
}

#[test]
fn a_line_comment_above_a_block_does_not_hide_it() {
    let src = "<?php\n// Test: first\nnamespace A {\n}\n\n// Test: second http://x\nnamespace B {\n    function t() { | }\n}\n";
    assert_eq!(ns_at_marker(src).as_deref(), Some("B"));
}

#[test]
fn a_hash_comment_after_code_does_not_hide_the_declaration() {
    let src = "<?php\nnamespace A {\n} # end of A\nnamespace B {\n    function t() { | }\n}\n";
    assert_eq!(ns_at_marker(src).as_deref(), Some("B"));
}

#[test]
fn a_block_comment_above_the_declaration_does_not_hide_it() {
    let src = "<?php\n/**\n * File header.\n */\nnamespace App;\nfunction t() { | }\n";
    assert_eq!(ns_at_marker(src).as_deref(), Some("App"));
}

#[test]
fn the_keyword_on_its_own_line_inside_a_block_comment_is_not_a_declaration() {
    let src = "<?php\nnamespace App;\n/* about\nnamespace Other;\n*/\nfunction t() { | }\n";
    assert_eq!(ns_at_marker(src).as_deref(), Some("App"));
}

#[test]
fn the_keyword_in_a_string_is_not_a_declaration() {
    let src = "<?php\nnamespace App;\n$s = \"namespace Other;\";\nfunction t() { | }\n";
    assert_eq!(ns_at_marker(src).as_deref(), Some("App"));
}

#[test]
fn the_namespace_operator_is_not_a_declaration() {
    let src = "<?php\nnamespace App;\n$x = namespace\\helper();\nfunction t() { | }\n";
    assert_eq!(ns_at_marker(src).as_deref(), Some("App"));
}

#[test]
fn an_identifier_starting_with_the_keyword_is_not_a_declaration() {
    let src = "<?php\nnamespace App;\n$namespaced = 1;\nfunction t() { | }\n";
    assert_eq!(ns_at_marker(src).as_deref(), Some("App"));
}

#[test]
fn two_blocks_declaring_the_same_namespace_are_told_apart() {
    let src = "<?php\nnamespace {\n    f();\n}\nnamespace {\n    f();\n}\n";
    let first = src.find("f()").unwrap();
    let second = src.rfind("f()").unwrap();
    assert_eq!(namespace_decl_at(src, 0), None);
    assert_eq!(namespace_decl_at(src, first), Some(6));
    assert_eq!(
        namespace_decl_at(src, second),
        Some(src.rfind("namespace").unwrap())
    );
}

#[test]
fn top_level_commas_split_entries() {
    assert_eq!(split_top_level_commas("a, b, c"), vec!["a", " b", " c"]);
    assert_eq!(
        split_top_level_commas("foo(bar, baz), qux"),
        vec!["foo(bar, baz)", " qux"]
    );
    assert_eq!(
        split_top_level_commas("fn() => match ($x) { 1 => 'a', 2 => 'b' }, $y"),
        vec!["fn() => match ($x) { 1 => 'a', 2 => 'b' }", " $y"]
    );
}

#[test]
fn commas_in_strings_do_not_split() {
    assert_eq!(
        split_top_level_commas("'1,234,567.89', true"),
        vec!["'1,234,567.89'", " true"]
    );
    assert_eq!(
        split_top_level_commas("\"hello, world\", 42"),
        vec!["\"hello, world\"", " 42"]
    );
    assert_eq!(
        split_top_level_commas(r#""a,\"b,c\",d", 'x,\'y,z\'', foo)"#),
        vec![r#""a,\"b,c\",d""#, " 'x,\\'y,z\\''", " foo)"]
    );
    assert_eq!(
        split_top_level_commas(r#"array('a,b', ["x,y"]), "foo,bar""#),
        vec![r#"array('a,b', ["x,y"])"#, r#" "foo,bar""#]
    );
    assert_eq!(
        split_top_level_commas(r#"'foo\,bar', "baz\"qux", plain"#),
        vec![r#"'foo\,bar'"#, r#" "baz\"qux""#, " plain"]
    );
}

/// A comma inside a comment ends nothing, so the argument after it is
/// still the one the caller asked for by index.
#[test]
fn commas_in_comments_do_not_split() {
    assert_eq!(
        split_top_level_commas("'view', // one, two\n['a' => 1]"),
        vec!["'view'", " // one, two\n['a' => 1]"]
    );
    assert_eq!(
        split_top_level_commas("'view' /* one, two */, ['a' => 1]"),
        vec!["'view' /* one, two */", " ['a' => 1]"]
    );
}

/// The statement ends at its own `;`, not at one in a comment, a string or
/// a nested body.
#[test]
fn the_statement_semicolon_skips_comments_strings_and_brackets() {
    let text = " $a // why; not\n    ?? fn () => { $b; } ?? ';' ?? f(';');\nnext();";
    assert_eq!(find_semicolon_balanced(text), text.find(";\n"));
    assert_eq!(find_semicolon_balanced(" $a /* ; */"), None);
}

#[test]
fn a_trailing_comma_and_blank_text_leave_no_empty_entry() {
    assert_eq!(split_top_level_commas("$a, $b, "), vec!["$a", " $b"]);
    assert_eq!(split_top_level_commas("   "), Vec::<&str>::new());
    assert_eq!(split_top_level_commas(""), Vec::<&str>::new());
    assert_eq!(top_level_commas("$a, $b, ").count(), 2);
}

/// The opening bracket the end of `text` sits in, as the backward scan
/// finds it.
fn enclosing_open(text: &str) -> Option<usize> {
    scan_top_level_backward(text.as_bytes(), |bytes, i| match bytes[i] {
        b'(' | b'[' | b'{' => ScanStep::Stop,
        b';' => ScanStep::Abort,
        _ => ScanStep::Skip(1),
    })
}

#[test]
fn backward_scan_steps_over_closed_pairs_and_strings() {
    assert_eq!(enclosing_open("f($a, [1, 2], g(3), "), Some(1));
    assert_eq!(enclosing_open("f('(', \"[\", 'it\\'s ', "), Some(1));
    assert_eq!(enclosing_open("$x = [fn () => h(1), "), Some(5));
}

#[test]
fn backward_scan_stops_at_what_the_visitor_rules_out() {
    assert_eq!(enclosing_open("done(); $x, "), None);
    assert_eq!(enclosing_open("no bracket at all"), None);
    assert_eq!(enclosing_open("f(';', "), Some(1));
}

#[test]
fn a_whole_string_literal_is_one_quoted_string() {
    assert!(is_whole_string_literal("'a'"));
    assert!(is_whole_string_literal("\"a \\\" b\""));
    assert!(is_whole_string_literal("'a\\\\'"));
    assert!(is_whole_string_literal("''"));
    assert!(!is_whole_string_literal("'a' === 'b'"));
    assert!(!is_whole_string_literal("'a' . 'b'"));
    assert!(!is_whole_string_literal("'a\\'"));
    assert!(!is_whole_string_literal("'a\""));
    assert!(!is_whole_string_literal("a"));
    assert!(!is_whole_string_literal("'"));
}

#[test]
fn unquoting_takes_only_a_whole_literal() {
    assert_eq!(unquote_php_string("'key'"), Some("key"));
    assert_eq!(unquote_php_string("\"key\""), Some("key"));
    assert_eq!(unquote_php_string("'a' . 'b'"), None);
    assert_eq!(unquote_php_string("key"), None);
}
