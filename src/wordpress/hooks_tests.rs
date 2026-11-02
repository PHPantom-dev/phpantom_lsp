use super::*;

#[test]
fn a_literal_name_matches_a_pattern_by_its_literal_parts() {
    assert!(hook_names_match("save_post_{$type}", "save_post_product"));
    assert!(hook_names_match("save_post_product", "save_post_{$type}"));
    assert!(hook_names_match(
        "{$taxonomy}_edit_form",
        "category_edit_form"
    ));
    assert!(hook_names_match("{$a}_x_{$b}", "one_x_two"));
    assert!(hook_names_match("{$a}_x_{$b}", "one_x_x_two"));
    // A computed part stands for at least one character.
    assert!(!hook_names_match("save_post_{$type}", "save_post_"));
    assert!(!hook_names_match("{$a}_x_{$b}", "_x_two"));
    assert!(!hook_names_match("save_post_{$type}", "save_page_product"));
    assert!(!hook_names_match(
        "{$taxonomy}_edit_form",
        "category_edit_forms"
    ));
}

#[test]
fn two_patterns_match_when_their_literal_parts_are_the_same() {
    assert!(hook_names_match(
        "save_post_{$post->post_type}",
        "save_post_{$type}"
    ));
    assert!(!hook_names_match("save_post_{$type}", "save_{$type}"));
    assert!(hook_names_match("acme", "acme"));
    assert!(!hook_names_match("acme", "Acme"));
}

#[test]
fn a_pattern_completes_as_a_snippet_named_after_its_computed_parts() {
    assert_eq!(
        pattern_snippet("save_post_{$post->post_type}"),
        "save_post_${1:post_type}"
    );
    assert_eq!(
        pattern_snippet("{$taxonomy}_{get_post_type()}"),
        "${1:taxonomy}_${2:get_post_type}"
    );
    assert_eq!(pattern_snippet("a$b_{$c['']}"), "a\\$b_${1:c}");
}

#[test]
fn a_hook_name_is_written_back_only_without_quotes_or_interpolation() {
    assert!(is_valid_hook_name("acme/order-placed.v2"));
    assert!(!is_valid_hook_name(""));
    assert!(!is_valid_hook_name("it's"));
    assert!(!is_valid_hook_name("a{$b}"));
    assert!(!is_valid_hook_name("a\\b"));
}

#[test]
fn only_a_global_function_call_is_one_of_the_hook_functions() {
    assert_eq!(written_function("    add_action"), Some("add_action"));
    assert_eq!(written_function("<?php\nadd_action"), Some("add_action"));
    assert_eq!(written_function("return add_action"), Some("add_action"));
    assert_eq!(written_function("x = \\add_action"), Some("add_action"));
    assert_eq!(written_function("$this->add_action"), None);
    assert_eq!(written_function("Hooks::add_action"), None);
    assert_eq!(written_function("Acme\\add_action"), None);
    assert_eq!(written_function("function add_action"), None);
}

/// The hook spans of `php`: the name, the text the span covers, and
/// whether it fires the hook.
fn hooks_of(php: &str) -> Vec<(String, String, bool)> {
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(b"test.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
    let map = crate::symbol_map::extraction::extract_symbol_map(program, php);
    hook_spans(&map)
        .map(|span| {
            (
                span.key.to_string(),
                php[span.start as usize..span.end as usize].to_string(),
                span.fires,
            )
        })
        .collect()
}

#[test]
fn hook_functions_record_the_hook_they_name() {
    let php = r#"<?php
namespace Acme;
do_action('acme_placed', $order);
\apply_filters_ref_array("acme_total", [$total]);
add_action(hook_name: 'acme_placed', callback: 'cb');
do_action("save_post_{$post->post_type}", $post);
do_action("{$taxonomy}_edit_form");
do_action('acme_status_' . $order->status . '_done');
do_action("acme_$step");
do_action($dynamic);
do_action("{$dynamic}");
do_action("{$old}_{$new}");
do_action("{$old}_to_{$new}");
do_action('braces{}');
Other\do_action('qualified');
$hooks->add_action('method');
"#;
    assert_eq!(
        hooks_of(php),
        [
            ("acme_placed".into(), "acme_placed".into(), true),
            ("acme_total".into(), "acme_total".into(), true),
            ("acme_placed".into(), "acme_placed".into(), false),
            (
                "save_post_{$post->post_type}".into(),
                "save_post_".into(),
                true
            ),
            ("{$taxonomy}_edit_form".into(), "_edit_form".into(), true),
            (
                "acme_status_{$order->status}_done".into(),
                "acme_status_".into(),
                true
            ),
            ("acme_{$step}".into(), "acme_".into(), true),
            ("{$old}_to_{$new}".into(), "_to_".into(), true),
        ]
    );
}
