//! Unit tests for [`super::namespace_at_offset`].

use super::namespace_at_offset;

/// Return the namespace in force where `|` sits in `src` (the marker is
/// stripped before scanning).
fn ns_at_marker(src: &str) -> Option<String> {
    let offset = src.find('|').expect("fixture needs a `|` cursor marker");
    let stripped = src.replace('|', "");
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
