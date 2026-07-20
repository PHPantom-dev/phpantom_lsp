use super::*;
use crate::php_type::PhpType;

// ── detect_call_site_text_fallback ──────────────────────────────

#[test]
fn detect_simple_function_call() {
    let content = "<?php\nfoo(";
    let pos = Position {
        line: 1,
        character: 4,
    };
    let site = detect_call_site_text_fallback(content, pos).unwrap();
    assert_eq!(site.call_expression, "foo");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn detect_second_parameter() {
    let content = "<?php\nfoo($a, ";
    let pos = Position {
        line: 1,
        character: 8,
    };
    let site = detect_call_site_text_fallback(content, pos).unwrap();
    assert_eq!(site.call_expression, "foo");
    assert_eq!(site.active_parameter, 1);
}

#[test]
fn detect_third_parameter() {
    let content = "<?php\nfoo($a, $b, ";
    let pos = Position {
        line: 1,
        character: 13,
    };
    let site = detect_call_site_text_fallback(content, pos).unwrap();
    assert_eq!(site.call_expression, "foo");
    assert_eq!(site.active_parameter, 2);
}

#[test]
fn detect_method_call() {
    let content = "<?php\n$obj->bar(";
    let pos = Position {
        line: 1,
        character: 10,
    };
    let site = detect_call_site_text_fallback(content, pos).unwrap();
    assert_eq!(site.call_expression, "$obj->bar");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn detect_static_method_call() {
    let content = "<?php\nFoo::bar(";
    let pos = Position {
        line: 1,
        character: 9,
    };
    let site = detect_call_site_text_fallback(content, pos).unwrap();
    assert_eq!(site.call_expression, "Foo::bar");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn detect_constructor_call() {
    let content = "<?php\nnew Foo(";
    let pos = Position {
        line: 1,
        character: 8,
    };
    let site = detect_call_site_text_fallback(content, pos).unwrap();
    assert_eq!(site.call_expression, "new Foo");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn detect_none_outside_parens() {
    let content = "<?php\nfoo();";
    let pos = Position {
        line: 1,
        character: 6,
    };
    assert!(detect_call_site_text_fallback(content, pos).is_none());
}

#[test]
fn detect_nested_call_inner() {
    // Cursor inside inner call
    let content = "<?php\nfoo(bar(";
    let pos = Position {
        line: 1,
        character: 8,
    };
    let site = detect_call_site_text_fallback(content, pos).unwrap();
    assert_eq!(site.call_expression, "bar");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn detect_with_string_containing_comma() {
    let content = "<?php\nfoo('a,b', ";
    let pos = Position {
        line: 1,
        character: 12,
    };
    let site = detect_call_site_text_fallback(content, pos).unwrap();
    assert_eq!(site.call_expression, "foo");
    assert_eq!(site.active_parameter, 1);
}

#[test]
fn detect_with_nested_parens_containing_comma() {
    let content = "<?php\nfoo(bar(1, 2), ";
    let pos = Position {
        line: 1,
        character: 16,
    };
    let site = detect_call_site_text_fallback(content, pos).unwrap();
    assert_eq!(site.call_expression, "foo");
    assert_eq!(site.active_parameter, 1);
}

// ── count_top_level_commas ──────────────────────────────────────

#[test]
fn count_commas_empty() {
    let chars: Vec<char> = "()".chars().collect();
    assert_eq!(count_top_level_commas(&chars, 1, 1), 0);
}

#[test]
fn count_commas_two() {
    let chars: Vec<char> = "($a, $b, $c)".chars().collect();
    assert_eq!(count_top_level_commas(&chars, 1, 11), 2);
}

#[test]
fn count_commas_nested() {
    let chars: Vec<char> = "(foo(1, 2), $b)".chars().collect();
    assert_eq!(count_top_level_commas(&chars, 1, 14), 1);
}

#[test]
fn count_commas_in_string() {
    let chars: Vec<char> = "('a,b', $c)".chars().collect();
    assert_eq!(count_top_level_commas(&chars, 1, 10), 1);
}

// ── format_param_label ──────────────────────────────────────────

#[test]
fn format_param_with_default_value() {
    let p = ParameterInfo {
        name: crate::atom::atom("$limit"),
        type_hint: Some(PhpType::parse("int")),
        native_type_hint: Some(PhpType::parse("int")),
        description: None,
        default_value: Some("10".to_string()),
        is_required: false,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    };
    assert_eq!(format_param_label(&p), "int $limit = 10");
}

#[test]
fn format_param_with_null_default() {
    let p = ParameterInfo {
        name: crate::atom::atom("$name"),
        type_hint: Some(PhpType::parse("?string")),
        native_type_hint: Some(PhpType::parse("?string")),
        description: None,
        default_value: Some("null".to_string()),
        is_required: false,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    };
    assert_eq!(format_param_label(&p), "?string $name = null");
}

#[test]
fn format_param_optional_no_known_default() {
    // Optional but no default_value extracted — no ` = ...` suffix.
    let p = ParameterInfo {
        name: crate::atom::atom("$x"),
        type_hint: Some(PhpType::parse("int")),
        native_type_hint: Some(PhpType::parse("int")),
        description: None,
        default_value: None,
        is_required: false,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    };
    assert_eq!(format_param_label(&p), "int $x");
}

#[test]
fn format_param_variadic_no_default_even_if_set() {
    // Variadic params should never show a default value.
    let p = ParameterInfo {
        name: crate::atom::atom("$items"),
        type_hint: Some(PhpType::parse("string")),
        native_type_hint: Some(PhpType::parse("string")),
        description: None,
        default_value: Some("[]".to_string()),
        is_required: false,
        is_variadic: true,
        is_reference: false,
        closure_this_type: None,
    };
    assert_eq!(format_param_label(&p), "string ...$items");
}

#[test]
fn format_param_simple() {
    let p = ParameterInfo {
        name: crate::atom::atom("$x"),
        type_hint: Some(PhpType::parse("int")),
        native_type_hint: Some(PhpType::parse("int")),
        description: None,
        default_value: None,
        is_required: true,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    };
    assert_eq!(format_param_label(&p), "int $x");
}

#[test]
fn format_param_variadic() {
    let p = ParameterInfo {
        name: crate::atom::atom("$items"),
        type_hint: Some(PhpType::parse("string")),
        native_type_hint: Some(PhpType::parse("string")),
        description: None,
        default_value: None,
        is_required: false,
        is_variadic: true,
        is_reference: false,
        closure_this_type: None,
    };
    assert_eq!(format_param_label(&p), "string ...$items");
}

#[test]
fn format_param_reference() {
    let p = ParameterInfo {
        name: crate::atom::atom("$arr"),
        type_hint: Some(PhpType::parse("array")),
        native_type_hint: Some(PhpType::parse("array")),
        description: None,
        default_value: None,
        is_required: true,
        is_variadic: false,
        is_reference: true,
        closure_this_type: None,
    };
    assert_eq!(format_param_label(&p), "array &$arr");
}

#[test]
fn format_param_no_type() {
    let p = ParameterInfo {
        name: crate::atom::atom("$x"),
        type_hint: None,
        native_type_hint: None,
        description: None,
        default_value: None,
        is_required: true,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    };
    assert_eq!(format_param_label(&p), "$x");
}

// ── build_signature ─────────────────────────────────────────────

#[test]
fn build_signature_label() {
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
            is_required: true,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
    ];
    let ret = PhpType::parse("void");
    let sig = build_signature(&params, Some(&ret));
    assert_eq!(sig.label, "(string $name, int $age): void");
}

#[test]
fn build_signature_parameter_offsets() {
    let params = vec![
        ParameterInfo {
            name: crate::atom::atom("$a"),
            type_hint: None,
            native_type_hint: None,
            description: None,
            default_value: None,
            is_required: true,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
        ParameterInfo {
            name: crate::atom::atom("$b"),
            type_hint: None,
            native_type_hint: None,
            description: None,
            default_value: None,
            is_required: true,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
    ];
    let sig = build_signature(&params, None);
    // label: "($a, $b): mixed"
    //         0123456789...
    let pi = sig.parameters.unwrap();
    assert_eq!(pi[0].label, ParameterLabel::LabelOffsets([1, 3])); // "$a"
    assert_eq!(pi[1].label, ParameterLabel::LabelOffsets([5, 7])); // "$b"
}

#[test]
fn build_signature_no_params() {
    let ret = PhpType::parse("void");
    let sig = build_signature(&[], Some(&ret));
    assert_eq!(sig.label, "(): void");
    assert!(sig.parameters.unwrap().is_empty());
}

#[test]
fn build_signature_no_return_type_shows_mixed() {
    let sig = build_signature(&[], None);
    assert_eq!(sig.label, "(): mixed");
}

#[test]
fn build_signature_with_default_values() {
    let params = vec![
        ParameterInfo {
            name: crate::atom::atom("$name"),
            type_hint: Some(PhpType::parse("string")),
            native_type_hint: Some(PhpType::parse("string")),
            description: None,
            default_value: Some("'World'".to_string()),
            is_required: false,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
        ParameterInfo {
            name: crate::atom::atom("$count"),
            type_hint: Some(PhpType::parse("int")),
            native_type_hint: Some(PhpType::parse("int")),
            description: None,
            default_value: Some("1".to_string()),
            is_required: false,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
    ];
    let ret = PhpType::parse("void");
    let sig = build_signature(&params, Some(&ret));
    assert_eq!(sig.label, "(string $name = 'World', int $count = 1): void");
    // Verify offsets still track the labels correctly.
    let pi = sig.parameters.unwrap();
    // "(" = 1 char, then "string $name = 'World'" = 22 chars
    assert_eq!(pi[0].label, ParameterLabel::LabelOffsets([1, 23]));
    // ", " = 2 chars, then "int $count = 1" = 14 chars
    assert_eq!(pi[1].label, ParameterLabel::LabelOffsets([25, 39]));
}

#[test]
fn build_signature_param_documentation_same_types() {
    // When effective == native, only the description text is shown.
    let params = vec![
        ParameterInfo {
            name: crate::atom::atom("$callback"),
            type_hint: Some(PhpType::parse("callable")),
            native_type_hint: Some(PhpType::parse("callable")),
            description: Some("The callback function to run for each element.".to_string()),
            default_value: None,
            is_required: true,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
        ParameterInfo {
            name: crate::atom::atom("$array"),
            type_hint: Some(PhpType::parse("array")),
            native_type_hint: Some(PhpType::parse("array")),
            description: None,
            default_value: None,
            is_required: true,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
    ];
    let ret = PhpType::parse("array");
    let sig = build_signature(&params, Some(&ret));
    let pi = sig.parameters.unwrap();

    // First param: effective == native, so just the description.
    match &pi[0].documentation {
        Some(Documentation::MarkupContent(mc)) => {
            assert_eq!(mc.kind, MarkupKind::Markdown);
            assert_eq!(mc.value, "The callback function to run for each element.");
        }
        other => panic!("Expected MarkupContent, got {:?}", other),
    }
    // Second param has no documentation.
    assert!(pi[1].documentation.is_none());
}

#[test]
fn build_signature_param_documentation_effective_differs() {
    // When effective != native, the doc line is prefixed with the shortened effective type.
    let params = vec![ParameterInfo {
        name: crate::atom::atom("$users"),
        type_hint: Some(PhpType::parse("list<User>")),
        native_type_hint: Some(PhpType::parse("array")),
        description: Some("The active users.".to_string()),
        default_value: None,
        is_required: true,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    }];
    let ret = PhpType::parse("void");
    let sig = build_signature(&params, Some(&ret));
    let pi = sig.parameters.unwrap();

    match &pi[0].documentation {
        Some(Documentation::MarkupContent(mc)) => {
            assert_eq!(mc.value, "`list<User>` The active users."); // already short
        }
        other => panic!("Expected MarkupContent, got {:?}", other),
    }
    // The label uses native type, not effective.
    assert_eq!(sig.label, "(array $users): void");
}

#[test]
fn build_signature_param_effective_only_no_native() {
    // When effective exists but native is None, show effective prefix.
    let params = vec![ParameterInfo {
        name: crate::atom::atom("$items"),
        type_hint: Some(PhpType::parse("list<Pen>")),
        native_type_hint: None,
        description: Some("The items.".to_string()),
        default_value: None,
        is_required: true,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    }];
    let sig = build_signature(&params, None);
    let pi = sig.parameters.unwrap();

    match &pi[0].documentation {
        Some(Documentation::MarkupContent(mc)) => {
            assert_eq!(mc.value, "`list<Pen>` The items."); // already short
        }
        other => panic!("Expected MarkupContent, got {:?}", other),
    }
    // No native type → label has bare $items.
    assert_eq!(sig.label, "($items): mixed");
}

#[test]
fn build_signature_param_effective_differs_no_description() {
    // When effective != native but there is no description text,
    // the shortened effective type alone is shown (e.g. `class-string<T>`).
    let params = vec![ParameterInfo {
        name: crate::atom::atom("$class"),
        type_hint: Some(PhpType::parse("class-string<T>")),
        native_type_hint: Some(PhpType::parse("string")),
        description: None,
        default_value: None,
        is_required: true,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    }];
    let ret = PhpType::parse("object");
    let sig = build_signature(&params, Some(&ret));
    let pi = sig.parameters.unwrap();

    match &pi[0].documentation {
        Some(Documentation::MarkupContent(mc)) => {
            assert_eq!(mc.value, "`class-string<T>`"); // already short
        }
        other => panic!("Expected MarkupContent, got {:?}", other),
    }
    // Label uses native type.
    assert_eq!(sig.label, "(string $class): object");
}

#[test]
fn build_signature_no_sig_documentation() {
    // Signature-level documentation is always None.
    let sig = build_signature(&[], None);
    assert!(sig.documentation.is_none());
    assert_eq!(sig.label, "(): mixed");
}

#[test]
fn build_signature_param_effective_fqn_shortened_in_doc() {
    // FQNs inside the effective type are shortened to base names in param docs.
    let params = vec![ParameterInfo {
        name: crate::atom::atom("$users"),
        type_hint: Some(PhpType::parse("list<\\App\\Models\\User>")),
        native_type_hint: Some(PhpType::parse("array")),
        description: Some("The active users.".to_string()),
        default_value: None,
        is_required: true,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    }];
    let ret = PhpType::parse("void");
    let sig = build_signature(&params, Some(&ret));
    let pi = sig.parameters.unwrap();

    match &pi[0].documentation {
        Some(Documentation::MarkupContent(mc)) => {
            assert_eq!(mc.value, "`list<User>` The active users.");
        }
        other => panic!("Expected MarkupContent, got {:?}", other),
    }
}

#[test]
fn build_signature_param_effective_fqn_no_desc() {
    // FQN shortened even when there is no description.
    let params = vec![ParameterInfo {
        name: crate::atom::atom("$item"),
        type_hint: Some(PhpType::parse("\\App\\Models\\Item")),
        native_type_hint: Some(PhpType::parse("object")),
        description: None,
        default_value: None,
        is_required: true,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    }];
    let sig = build_signature(&params, None);
    let pi = sig.parameters.unwrap();

    match &pi[0].documentation {
        Some(Documentation::MarkupContent(mc)) => {
            assert_eq!(mc.value, "`Item`");
        }
        other => panic!("Expected MarkupContent, got {:?}", other),
    }
}

#[test]
fn build_signature_return_type_shortened() {
    let ret = PhpType::parse("\\App\\Models\\User");
    let sig = build_signature(&[], Some(&ret));
    assert_eq!(sig.label, "(): User");
}

#[test]
fn build_signature_return_type_union_shortened() {
    let ret = PhpType::parse("\\App\\User|\\App\\Admin");
    let sig = build_signature(&[], Some(&ret));
    assert_eq!(sig.label, "(): User|Admin");
}

#[test]
fn build_signature_return_type_scalar_unchanged() {
    let ret = PhpType::parse("string");
    let sig = build_signature(&[], Some(&ret));
    assert_eq!(sig.label, "(): string");
}

// ── shorten_type (delegates to crate::hover::shorten_php_type) ──────

#[test]
fn shorten_plain_scalar() {
    let ty = PhpType::parse("int");
    assert_eq!(crate::hover::shorten_php_type(&ty), "int");
}

#[test]
fn shorten_fqn() {
    let ty = PhpType::parse("\\App\\Models\\User");
    assert_eq!(crate::hover::shorten_php_type(&ty), "User");
}

#[test]
fn shorten_union() {
    let ty = PhpType::parse("\\App\\User|\\App\\Admin");
    assert_eq!(crate::hover::shorten_php_type(&ty), "User|Admin");
}

#[test]
fn shorten_mixed_union() {
    let ty = PhpType::parse("string|\\App\\User|null");
    assert_eq!(crate::hover::shorten_php_type(&ty), "string|User|null");
}

#[test]
fn shorten_generic_param() {
    let ty = PhpType::parse("list<\\App\\User>");
    assert_eq!(crate::hover::shorten_php_type(&ty), "list<User>");
}

#[test]
fn shorten_generic_multiple_params() {
    let ty = PhpType::parse("array<string, \\App\\Models\\User>");
    assert_eq!(crate::hover::shorten_php_type(&ty), "array<string, User>");
}

#[test]
fn shorten_nested_generic_union() {
    let ty = PhpType::parse("Collection<\\App\\User|\\App\\Admin>");
    assert_eq!(
        crate::hover::shorten_php_type(&ty),
        "Collection<User|Admin>"
    );
}

#[test]
fn shorten_class_string_generic() {
    let ty = PhpType::parse("class-string<\\App\\User>");
    assert_eq!(crate::hover::shorten_php_type(&ty), "class-string<User>");
}

#[test]
fn shorten_no_namespace_unchanged() {
    let ty = PhpType::parse("list<User>");
    assert_eq!(crate::hover::shorten_php_type(&ty), "list<User>");
}

// ── clamp_active_param ──────────────────────────────────────────

#[test]
fn clamp_within_range() {
    let params = vec![
        ParameterInfo {
            name: crate::atom::atom("$a"),
            type_hint: None,
            native_type_hint: None,
            description: None,
            default_value: None,
            is_required: true,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
        ParameterInfo {
            name: crate::atom::atom("$b"),
            type_hint: None,
            native_type_hint: None,
            description: None,
            default_value: None,
            is_required: true,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
        },
    ];
    assert_eq!(clamp_active_param(0, &params), 0);
    assert_eq!(clamp_active_param(1, &params), 1);
}

#[test]
fn clamp_exceeds_range() {
    let params = vec![ParameterInfo {
        name: crate::atom::atom("$a"),
        type_hint: None,
        native_type_hint: None,
        description: None,
        default_value: None,
        is_required: true,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    }];
    assert_eq!(clamp_active_param(5, &params), 0);
}

#[test]
fn clamp_empty_params() {
    assert_eq!(clamp_active_param(0, &[]), 0);
}

// ── detect_call_site_from_map ───────────────────────────────────

/// Helper: parse PHP source and build a SymbolMap, then call
/// `detect_call_site_from_map` at the given line/character.
fn map_detect(content: &str, line: u32, character: u32) -> Option<CallSiteContext> {
    use mago_allocator::LocalArena;
    use mago_database::file::FileId;

    let arena = LocalArena::new();
    let file_id = FileId::new(b"test.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let sm = crate::symbol_map::extract_symbol_map(program, content);
    let pos = Position { line, character };
    detect_call_site_from_map(&sm, content, pos)
}

#[test]
fn map_simple_function_call() {
    // "foo($a, );" — cursor on the space before `)`, after the comma
    //  f o o ( $ a ,   ) ;
    //  0 1 2 3 4 5 6 7 8 9   (col on line 1)
    let content = "<?php\nfoo($a, );";
    let site = map_detect(content, 1, 7).unwrap();
    assert_eq!(site.call_expression, "foo");
    assert_eq!(site.active_parameter, 1);
}

#[test]
fn map_function_call_first_param() {
    // "foo($a);" — cursor on `$` inside parens
    //  f o o ( $ a ) ;
    //  0 1 2 3 4 5 6 7
    let content = "<?php\nfoo($a);";
    let site = map_detect(content, 1, 5).unwrap();
    assert_eq!(site.call_expression, "foo");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_method_call() {
    // "$obj->bar($x);" — cursor on `$x` inside parens
    //  $ o b j - > b a r (  $  x  )  ;
    //  0 1 2 3 4 5 6 7 8 9 10 11 12 13
    let content = "<?php\n$obj->bar($x);";
    let site = map_detect(content, 1, 11).unwrap();
    assert_eq!(site.call_expression, "$obj->bar");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_property_chain_method_call() {
    // "$this->prop->method($x);" — cursor on `$x` inside method parens
    //  $ t h i s - > p r o  p  -  >  m  e  t  h  o  d  (  $  x  )  ;
    //  0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23
    let content = "<?php\n$this->prop->method($x);";
    let site = map_detect(content, 1, 22).unwrap();
    assert_eq!(site.call_expression, "$this->prop->method");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_chained_method_result() {
    // "$obj->first()->second($x);" — cursor inside second()'s parens
    //  $ o b j - > f i r s  t  (  )  -  >  s  e  c  o  n  d  (  $  x  )  ;
    //  0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25
    let content = "<?php\n$obj->first()->second($x);";
    let site = map_detect(content, 1, 24).unwrap();
    assert_eq!(site.call_expression, "$obj->first()->second");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_static_method_call() {
    // "Foo::bar($x);" — cursor on `$x` inside parens
    //  F o o : : b a r (  $  x  )  ;
    //  0 1 2 3 4 5 6 7 8  9 10 11 12
    let content = "<?php\nFoo::bar($x);";
    let site = map_detect(content, 1, 10).unwrap();
    assert_eq!(site.call_expression, "Foo::bar");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_constructor_call() {
    // "new Foo($x);" — cursor on `$x` inside parens
    //  n e w   F o o (  $  x  )  ;
    //  0 1 2 3 4 5 6 7  8  9 10 11
    let content = "<?php\nnew Foo($x);";
    let site = map_detect(content, 1, 9).unwrap();
    assert_eq!(site.call_expression, "new Foo");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_nested_call_inner() {
    // "foo(bar($x));" — cursor inside bar()'s parens on `$x`
    //  f o o ( b a r (  $  x  )  )  ;
    //  0 1 2 3 4 5 6 7  8  9 10 11 12
    let content = "<?php\nfoo(bar($x));";
    let site = map_detect(content, 1, 9).unwrap();
    assert_eq!(site.call_expression, "bar");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_nested_call_outer() {
    // "foo(bar($x), $y);" — cursor on `$y` in foo()'s second arg
    //  f o o ( b a r (  $  x  )  ,     $  y  )  ;
    //  0 1 2 3 4 5 6 7  8  9 10 11 12 13 14 15 16
    let content = "<?php\nfoo(bar($x), $y);";
    let site = map_detect(content, 1, 14).unwrap();
    assert_eq!(site.call_expression, "foo");
    assert_eq!(site.active_parameter, 1);
}

#[test]
fn map_string_with_commas() {
    // "foo('a,b', $x);" — comma inside string not counted
    //  f o o ( '  a  ,  b  '  ,     $  x  )  ;
    //  0 1 2 3 4  5  6  7  8  9 10 11 12 13 14
    let content = "<?php\nfoo('a,b', $x);";
    let site = map_detect(content, 1, 11).unwrap();
    assert_eq!(site.call_expression, "foo");
    assert_eq!(site.active_parameter, 1);
}

#[test]
fn map_nullsafe_method_call() {
    // "$obj?->format($x);" — cursor on `$x` inside parens
    //  $ o b j ?  -  >  f  o  r  m  a  t  (  $  x  )  ;
    //  0 1 2 3 4  5  6  7  8  9 10 11 12 13 14 15 16 17
    let content = "<?php\n$obj?->format($x);";
    let site = map_detect(content, 1, 15).unwrap();
    assert_eq!(site.call_expression, "$obj->format");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_new_expression_chain() {
    // "(new Foo())->method($x);" — cursor on `$x`
    //  (  n  e  w     F  o  o  (  )  )  -  >  m  e  t  h  o  d  (  $  x  )  ;
    //  0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15 16 17 18 19 20 21 22 23
    let content = "<?php\n(new Foo())->method($x);";
    let site = map_detect(content, 1, 21).unwrap();
    assert_eq!(site.call_expression, "Foo->method");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_none_outside_parens() {
    // "foo();" — cursor on `;` after closing paren
    //  f o o ( ) ;
    //  0 1 2 3 4 5
    let content = "<?php\nfoo();";
    assert!(map_detect(content, 1, 5).is_none());
}

#[test]
fn map_deep_property_chain() {
    // "$a->b->c->d($x);" — cursor on `$x` inside d()'s parens
    //  $ a - > b -  >  c  -  >  d  (  $  x  )  ;
    //  0 1 2 3 4 5  6  7  8  9 10 11 12 13 14 15
    let content = "<?php\n$a->b->c->d($x);";
    let site = map_detect(content, 1, 13).unwrap();
    assert_eq!(site.call_expression, "$a->b->c->d");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_function_return_chain() {
    // "app()->make($x);" — cursor on `$x` inside make()'s parens
    //  a p p (  )  -  >  m  a  k  e  (  $  x  )  ;
    //  0 1 2 3  4  5  6  7  8  9 10 11 12 13 14 15
    let content = "<?php\napp()->make($x);";
    let site = map_detect(content, 1, 13).unwrap();
    assert_eq!(site.call_expression, "app()->make");
    assert_eq!(site.active_parameter, 0);
}

#[test]
fn map_third_parameter() {
    // "foo($a, $b, $c);" — cursor on `$c` after two commas
    //  f o o ( $  a  ,     $  b  ,     $  c  )  ;
    //  0 1 2 3 4  5  6  7  8  9 10 11 12 13 14 15
    let content = "<?php\nfoo($a, $b, $c);";
    let site = map_detect(content, 1, 13).unwrap();
    assert_eq!(site.call_expression, "foo");
    assert_eq!(site.active_parameter, 2);
}

// ── Function-definition suppression ─────────────────────────────

#[test]
fn suppressed_on_named_function_definition() {
    // `function foo(` — cursor inside definition param list, not a call.
    let content = "<?php\nfunction foo(int $a, string $b) {}";
    let pos = Position {
        line: 1,
        character: 13,
    };
    assert!(detect_call_site_text_fallback(content, pos).is_none());
}

#[test]
fn suppressed_on_anonymous_function() {
    // `function (` — anonymous function definition.
    let content = "<?php\n$f = function (int $x) {};";
    let pos = Position {
        line: 1,
        character: 15,
    };
    assert!(detect_call_site_text_fallback(content, pos).is_none());
}

#[test]
fn suppressed_on_arrow_function() {
    // `fn(` — arrow function definition.
    let content = "<?php\n$f = fn(int $x) => $x;";
    let pos = Position {
        line: 1,
        character: 8,
    };
    assert!(detect_call_site_text_fallback(content, pos).is_none());
}

#[test]
fn suppressed_on_method_definition() {
    // `public function bar(` — method definition.
    let content = "<?php\nclass A {\n    public function bar(int $a) {}\n}";
    let pos = Position {
        line: 2,
        character: 25,
    };
    assert!(detect_call_site_text_fallback(content, pos).is_none());
}

#[test]
fn not_suppressed_on_actual_function_call() {
    // `foo(` — a regular function call should still work.
    let content = "<?php\nfoo($a);";
    let pos = Position {
        line: 1,
        character: 4,
    };
    let site = detect_call_site_text_fallback(content, pos).unwrap();
    assert_eq!(site.call_expression, "foo");
}

// ── is_function_definition_paren unit tests ─────────────────────

#[test]
fn defn_paren_named_function() {
    let chars: Vec<char> = "function foo(".chars().collect();
    // paren_pos = index of `(`
    assert!(is_function_definition_paren(&chars, 12));
}

#[test]
fn defn_paren_anonymous_function() {
    let chars: Vec<char> = "$f = function (".chars().collect();
    assert!(is_function_definition_paren(&chars, 14));
}

#[test]
fn defn_paren_arrow_fn() {
    let chars: Vec<char> = "$f = fn(".chars().collect();
    assert!(is_function_definition_paren(&chars, 7));
}

#[test]
fn defn_paren_method() {
    let chars: Vec<char> = "    public function bar(".chars().collect();
    assert!(is_function_definition_paren(&chars, 23));
}

#[test]
fn defn_paren_not_a_call() {
    let chars: Vec<char> = "foo(".chars().collect();
    assert!(!is_function_definition_paren(&chars, 3));
}

#[test]
fn defn_paren_not_new() {
    let chars: Vec<char> = "new Foo(".chars().collect();
    assert!(!is_function_definition_paren(&chars, 7));
}

#[test]
fn defn_paren_not_method_call() {
    let chars: Vec<char> = "$obj->method(".chars().collect();
    assert!(!is_function_definition_paren(&chars, 12));
}

#[test]
fn defn_paren_suffix_not_keyword() {
    // `myfunction(` — "function" is a suffix of the identifier, not a keyword.
    let chars: Vec<char> = "myfunction(".chars().collect();
    assert!(!is_function_definition_paren(&chars, 10));
}

#[test]
fn fcc_extract_simple_function() {
    let content = "<?php\nfunction makePen(string $ink): Pen { return new Pen(); }\nclass Pen {}\nclass Demo {\n    public function test() {\n        $fn = makePen(...);\n        $fn();\n    }\n}\n";
    // cursor_offset should be somewhere on line 6 (`$fn();`)
    // Find the offset of `$fn()` on line 6.
    let cursor_offset = content.find("$fn();").unwrap() as u32 + 4; // inside parens
    let result = Backend::extract_callable_target_from_variable("$fn", content, cursor_offset);
    assert_eq!(result, Some("makePen".to_string()));
}
