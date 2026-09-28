//! Tests for the named-argument completion helpers in
//! `src/completion/named_args.rs`: locating the enclosing call
//! expression, parsing already-typed arguments, and building the
//! resulting completion items.

use phpantom_lsp::atom::atom;
use phpantom_lsp::completion::named_args::*;
use phpantom_lsp::php_type::PhpType;
use phpantom_lsp::types::ParameterInfo;
use tower_lsp::lsp_types::*;

// ── position_to_char_offset ─────────────────────────────────────

#[test]
fn char_offset_first_line() {
    let content = "<?php\nfoo()\n";
    let chars: Vec<char> = content.chars().collect();
    let pos = Position {
        line: 1,
        character: 3,
    };
    // "foo" starts at offset 6 (after "<?php\n"), character 3 = '('
    assert_eq!(position_to_char_offset(&chars, pos), Some(9));
}

#[test]
fn char_offset_end_of_line() {
    let content = "<?php\nfoo()\n";
    let chars: Vec<char> = content.chars().collect();
    let pos = Position {
        line: 1,
        character: 5,
    };
    assert_eq!(position_to_char_offset(&chars, pos), Some(11));
}

// ── find_enclosing_open_paren ───────────────────────────────────

#[test]
fn finds_open_paren_simple() {
    let chars: Vec<char> = "foo(".chars().collect();
    assert_eq!(find_enclosing_open_paren(&chars, 4), Some(3));
}

#[test]
fn finds_open_paren_with_args() {
    let chars: Vec<char> = "foo($x, ".chars().collect();
    assert_eq!(find_enclosing_open_paren(&chars, 8), Some(3));
}

#[test]
fn skips_nested_parens() {
    let chars: Vec<char> = "foo(bar(1), ".chars().collect();
    assert_eq!(find_enclosing_open_paren(&chars, 12), Some(3));
}

#[test]
fn none_outside_parens() {
    let chars: Vec<char> = "foo();".chars().collect();
    // After the `)` and `;`
    assert_eq!(find_enclosing_open_paren(&chars, 6), None);
}

#[test]
fn stops_at_semicolon() {
    let chars: Vec<char> = "$x = 1; foo(".chars().collect();
    // Searching from after `foo(`, should find `(` at position 11
    assert_eq!(find_enclosing_open_paren(&chars, 12), Some(11));
}

#[test]
fn skips_single_quoted_string() {
    let chars: Vec<char> = "foo('(', ".chars().collect();
    assert_eq!(find_enclosing_open_paren(&chars, 9), Some(3));
}

#[test]
fn skips_double_quoted_string() {
    let chars: Vec<char> = "foo(\"(\", ".chars().collect();
    assert_eq!(find_enclosing_open_paren(&chars, 9), Some(3));
}

// ── extract_call_expression ─────────────────────────────────────

#[test]
fn call_expr_standalone_function() {
    let chars: Vec<char> = "foo(".chars().collect();
    assert_eq!(extract_call_expression(&chars, 3), Some("foo".to_string()));
}

#[test]
fn call_expr_namespaced_function() {
    let chars: Vec<char> = "App\\Helper\\foo(".chars().collect();
    assert_eq!(
        extract_call_expression(&chars, 14),
        Some("App\\Helper\\foo".to_string())
    );
}

#[test]
fn call_expr_instance_method() {
    let chars: Vec<char> = "$this->method(".chars().collect();
    assert_eq!(
        extract_call_expression(&chars, 13),
        Some("$this->method".to_string())
    );
}

#[test]
fn call_expr_variable_method() {
    let chars: Vec<char> = "$service->handle(".chars().collect();
    assert_eq!(
        extract_call_expression(&chars, 16),
        Some("$service->handle".to_string())
    );
}

#[test]
fn call_expr_static_method() {
    let chars: Vec<char> = "Cache::get(".chars().collect();
    assert_eq!(
        extract_call_expression(&chars, 10),
        Some("Cache::get".to_string())
    );
}

#[test]
fn call_expr_self_method() {
    let chars: Vec<char> = "self::create(".chars().collect();
    assert_eq!(
        extract_call_expression(&chars, 12),
        Some("self::create".to_string())
    );
}

#[test]
fn call_expr_parent_method() {
    let chars: Vec<char> = "parent::__construct(".chars().collect();
    assert_eq!(
        extract_call_expression(&chars, 19),
        Some("parent::__construct".to_string())
    );
}

#[test]
fn call_expr_constructor() {
    let chars: Vec<char> = "new UserService(".chars().collect();
    assert_eq!(
        extract_call_expression(&chars, 15),
        Some("new UserService".to_string())
    );
}

#[test]
fn call_expr_constructor_with_extra_space() {
    let chars: Vec<char> = "new  Foo(".chars().collect();
    assert_eq!(
        extract_call_expression(&chars, 8),
        Some("new Foo".to_string())
    );
}

#[test]
fn call_expr_none_for_chained_call() {
    let chars: Vec<char> = "foo()->bar(".chars().collect();
    // The `(` at index 10 follows `bar`, but before `bar` is `)->` preceded by `)`
    // We don't support chained-call resolution for named args
    assert_eq!(extract_call_expression(&chars, 10), None);
}

// ── extract_named_arg_name ──────────────────────────────────────

#[test]
fn named_arg_simple() {
    assert_eq!(
        extract_named_arg_name("name: $value"),
        Some("name".to_string())
    );
}

#[test]
fn named_arg_with_whitespace() {
    assert_eq!(extract_named_arg_name("  age: 42"), Some("age".to_string()));
}

#[test]
fn positional_arg_variable() {
    assert_eq!(extract_named_arg_name("$value"), None);
}

#[test]
fn positional_arg_number() {
    assert_eq!(extract_named_arg_name("42"), None);
}

#[test]
fn not_named_arg_double_colon() {
    assert_eq!(extract_named_arg_name("Foo::class"), None);
}

#[test]
fn not_named_arg_string() {
    assert_eq!(extract_named_arg_name("'hello'"), None);
}

// ── parse_existing_args ─────────────────────────────────────────

#[test]
fn no_args() {
    let (named, pos) = parse_existing_args("");
    assert!(named.is_empty());
    assert_eq!(pos, 0);
}

#[test]
fn one_positional() {
    let (named, pos) = parse_existing_args("$x, ");
    assert!(named.is_empty());
    assert_eq!(pos, 1);
}

#[test]
fn two_positional() {
    let (named, pos) = parse_existing_args("$x, $y, ");
    assert!(named.is_empty());
    assert_eq!(pos, 2);
}

#[test]
fn one_named() {
    let (named, pos) = parse_existing_args("name: $x, ");
    assert_eq!(named, vec!["name"]);
    assert_eq!(pos, 0);
}

#[test]
fn mixed_positional_and_named() {
    let (named, pos) = parse_existing_args("$x, name: $y, ");
    assert_eq!(named, vec!["name"]);
    assert_eq!(pos, 1);
}

#[test]
fn multiple_named() {
    let (named, pos) = parse_existing_args("name: 'John', age: 30, ");
    assert_eq!(named, vec!["name", "age"]);
    assert_eq!(pos, 0);
}

#[test]
fn nested_call_in_arg() {
    let (named, pos) = parse_existing_args("getName($obj), ");
    assert!(named.is_empty());
    assert_eq!(pos, 1);
}

// ── detect_named_arg_context ────────────────────────────────────

#[test]
fn context_simple_function() {
    let content = "<?php\nfoo(";
    let pos = Position {
        line: 1,
        character: 4,
    };
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_some(), "Should detect context inside foo(");
    let ctx = ctx.unwrap();
    assert_eq!(ctx.call_expression, "foo");
    assert!(ctx.existing_named_args.is_empty());
    assert_eq!(ctx.positional_count, 0);
    assert_eq!(ctx.prefix, "");
}

#[test]
fn context_with_prefix() {
    let content = "<?php\nfoo(na";
    let pos = Position {
        line: 1,
        character: 6,
    };
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.call_expression, "foo");
    assert_eq!(ctx.prefix, "na");
}

#[test]
fn context_after_positional() {
    let content = "<?php\nfoo($x, ";
    let pos = Position {
        line: 1,
        character: 8,
    };
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.call_expression, "foo");
    assert_eq!(ctx.positional_count, 1);
}

#[test]
fn context_after_named_arg() {
    let content = "<?php\nfoo(name: $x, ";
    let pos = Position {
        line: 1,
        character: 15,
    };
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.call_expression, "foo");
    assert_eq!(ctx.existing_named_args, vec!["name"]);
    assert_eq!(ctx.positional_count, 0);
}

#[test]
fn context_method_call() {
    let content = "<?php\n$this->method(";
    let pos = Position {
        line: 1,
        character: 16,
    };
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_some());
    assert_eq!(ctx.unwrap().call_expression, "$this->method");
}

#[test]
fn context_static_call() {
    let content = "<?php\nCache::get(";
    let pos = Position {
        line: 1,
        character: 11,
    };
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_some());
    assert_eq!(ctx.unwrap().call_expression, "Cache::get");
}

#[test]
fn context_constructor() {
    let content = "<?php\nnew Foo(";
    let pos = Position {
        line: 1,
        character: 8,
    };
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_some());
    assert_eq!(ctx.unwrap().call_expression, "new Foo");
}

#[test]
fn no_context_typing_variable() {
    let content = "<?php\nfoo($va";
    let pos = Position {
        line: 1,
        character: 7,
    };
    // Preceded by `$` — should return None
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_none(), "Should not trigger for variable names");
}

#[test]
fn no_context_after_arrow() {
    let content = "<?php\nfoo($this->pr";
    let pos = Position {
        line: 1,
        character: 14,
    };
    // Preceded by `->` — should return None
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_none(), "Should not trigger after ->");
}

#[test]
fn no_context_outside_parens() {
    let content = "<?php\nfoo();";
    let pos = Position {
        line: 1,
        character: 6,
    };
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_none(), "Should not trigger outside parens");
}

#[test]
fn context_multiline() {
    let content = "<?php\nfoo(\n    $x,\n    ";
    let pos = Position {
        line: 3,
        character: 4,
    };
    let ctx = detect_named_arg_context(content, pos);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.call_expression, "foo");
    assert_eq!(ctx.positional_count, 1);
}

// ── build_named_arg_completions ─────────────────────────────────

fn make_param(name: &str, type_hint: Option<&str>, required: bool) -> ParameterInfo {
    ParameterInfo {
        name: atom(&format!("${}", name)),
        is_required: required,
        type_hint: type_hint.map(PhpType::parse),
        native_type_hint: type_hint.map(PhpType::parse),
        description: None,
        default_value: None,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
        param_out_type: None,
    }
}

#[test]
fn completions_all_params() {
    let params = vec![
        make_param("name", Some("string"), true),
        make_param("age", Some("int"), true),
    ];
    let ctx = NamedArgContext {
        call_expression: "foo".to_string(),
        existing_named_args: vec![],
        positional_count: 0,
        prefix: String::new(),
    };

    let items = build_named_arg_completions(&ctx, &params);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].label, "name: string");
    assert_eq!(items[0].insert_text.as_deref(), Some("name: "));
    assert_eq!(items[0].filter_text.as_deref(), Some("name"));
    assert_eq!(items[1].label, "age: int");
    assert_eq!(items[1].insert_text.as_deref(), Some("age: "));
}

#[test]
fn completions_skip_positional() {
    let params = vec![
        make_param("name", Some("string"), true),
        make_param("age", Some("int"), true),
        make_param("email", Some("string"), false),
    ];
    let ctx = NamedArgContext {
        call_expression: "foo".to_string(),
        existing_named_args: vec![],
        positional_count: 1, // first param covered by positional
        prefix: String::new(),
    };

    let items = build_named_arg_completions(&ctx, &params);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].filter_text.as_deref(), Some("age"));
    assert_eq!(items[1].filter_text.as_deref(), Some("email"));
}

#[test]
fn completions_skip_named() {
    let params = vec![
        make_param("name", Some("string"), true),
        make_param("age", Some("int"), true),
    ];
    let ctx = NamedArgContext {
        call_expression: "foo".to_string(),
        existing_named_args: vec!["name".to_string()],
        positional_count: 0,
        prefix: String::new(),
    };

    let items = build_named_arg_completions(&ctx, &params);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].filter_text.as_deref(), Some("age"));
}

#[test]
fn completions_filter_by_prefix() {
    let params = vec![
        make_param("name", Some("string"), true),
        make_param("notify", Some("bool"), false),
        make_param("age", Some("int"), true),
    ];
    let ctx = NamedArgContext {
        call_expression: "foo".to_string(),
        existing_named_args: vec![],
        positional_count: 0,
        prefix: "na".to_string(),
    };

    let items = build_named_arg_completions(&ctx, &params);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].filter_text.as_deref(), Some("name"));
}

#[test]
fn completions_untyped_param() {
    let params = vec![make_param("data", None, true)];
    let ctx = NamedArgContext {
        call_expression: "foo".to_string(),
        existing_named_args: vec![],
        positional_count: 0,
        prefix: String::new(),
    };

    let items = build_named_arg_completions(&ctx, &params);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "data:");
}

#[test]
fn completions_optional_detail() {
    let params = vec![
        make_param("name", Some("string"), true),
        make_param("age", Some("int"), false),
    ];
    let ctx = NamedArgContext {
        call_expression: "foo".to_string(),
        existing_named_args: vec![],
        positional_count: 0,
        prefix: String::new(),
    };

    let items = build_named_arg_completions(&ctx, &params);
    assert_eq!(items[0].detail.as_deref(), Some("Named argument"));
    assert_eq!(
        items[1].detail.as_deref(),
        Some("Named argument (optional)")
    );
}

#[test]
fn completions_variadic_detail() {
    let params = vec![ParameterInfo {
        name: atom("$items"),
        is_required: true,
        type_hint: Some(PhpType::parse("string")),
        native_type_hint: Some(PhpType::parse("string")),
        description: None,
        default_value: None,
        is_variadic: true,
        is_reference: false,
        closure_this_type: None,
        param_out_type: None,
    }];
    let ctx = NamedArgContext {
        call_expression: "foo".to_string(),
        existing_named_args: vec![],
        positional_count: 0,
        prefix: String::new(),
    };

    let items = build_named_arg_completions(&ctx, &params);
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].detail.as_deref(),
        Some("Named argument (variadic)")
    );
}

#[test]
fn completions_have_variable_kind() {
    let params = vec![make_param("x", Some("int"), true)];
    let ctx = NamedArgContext {
        call_expression: "foo".to_string(),
        existing_named_args: vec![],
        positional_count: 0,
        prefix: String::new(),
    };

    let items = build_named_arg_completions(&ctx, &params);
    assert_eq!(items[0].kind, Some(CompletionItemKind::VARIABLE));
}

#[test]
fn completions_empty_when_all_used() {
    let params = vec![
        make_param("x", Some("int"), true),
        make_param("y", Some("int"), true),
    ];
    let ctx = NamedArgContext {
        call_expression: "foo".to_string(),
        existing_named_args: vec!["x".to_string(), "y".to_string()],
        positional_count: 0,
        prefix: String::new(),
    };

    let items = build_named_arg_completions(&ctx, &params);
    assert!(items.is_empty());
}

#[test]
fn completions_prefix_case_insensitive() {
    let params = vec![
        make_param("Name", Some("string"), true),
        make_param("age", Some("int"), true),
    ];
    let ctx = NamedArgContext {
        call_expression: "foo".to_string(),
        existing_named_args: vec![],
        positional_count: 0,
        prefix: "na".to_string(),
    };

    let items = build_named_arg_completions(&ctx, &params);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].filter_text.as_deref(), Some("Name"));
}
