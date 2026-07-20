use super::*;
use crate::php_type::PhpType;

#[test]
fn extract_description_simple() {
    let doc = "/** This is a simple description. */";
    assert_eq!(
        extract_docblock_description(Some(doc)),
        Some("This is a simple description.".to_string())
    );
}

#[test]
fn extract_description_multiline() {
    let doc = "/**\n * First line.\n * Second line.\n * @param string $x\n */";
    assert_eq!(
        extract_docblock_description(Some(doc)),
        Some("First line.\nSecond line.".to_string())
    );
}

#[test]
fn extract_description_none_when_only_tags() {
    let doc = "/**\n * @return string\n */";
    assert_eq!(extract_docblock_description(Some(doc)), None);
}

#[test]
fn extract_description_none_when_empty() {
    assert_eq!(extract_docblock_description(None), None);
}

#[test]
fn namespace_line_with_namespace() {
    assert_eq!(
        namespace_line(Some("App\\Models")),
        "namespace App\\Models;\n"
    );
}

#[test]
fn namespace_line_without_namespace() {
    assert_eq!(namespace_line(None), "");
}

#[test]
fn format_params_empty() {
    assert_eq!(format_native_params(&[]), "");
}

#[test]
fn format_params_with_types() {
    let params = vec![
        ParameterInfo {
            name: crate::atom::atom("$name"),
            type_hint: Some(PhpType::parse("string")),
            native_type_hint: Some(PhpType::parse("string")),
            description: None,
            default_value: None,
            is_required: true,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
        ParameterInfo {
            name: crate::atom::atom("$age"),
            type_hint: Some(PhpType::parse("int")),
            native_type_hint: Some(PhpType::parse("int")),
            description: None,
            default_value: None,
            is_required: false,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
    ];
    assert_eq!(
        format_native_params(&params),
        "string $name, int $age = ..."
    );
}

#[test]
fn format_params_variadic() {
    let params = vec![ParameterInfo {
        name: crate::atom::atom("$items"),
        type_hint: Some(PhpType::parse("string")),
        native_type_hint: Some(PhpType::parse("string")),
        description: None,
        default_value: None,
        is_required: false,
        is_variadic: true,
        is_reference: false,
        closure_this_type: None,
    }];
    assert_eq!(format_native_params(&params), "string ...$items");
}

#[test]
fn format_params_reference() {
    let params = vec![ParameterInfo {
        name: crate::atom::atom("$arr"),
        type_hint: Some(PhpType::parse("array")),
        native_type_hint: Some(PhpType::parse("array")),
        description: None,
        default_value: None,
        is_required: true,
        is_variadic: false,
        is_reference: true,
        closure_this_type: None,
    }];
    assert_eq!(format_native_params(&params), "array &$arr");
}

#[test]
fn format_visibility_all() {
    assert_eq!(format_visibility(Visibility::Public), "public ");
    assert_eq!(format_visibility(Visibility::Protected), "protected ");
    assert_eq!(format_visibility(Visibility::Private), "private ");
}

// ─── short_name tests ───────────────────────────────────────────────────────

#[test]
fn short_name_plain() {
    assert_eq!(short_name("User"), "User");
}

#[test]
fn short_name_namespaced() {
    assert_eq!(short_name("App\\Models\\User"), "User");
}

#[test]
fn short_name_leading_backslash() {
    assert_eq!(short_name("\\App\\Models\\User"), "User");
}

#[test]
fn short_name_scalar() {
    assert_eq!(short_name("string"), "string");
}

#[test]
fn short_name_single_namespace() {
    assert_eq!(short_name("Demo\\Brush"), "Brush");
}

// ─── shorten_type_string tests ──────────────────────────────────────────────

#[test]
fn shorten_type_string_plain_class() {
    assert_eq!(shorten_type_string("App\\Models\\User"), "User");
}

#[test]
fn shorten_type_string_already_short() {
    assert_eq!(shorten_type_string("User"), "User");
}

#[test]
fn shorten_type_string_scalar() {
    assert_eq!(shorten_type_string("string"), "string");
}

#[test]
fn shorten_type_string_nullable() {
    assert_eq!(shorten_type_string("?App\\Models\\User"), "?User");
}

#[test]
fn shorten_type_string_union() {
    assert_eq!(shorten_type_string("App\\Models\\User|null"), "User|null");
}

#[test]
fn shorten_type_string_generic() {
    assert_eq!(shorten_type_string("list<App\\Models\\User>"), "list<User>");
}

#[test]
fn shorten_type_string_nested_generic() {
    assert_eq!(
        shorten_type_string("array<int, App\\Collection<string, App\\Models\\User>>"),
        "array<int, Collection<string, User>>"
    );
}

#[test]
fn shorten_type_string_intersection() {
    assert_eq!(
        shorten_type_string("App\\Countable&App\\Traversable"),
        "Countable&Traversable"
    );
}

#[test]
fn shorten_type_string_leading_backslash() {
    assert_eq!(shorten_type_string("\\App\\Models\\User"), "User");
}

#[test]
fn shorten_type_string_object_shape() {
    assert_eq!(
        shorten_type_string("object{name: string, user: App\\Models\\User}"),
        "object{name: string, user: User}"
    );
}

#[test]
fn shorten_type_string_mixed_union_with_generics() {
    assert_eq!(
        shorten_type_string("App\\Collection<int, App\\Models\\User>|null"),
        "Collection<int, User>|null"
    );
}

#[test]
fn shorten_type_string_parenthesized_callable_union() {
    assert_eq!(
        shorten_type_string(
            "(\\Closure(static): mixed)|string|array|\\Illuminate\\Contracts\\Database\\Query\\Expression"
        ),
        "(Closure(static): mixed)|string|array|Expression"
    );
}

// ─── build_variable_hover_body tests ────────────────────────────────────────

#[test]
fn variable_hover_body_single_type() {
    let ty = PhpType::parse("User");
    let body = build_variable_hover_body("$user", &ty, &|_| None, None);
    assert_eq!(body, "```php\n<?php\n$user = User\n```");
}

#[test]
fn variable_hover_body_union_splits_into_blocks() {
    let ty = PhpType::parse("Lamp|Faucet");
    let body = build_variable_hover_body("$ambiguous", &ty, &|_| None, None);
    assert!(body.contains("$ambiguous = Lamp"), "got: {}", body);
    assert!(body.contains("---"), "got: {}", body);
    assert!(body.contains("$ambiguous = Faucet"), "got: {}", body);
}

#[test]
fn variable_hover_body_union_with_template_line() {
    let ty = PhpType::parse("Lamp|Faucet");
    let body = build_variable_hover_body("$item", &ty, &|_| None, Some("**template** `T`"));
    assert!(body.starts_with("**template** `T`\n\n"));
    assert!(body.contains("$item = Lamp"));
    assert!(body.contains("---"));
    assert!(body.contains("$item = Faucet"));
}

#[test]
fn variable_hover_body_generic_union_not_split() {
    // A single generic type is not split even though it contains `|` inside `<>`.
    let ty = PhpType::parse("Generator<int, Foo>");
    let body = build_variable_hover_body("$gen", &ty, &|_| None, None);
    assert!(!body.contains("---"), "got: {}", body);
    assert!(body.contains("Generator<int, Foo>"), "got: {}", body);
}

#[test]
fn variable_hover_body_three_way_union() {
    let ty = PhpType::parse("A|B|C");
    let body = build_variable_hover_body("$x", &ty, &|_| None, None);
    let blocks: Vec<&str> = body.split("\n\n---\n\n").collect();
    assert_eq!(blocks.len(), 3);
    assert!(blocks[0].contains("$x = A"));
    assert!(blocks[1].contains("$x = B"));
    assert!(blocks[2].contains("$x = C"));
}

#[test]
fn variable_hover_body_nullable_class_not_split() {
    // `Foo|null` has only one class-like type, so it should stay in a single block.
    let ty = PhpType::parse("Foo|null");
    let body = build_variable_hover_body("$x", &ty, &|_| None, None);
    assert!(!body.contains("---"), "Foo|null should not split: {}", body);
    assert!(body.contains("$x = Foo|null"), "got: {}", body);
}

#[test]
fn variable_hover_body_scalar_not_split() {
    let ty = PhpType::parse("string");
    let body = build_variable_hover_body("$val", &ty, &|_| None, None);
    assert!(!body.contains("---"));
    assert!(body.contains("$val = string"));
}

// ─── extract_constant_value_from_source tests ───────────────────────────────

#[test]
fn extract_constant_value_simple_define() {
    let source = "define('MY_CONST', 42);";
    assert_eq!(
        extract_constant_value_from_source("MY_CONST", source),
        Some("42".to_string())
    );
}

#[test]
fn extract_constant_value_string_define() {
    let source = "define('BASE_PATH', '/var/www');";
    assert_eq!(
        extract_constant_value_from_source("BASE_PATH", source),
        Some("'/var/www'".to_string())
    );
}

#[test]
fn extract_constant_value_strips_third_arg_true() {
    let source = "define('__DIR__', '', true);";
    assert_eq!(
        extract_constant_value_from_source("__DIR__", source),
        Some("string".to_string())
    );
}

#[test]
fn extract_constant_value_strips_third_arg_false() {
    let source = "define('__FILE__', \"\", false);";
    assert_eq!(
        extract_constant_value_from_source("__FILE__", source),
        Some("string".to_string())
    );
}

#[test]
fn extract_constant_value_third_arg_with_nonempty_value() {
    let source = "define('FOO', 123, true);";
    assert_eq!(
        extract_constant_value_from_source("FOO", source),
        Some("123".to_string())
    );
}

#[test]
fn extract_constant_value_empty_single_quoted_string() {
    let source = "define('EMPTY_CONST', '');";
    assert_eq!(
        extract_constant_value_from_source("EMPTY_CONST", source),
        Some("string".to_string())
    );
}

#[test]
fn extract_constant_value_empty_double_quoted_string() {
    let source = "define('EMPTY_CONST', \"\");";
    assert_eq!(
        extract_constant_value_from_source("EMPTY_CONST", source),
        Some("string".to_string())
    );
}

#[test]
fn extract_constant_value_no_third_arg_not_stripped() {
    let source = "define('NORMAL', 'hello');";
    assert_eq!(
        extract_constant_value_from_source("NORMAL", source),
        Some("'hello'".to_string())
    );
}

#[test]
fn extract_constant_value_const_syntax() {
    let source = "const MY_CONST = 99;";
    assert_eq!(
        extract_constant_value_from_source("MY_CONST", source),
        Some("99".to_string())
    );
}

#[test]
fn extract_constant_value_not_found() {
    let source = "define('OTHER', 1);";
    assert_eq!(extract_constant_value_from_source("MISSING", source), None);
}

#[test]
fn extract_constant_value_comma_inside_string_not_confused() {
    let source = "define('MSG', 'hello, world', true);";
    assert_eq!(
        extract_constant_value_from_source("MSG", source),
        Some("'hello, world'".to_string())
    );
}

// ── html_to_markdown ────────────────────────────────────────────────

#[test]
fn html_to_markdown_bold_and_italic() {
    assert_eq!(
        formatting::html_to_markdown("<b>bold</b> and <i>italic</i>"),
        "**bold** and *italic*"
    );
}

#[test]
fn html_to_markdown_strong_and_em() {
    assert_eq!(
        formatting::html_to_markdown("<strong>bold</strong> and <em>italic</em>"),
        "**bold** and *italic*"
    );
}

#[test]
fn html_to_markdown_code() {
    assert_eq!(
        formatting::html_to_markdown("use <code>foo()</code> instead"),
        "use `foo()` instead"
    );
}

#[test]
fn html_to_markdown_br_variants() {
    assert_eq!(formatting::html_to_markdown("a<br>b"), "a\nb");
    assert_eq!(formatting::html_to_markdown("a<br/>b"), "a\nb");
    assert_eq!(formatting::html_to_markdown("a<br />b"), "a\nb");
}

#[test]
fn html_to_markdown_paragraph() {
    assert_eq!(
        formatting::html_to_markdown("first<p>second</p>"),
        "first\n\nsecond"
    );
}

#[test]
fn html_to_markdown_unordered_list() {
    let input = "Values:<ul><li>one</li><li>two</li><li>three</li></ul>done";
    let expected = "Values:\n- one\n- two\n- three\n\ndone";
    assert_eq!(formatting::html_to_markdown(input), expected);
}

#[test]
fn html_to_markdown_ordered_list() {
    let input = "Steps:<ol><li>first</li><li>second</li></ol>";
    let expected = "Steps:\n- first\n- second\n\n";
    assert_eq!(formatting::html_to_markdown(input), expected);
}

#[test]
fn html_to_markdown_definition_list() {
    assert_eq!(
        formatting::html_to_markdown("<dl><dt>Term</dt><dd>Definition</dd></dl>"),
        "\n\n**Term**\n  Definition\n"
    );
}

#[test]
fn html_to_markdown_span_stripped() {
    assert_eq!(formatting::html_to_markdown("<span>text</span>"), "text");
}

#[test]
fn html_to_markdown_no_html() {
    let plain = "No HTML here.";
    assert_eq!(formatting::html_to_markdown(plain), plain);
}
