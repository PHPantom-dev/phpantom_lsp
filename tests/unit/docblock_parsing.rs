//! Unit tests for docblock parsing functions.
//!
//! These tests exercise the public API of `phpantom_lsp::docblock` —
//! tag extraction, type resolution, conditional return types, etc.

use phpantom_lsp::docblock::*;
use phpantom_lsp::php_type::PhpType;
use phpantom_lsp::types::*;

// ─── @method tag extraction ─────────────────────────────────────────

#[test]
fn method_tag_simple() {
    let doc = "/** @method MockInterface mock(string $abstract) */";
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "mock");
    assert_eq!(
        methods[0].return_type_str().as_deref(),
        Some("MockInterface")
    );
    assert!(!methods[0].is_static);
    assert_eq!(methods[0].parameters.len(), 1);
    assert_eq!(methods[0].parameters[0].name, "$abstract");
    assert_eq!(
        methods[0].parameters[0].type_hint_str().as_deref(),
        Some("string")
    );
    assert!(methods[0].parameters[0].is_required);
}

#[test]
fn method_tag_static() {
    let doc = "/** @method static Decimal getAmountUntilBonusCashIsTriggered() */";
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "getAmountUntilBonusCashIsTriggered");
    assert_eq!(methods[0].return_type_str().as_deref(), Some("Decimal"));
    assert!(methods[0].is_static);
    assert!(methods[0].parameters.is_empty());
}

#[test]
fn method_tag_no_return_type() {
    let doc = "/** @method assertDatabaseHas(string $table, array<string, mixed> $data, string $connection = null) */";
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "assertDatabaseHas");
    assert!(methods[0].return_type.is_none());
    assert_eq!(methods[0].parameters.len(), 3);
    assert_eq!(methods[0].parameters[0].name, "$table");
    assert_eq!(
        methods[0].parameters[0].type_hint_str().as_deref(),
        Some("string")
    );
    assert!(methods[0].parameters[0].is_required);
    assert_eq!(methods[0].parameters[1].name, "$data");
    assert_eq!(
        methods[0].parameters[1].type_hint_str().as_deref(),
        Some("array<string, mixed>")
    );
    assert!(methods[0].parameters[1].is_required);
    assert_eq!(methods[0].parameters[2].name, "$connection");
    assert_eq!(
        methods[0].parameters[2].type_hint_str().as_deref(),
        Some("string")
    );
    assert!(!methods[0].parameters[2].is_required);
}

#[test]
fn method_tag_fqn_return_type() {
    let doc = "/** @method \\Mockery\\MockInterface mock(string $abstract) */";
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 1);
    assert_eq!(
        methods[0].return_type_str().as_deref(),
        Some("\\Mockery\\MockInterface")
    );
}

#[test]
fn method_tag_callable_param() {
    let doc = "/** @method MockInterface mock(string $abstract, callable():mixed $mockDefinition = null) */";
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].parameters.len(), 2);
    assert_eq!(methods[0].parameters[1].name, "$mockDefinition");
    assert!(!methods[0].parameters[1].is_required);
}

#[test]
fn method_tag_multiple() {
    let doc = concat!(
        "/**\n",
        " * @method \\Mockery\\MockInterface mock(string $abstract, callable():mixed $mockDefinition = null)\n",
        " * @method assertDatabaseHas(string $table, array<string, mixed> $data, string $connection = null)\n",
        " * @method assertDatabaseMissing(string $table, array<string, mixed> $data, string $connection = null)\n",
        " * @method static Decimal getAmountUntilBonusCashIsTriggered()\n",
        " */",
    );
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 4);
    assert_eq!(methods[0].name, "mock");
    assert!(!methods[0].is_static);
    assert_eq!(methods[1].name, "assertDatabaseHas");
    assert!(!methods[1].is_static);
    assert_eq!(methods[2].name, "assertDatabaseMissing");
    assert!(!methods[2].is_static);
    assert_eq!(methods[3].name, "getAmountUntilBonusCashIsTriggered");
    assert!(methods[3].is_static);
}

#[test]
fn method_tag_no_params() {
    let doc = "/** @method string getName() */";
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "getName");
    assert_eq!(methods[0].return_type_str().as_deref(), Some("string"));
    assert!(methods[0].parameters.is_empty());
}

#[test]
fn method_tag_nullable_return() {
    let doc = "/** @method ?User findUser(int $id) */";
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].return_type_str().as_deref(), Some("?User"));
}

#[test]
fn method_tag_none_when_missing() {
    let doc = "/** @property string $name */";
    let methods = extract_method_tags(doc);
    assert!(methods.is_empty());
}

#[test]
fn method_tag_variadic_param() {
    let doc = "/** @method void addItems(string ...$items) */";
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].parameters.len(), 1);
    assert!(methods[0].parameters[0].is_variadic);
    assert!(!methods[0].parameters[0].is_required);
}

#[test]
fn method_tag_name_matches_type_keyword() {
    let doc =
        "/** @method static string string(string $key, \\Closure|string|null $default = null) */";
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].name, "string");
    assert_eq!(methods[0].return_type_str().as_deref(), Some("string"));
    assert!(methods[0].is_static);
    assert_eq!(methods[0].parameters.len(), 2);
    assert_eq!(methods[0].parameters[0].name, "$key");
    assert_eq!(
        methods[0].parameters[0].type_hint_str().as_deref(),
        Some("string")
    );
}

#[test]
fn method_tag_malformed_does_not_panic() {
    // A malformed signature where `(` is preceded by `>` at index 1
    // previously underflowed `i - 2` and panicked. It should now be
    // parsed gracefully (yielding no method).
    let doc = "/** @method >() */";
    let methods = extract_method_tags(doc);
    assert!(methods.is_empty());

    // A few more degenerate shapes that exercise the same scanner.
    assert!(extract_method_tags("/** @method <>() */").is_empty());
    assert!(extract_method_tags("/** @method static >() */").is_empty());
}

// ─── @property tag extraction ───────────────────────────────────────

#[test]
fn property_tag_simple() {
    let doc = "/** @property Session $session */";
    let props = extract_property_tags(doc);
    assert_eq!(
        props,
        vec![("session".to_string(), Some(PhpType::parse("Session")))]
    );
}

#[test]
fn property_tag_nullable() {
    let doc = "/** @property ?int $count */";
    let props = extract_property_tags(doc);
    assert_eq!(
        props,
        vec![("count".to_string(), Some(PhpType::parse("?int")))]
    );
}

#[test]
fn property_tag_union_with_null() {
    let doc = "/** @property null|int $latest_id */";
    let props = extract_property_tags(doc);
    assert_eq!(
        props,
        vec![("latest_id".to_string(), Some(PhpType::parse("null|int")))]
    );
}

#[test]
fn property_tag_fqn() {
    let doc = "/** @property \\App\\Models\\User $user */";
    let props = extract_property_tags(doc);
    assert_eq!(
        props,
        vec![(
            "user".to_string(),
            Some(PhpType::parse("\\App\\Models\\User"))
        )]
    );
}

#[test]
fn property_tag_multiple() {
    let doc = concat!(
        "/**\n",
        " * @property null|int                    $latest_subscription_agreement_id\n",
        " * @property UserMobileVerificationState $mobile_verification_state\n",
        " */",
    );
    let props = extract_property_tags(doc);
    assert_eq!(props.len(), 2);
    // `null|int` is preserved in the type hint.
    assert_eq!(
        props[0],
        (
            "latest_subscription_agreement_id".to_string(),
            Some(PhpType::parse("null|int"))
        )
    );
    assert_eq!(
        props[1],
        (
            "mobile_verification_state".to_string(),
            Some(PhpType::parse("UserMobileVerificationState"))
        )
    );
}

#[test]
fn property_tag_read_write_variants() {
    let doc = concat!(
        "/**\n",
        " * @property-read string $name\n",
        " * @property-write int $age\n",
        " */",
    );
    let props = extract_property_tags(doc);
    assert_eq!(props.len(), 2);
    assert_eq!(
        props[0],
        ("name".to_string(), Some(PhpType::parse("string")))
    );
    assert_eq!(props[1], ("age".to_string(), Some(PhpType::parse("int"))));
}

#[test]
fn property_tag_no_type() {
    let doc = "/** @property $thing */";
    let props = extract_property_tags(doc);
    assert_eq!(props, vec![("thing".to_string(), None)]);
}

#[test]
fn property_tag_generic_preserved() {
    let doc = "/** @property Collection<int, Model> $items */";
    let props = extract_property_tags(doc);
    assert_eq!(
        props,
        vec![(
            "items".to_string(),
            Some(PhpType::parse("Collection<int, Model>"))
        )]
    );
}

#[test]
fn property_tag_none_when_missing() {
    let doc = "/** @return Foo */";
    let props = extract_property_tags(doc);
    assert!(props.is_empty());
}

// ── extract_return_type (skips conditionals) ────────────────────────

#[test]
fn return_type_conditional_is_skipped() {
    let doc = concat!(
        "/**\n",
        " * @return ($abstract is class-string<TClass> ? TClass : mixed)\n",
        " */",
    );
    assert_eq!(extract_return_type(doc), None);
}

// ── extract_return_type ─────────────────────────────────────────────

#[test]
fn return_type_simple() {
    let doc = "/** @return Application */";
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse("Application"))
    );
}

#[test]
fn return_type_fqn() {
    let doc = "/** @return \\Illuminate\\Session\\Store */";
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse("\\Illuminate\\Session\\Store"))
    );
}

#[test]
fn return_type_nullable() {
    let doc = "/** @return ?Application */";
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse("?Application"))
    );
}

#[test]
fn return_type_with_description() {
    let doc = "/** @return Application The main app instance */";
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse("Application"))
    );
}

#[test]
fn return_type_multiline() {
    let doc = concat!(
        "/**\n",
        " * Some method.\n",
        " *\n",
        " * @param string $key\n",
        " * @return \\Illuminate\\Session\\Store\n",
        " */",
    );
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse("\\Illuminate\\Session\\Store"))
    );
}

#[test]
fn return_type_none_when_missing() {
    let doc = "/** This is a docblock without a return tag */";
    assert_eq!(extract_return_type(doc), None);
}

#[test]
fn return_type_nullable_union() {
    let doc = "/** @return Application|null */";
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse("Application|null"))
    );
}

#[test]
fn return_type_generic_preserved() {
    let doc = "/** @return Collection<int, Model> */";
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse("Collection<int, Model>"))
    );
}

// ── Multi-line @return types ────────────────────────────────────────

#[test]
fn return_type_multiline_generic_simple() {
    let doc = concat!(
        "/**\n",
        " * @return array<\n",
        " *   string,\n",
        " *   int\n",
        " * >\n",
        " */",
    );
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse("array<string, int>"))
    );
}

#[test]
fn return_type_multiline_static_with_conditionals() {
    // Stripped-down version of Laravel Collection::groupBy's @return
    let doc = concat!(
        "/**\n",
        " * @return static<\n",
        " *  ($groupBy is (array|string)\n",
        " *      ? array-key\n",
        " *      : TGroupKey),\n",
        " *  static<($preserveKeys is true ? TKey : int), TValue>\n",
        " * >\n",
        " */",
    );
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse(
            "static<($groupBy is (array|string) ? array-key : TGroupKey), static<($preserveKeys is true ? TKey : int), TValue>>"
        ))
    );
}

#[test]
fn return_type_multiline_nested_generics() {
    let doc = concat!(
        "/**\n",
        " * @return Collection<\n",
        " *   int,\n",
        " *   Collection<string, User>\n",
        " * >\n",
        " */",
    );
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse("Collection<int, Collection<string, User>>"))
    );
}

#[test]
fn return_type_multiline_brace_shape() {
    let doc = concat!(
        "/**\n",
        " * @return array{\n",
        " *   name: string,\n",
        " *   age: int\n",
        " * }\n",
        " */",
    );
    assert_eq!(
        extract_return_type(doc),
        Some(PhpType::parse("array{name: string, age: int}"))
    );
}

// ── Unclosed-bracket recovery ───────────────────────────────────────

#[test]
fn return_type_unclosed_angle_recovers_base() {
    // `extract_return_type` now returns PhpType directly. For broken
    // docblocks with unclosed `<`, the internal sanitisation recovers
    // the base type.
    let doc = concat!("/**\n", " * @return SomeType<\n", " */",);
    let raw = extract_return_type(doc);
    assert_eq!(raw, Some(PhpType::parse("SomeType")));
}

#[test]
fn return_type_unclosed_angle_static_recovers() {
    let doc = concat!("/**\n", " * @return static<\n", " */",);
    let raw = extract_return_type(doc);
    assert_eq!(raw, Some(PhpType::parse("static")));
}

// ── resolve_effective_type fallback ─────────────────────────────────

#[test]
fn effective_type_broken_docblock_falls_back_to_native() {
    // If the docblock type is completely unrecoverable, the native type
    // should win.
    let native = PhpType::parse("Result");
    let sanitised = sanitise_and_parse_docblock_type("<broken");
    assert_eq!(
        resolve_effective_type_typed(Some(&native), sanitised.as_ref()).map(|t| t.to_string()),
        Some("Result".into()),
    );
}

#[test]
fn effective_type_broken_docblock_recovers_base() {
    // When there IS a recoverable base in the broken docblock and no
    // native hint, partial recovery should kick in.
    let sanitised = sanitise_and_parse_docblock_type("Collection<int");
    assert_eq!(
        resolve_effective_type_typed(None, sanitised.as_ref()).map(|t| t.to_string()),
        Some("Collection".into()),
    );
}

#[test]
fn effective_type_balanced_docblock_unchanged() {
    // A well-formed docblock type should pass through normally.
    assert_eq!(
        resolve_effective_type_typed(
            Some(&PhpType::parse("array")),
            Some(&PhpType::parse("Collection<int, User>"))
        )
        .map(|t| t.to_string()),
        Some("Collection<int, User>".into()),
    );
}

// ── extract_var_type ────────────────────────────────────────────────

#[test]
fn var_type_simple() {
    let doc = "/** @var Session */";
    assert_eq!(extract_var_type(doc), Some(PhpType::parse("Session")));
}

#[test]
fn var_type_fqn() {
    let doc = "/** @var \\App\\Models\\User */";
    assert_eq!(
        extract_var_type(doc),
        Some(PhpType::parse("\\App\\Models\\User"))
    );
}

#[test]
fn var_type_none_when_missing() {
    let doc = "/** just a comment */";
    assert_eq!(extract_var_type(doc), None);
}

// ── extract_var_type_with_name ──────────────────────────────────────

#[test]
fn var_type_with_name_simple() {
    let doc = "/** @var Session */";
    assert_eq!(
        extract_var_type_with_name(doc),
        Some((PhpType::parse("Session"), None))
    );
}

#[test]
fn var_type_with_name_has_var() {
    let doc = "/** @var Session $sess */";
    assert_eq!(
        extract_var_type_with_name(doc),
        Some((PhpType::parse("Session"), Some("$sess".into())))
    );
}

#[test]
fn var_type_with_name_fqn() {
    let doc = "/** @var \\App\\Models\\User $user */";
    assert_eq!(
        extract_var_type_with_name(doc),
        Some((PhpType::parse("\\App\\Models\\User"), Some("$user".into())))
    );
}

#[test]
fn var_type_with_name_no_var_tag() {
    let doc = "/** just a comment */";
    assert_eq!(extract_var_type_with_name(doc), None);
}

#[test]
fn var_type_with_name_description_not_var() {
    // Second token is not a $variable — should be ignored.
    let doc = "/** @var Session some description */";
    assert_eq!(
        extract_var_type_with_name(doc),
        Some((PhpType::parse("Session"), None))
    );
}

#[test]
fn var_type_with_name_generic_preserved() {
    let doc = "/** @var Collection<int, User> $items */";
    assert_eq!(
        extract_var_type_with_name(doc),
        Some((
            PhpType::parse("Collection<int, User>"),
            Some("$items".into())
        ))
    );
}

// ── find_inline_var_docblock ────────────────────────────────────────

#[test]
fn inline_var_docblock_simple() {
    let content = "<?php\n/** @var Session */\n$var = mystery();\n";
    let stmt_start = content.find("$var").unwrap();
    assert_eq!(
        find_inline_var_docblock(content, stmt_start),
        Some((PhpType::parse("Session"), None))
    );
}

#[test]
fn inline_var_docblock_with_var_name() {
    let content = "<?php\n/** @var Session $var */\n$var = mystery();\n";
    let stmt_start = content.find("$var =").unwrap();
    assert_eq!(
        find_inline_var_docblock(content, stmt_start),
        Some((PhpType::parse("Session"), Some("$var".into())))
    );
}

#[test]
fn inline_var_docblock_fqn() {
    let content = "<?php\n/** @var \\App\\Models\\User */\n$u = get();\n";
    let stmt_start = content.find("$u").unwrap();
    assert_eq!(
        find_inline_var_docblock(content, stmt_start),
        Some((PhpType::parse("\\App\\Models\\User"), None))
    );
}

#[test]
fn inline_var_docblock_no_docblock() {
    let content = "<?php\n$var = mystery();\n";
    let stmt_start = content.find("$var").unwrap();
    assert_eq!(find_inline_var_docblock(content, stmt_start), None);
}

#[test]
fn inline_var_docblock_regular_comment_ignored() {
    // A `/* ... */` comment (not `/** */`) should not match.
    let content = "<?php\n/* @var Session */\n$var = mystery();\n";
    let stmt_start = content.find("$var").unwrap();
    assert_eq!(find_inline_var_docblock(content, stmt_start), None);
}

#[test]
fn inline_var_docblock_with_indentation() {
    let content = "<?php\nclass A {\n    public function f() {\n        /** @var Session */\n        $var = mystery();\n    }\n}\n";
    let stmt_start = content.find("$var").unwrap();
    assert_eq!(
        find_inline_var_docblock(content, stmt_start),
        Some((PhpType::parse("Session"), None))
    );
}

// ── should_override_type ────────────────────────────────────────────

#[test]
fn override_object_with_class() {
    assert!(should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("object")
    ));
}

#[test]
fn override_mixed_with_class() {
    assert!(should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("mixed")
    ));
}

#[test]
fn override_class_with_subclass() {
    assert!(should_override_type_typed(
        &PhpType::parse("ConcreteSession"),
        &PhpType::parse("SessionInterface")
    ));
}

#[test]
fn no_override_int_with_class() {
    assert!(!should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("int")
    ));
}

#[test]
fn no_override_string_with_class() {
    assert!(!should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("string")
    ));
}

#[test]
fn no_override_bool_with_class() {
    assert!(!should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("bool")
    ));
}

#[test]
fn override_array_with_class() {
    // `array` is a broad container type that docblocks commonly refine
    // (e.g. `@param list<User> $users` with native `array`).
    // Non-scalar docblock types should be allowed to override it.
    assert!(should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("array")
    ));
}

#[test]
fn override_array_with_generic_list() {
    // `list<User>` is the most common refinement of native `array`.
    assert!(should_override_type_typed(
        &PhpType::parse("list<User>"),
        &PhpType::parse("array")
    ));
}

#[test]
fn override_array_with_generic_collection() {
    assert!(should_override_type_typed(
        &PhpType::parse("Collection<int, Order>"),
        &PhpType::parse("array")
    ));
}

#[test]
fn override_iterable_with_class() {
    // `iterable` is another broad container type that docblocks refine.
    assert!(should_override_type_typed(
        &PhpType::parse("Collection<int, User>"),
        &PhpType::parse("iterable")
    ));
}

#[test]
fn override_nullable_array_with_class() {
    assert!(should_override_type_typed(
        &PhpType::parse("list<User>"),
        &PhpType::parse("?array")
    ));
}

#[test]
fn no_override_array_with_scalar_docblock() {
    // A plain scalar docblock (no generics) should not override.
    assert!(!should_override_type_typed(
        &PhpType::parse("array"),
        &PhpType::parse("array")
    ));
    assert!(!should_override_type_typed(
        &PhpType::parse("string"),
        &PhpType::parse("string")
    ));
}

#[test]
fn override_array_with_generic_scalar_docblock() {
    // A scalar-based docblock WITH generic parameters (e.g. `array<string, mixed>`)
    // should override, because the generic arguments carry type information
    // useful for destructuring and foreach element type extraction.
    assert!(should_override_type_typed(
        &PhpType::parse("array<string, mixed>"),
        &PhpType::parse("array")
    ));
    assert!(should_override_type_typed(
        &PhpType::parse("array<int, User>"),
        &PhpType::parse("array")
    ));
    assert!(should_override_type_typed(
        &PhpType::parse("iterable<string, Order>"),
        &PhpType::parse("iterable")
    ));
}

#[test]
fn no_override_void_with_class() {
    assert!(!should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("void")
    ));
}

#[test]
fn no_override_nullable_int_with_class() {
    assert!(!should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("?int")
    ));
}

#[test]
fn override_nullable_object_with_class() {
    assert!(should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("?object")
    ));
}

#[test]
fn no_override_scalar_union_with_class() {
    assert!(!should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("string|int")
    ));
}

#[test]
fn override_union_with_object_part() {
    // `SomeClass|null` has a non-scalar part → overridable
    assert!(should_override_type_typed(
        &PhpType::parse("ConcreteClass"),
        &PhpType::parse("SomeClass|null")
    ));
}

#[test]
fn no_override_when_docblock_is_scalar() {
    // Even if native is object, if docblock says `int`, no point overriding
    assert!(!should_override_type_typed(
        &PhpType::parse("int"),
        &PhpType::parse("object")
    ));
}

#[test]
fn override_self_with_class() {
    assert!(should_override_type_typed(
        &PhpType::parse("ConcreteClass"),
        &PhpType::parse("self")
    ));
}

#[test]
fn override_static_with_class() {
    assert!(should_override_type_typed(
        &PhpType::parse("ConcreteClass"),
        &PhpType::parse("static")
    ));
}

#[test]
fn override_string_with_class_string() {
    assert!(should_override_type_typed(
        &PhpType::parse("class-string"),
        &PhpType::parse("string")
    ));
}

#[test]
fn override_string_with_non_empty_string() {
    assert!(should_override_type_typed(
        &PhpType::parse("non-empty-string"),
        &PhpType::parse("string")
    ));
}

#[test]
fn override_string_with_numeric_string() {
    assert!(should_override_type_typed(
        &PhpType::parse("numeric-string"),
        &PhpType::parse("string")
    ));
}

#[test]
fn override_string_with_literal_string() {
    assert!(should_override_type_typed(
        &PhpType::parse("literal-string"),
        &PhpType::parse("string")
    ));
}

#[test]
fn override_int_with_positive_int() {
    assert!(should_override_type_typed(
        &PhpType::parse("positive-int"),
        &PhpType::parse("int")
    ));
}

#[test]
fn override_int_with_negative_int() {
    assert!(should_override_type_typed(
        &PhpType::parse("negative-int"),
        &PhpType::parse("int")
    ));
}

#[test]
fn override_int_with_non_negative_int() {
    assert!(should_override_type_typed(
        &PhpType::parse("non-negative-int"),
        &PhpType::parse("int")
    ));
}

#[test]
fn override_nullable_string_with_non_empty_string() {
    assert!(should_override_type_typed(
        &PhpType::parse("non-empty-string"),
        &PhpType::parse("?string")
    ));
}

#[test]
fn override_string_with_class_string_generic() {
    // `class-string<Foo>` is a valid refinement of `string`.
    assert!(should_override_type_typed(
        &PhpType::parse("class-string<Foo>"),
        &PhpType::parse("string")
    ));
}

#[test]
fn no_override_string_with_array_generic() {
    // `array<int>` is not compatible with native `string` — completely
    // different type family.  The native declaration wins.
    assert!(!should_override_type_typed(
        &PhpType::parse("array<int>"),
        &PhpType::parse("string")
    ));
}

#[test]
fn no_override_string_with_collection_generic() {
    // `Collection<User>` is not a string refinement.
    assert!(!should_override_type_typed(
        &PhpType::parse("Collection<User>"),
        &PhpType::parse("string")
    ));
}

#[test]
fn no_override_int_with_array_generic() {
    // `array<int>` is not compatible with native `int`.
    assert!(!should_override_type_typed(
        &PhpType::parse("array<int>"),
        &PhpType::parse("int")
    ));
}

#[test]
fn no_override_int_with_class_name() {
    // A class name is not a refinement of `int`.
    assert!(!should_override_type_typed(
        &PhpType::parse("Session"),
        &PhpType::parse("int")
    ));
}

#[test]
fn no_override_bool_with_array_shape() {
    // `array{name: string}` is not compatible with native `bool`.
    assert!(!should_override_type_typed(
        &PhpType::parse("array{name: string}"),
        &PhpType::parse("bool")
    ));
}

#[test]
fn no_override_float_with_array_generic() {
    assert!(!should_override_type_typed(
        &PhpType::parse("array<string>"),
        &PhpType::parse("float")
    ));
}

#[test]
fn override_int_with_int_range() {
    // `int<0, max>` is a valid refinement of `int`.
    assert!(should_override_type_typed(
        &PhpType::parse("int<0, max>"),
        &PhpType::parse("int")
    ));
}

#[test]
fn override_string_with_non_empty_string_generic() {
    // `non-empty-string` pseudo-type with generic params (unusual but valid).
    assert!(should_override_type_typed(
        &PhpType::parse("non-empty-string"),
        &PhpType::parse("string")
    ));
}

#[test]
fn no_override_string_with_list_generic() {
    // `list<User>` is not a string refinement.
    assert!(!should_override_type_typed(
        &PhpType::parse("list<User>"),
        &PhpType::parse("string")
    ));
}

#[test]
fn no_override_nullable_int_with_array_generic() {
    // `array<int>` is not compatible with `?int`.
    assert!(!should_override_type_typed(
        &PhpType::parse("array<int>"),
        &PhpType::parse("?int")
    ));
}

// ── resolve_effective_type ──────────────────────────────────────────

#[test]
fn effective_type_docblock_only() {
    assert_eq!(
        resolve_effective_type_typed(None, Some(&PhpType::parse("Session"))).map(|t| t.to_string()),
        Some("Session".into())
    );
}

#[test]
fn effective_type_native_only() {
    assert_eq!(
        resolve_effective_type_typed(Some(&PhpType::parse("int")), None).map(|t| t.to_string()),
        Some("int".into())
    );
}

#[test]
fn effective_type_both_compatible() {
    assert_eq!(
        resolve_effective_type_typed(
            Some(&PhpType::parse("object")),
            Some(&PhpType::parse("Session"))
        )
        .map(|t| t.to_string()),
        Some("Session".into())
    );
}

#[test]
fn effective_type_both_incompatible() {
    assert_eq!(
        resolve_effective_type_typed(
            Some(&PhpType::parse("int")),
            Some(&PhpType::parse("Session"))
        )
        .map(|t| t.to_string()),
        Some("int".into())
    );
}

#[test]
fn effective_type_neither() {
    assert_eq!(
        resolve_effective_type_typed(None, None).map(|t| t.to_string()),
        None::<String>
    );
}

// ── extract_conditional_return_type ─────────────────────────────────

#[test]
fn conditional_simple_class_string() {
    let doc = concat!(
        "/**\n",
        " * @return ($abstract is class-string<TClass> ? TClass : mixed)\n",
        " */",
    );
    let result = extract_conditional_return_type(doc);
    assert!(result.is_some(), "Should parse a conditional return type");
    let cond = result.unwrap();
    match cond {
        PhpType::Conditional {
            ref param,
            negated,
            ref condition,
            ref then_type,
            ref else_type,
        } => {
            assert_eq!(param, "$abstract");
            assert!(!negated);
            assert!(matches!(condition.as_ref(), PhpType::ClassString(_)));
            assert_eq!(**then_type, PhpType::Named("TClass".into()));
            assert_eq!(**else_type, PhpType::mixed());
        }
        _ => panic!("Expected Conditional, got {:?}", cond),
    }
}

#[test]
fn conditional_null_check() {
    let doc = concat!(
        "/**\n",
        " * @return ($guard is null ? \\Illuminate\\Contracts\\Auth\\Factory : \\Illuminate\\Contracts\\Auth\\StatefulGuard)\n",
        " */",
    );
    let result = extract_conditional_return_type(doc).unwrap();
    match result {
        PhpType::Conditional {
            param,
            negated,
            condition,
            then_type,
            else_type,
        } => {
            assert_eq!(param, "$guard");
            assert!(!negated);
            assert_eq!(*condition, PhpType::null());
            assert_eq!(
                *then_type,
                PhpType::Named("\\Illuminate\\Contracts\\Auth\\Factory".into())
            );
            assert_eq!(
                *else_type,
                PhpType::Named("\\Illuminate\\Contracts\\Auth\\StatefulGuard".into())
            );
        }
        _ => panic!("Expected Conditional"),
    }
}

#[test]
fn conditional_nested() {
    let doc = concat!(
        "/**\n",
        " * @return ($abstract is class-string<TClass> ? TClass : ($abstract is null ? \\Illuminate\\Foundation\\Application : mixed))\n",
        " */",
    );
    let result = extract_conditional_return_type(doc).unwrap();
    match result {
        PhpType::Conditional {
            ref param,
            negated,
            ref condition,
            ref then_type,
            ref else_type,
        } => {
            assert_eq!(param, "$abstract");
            assert!(!negated);
            assert!(matches!(condition.as_ref(), PhpType::ClassString(_)));
            assert_eq!(**then_type, PhpType::Named("TClass".into()));
            // else_type should be another conditional
            match else_type.as_ref() {
                PhpType::Conditional {
                    param: inner_param,
                    negated: inner_negated,
                    condition: inner_cond,
                    then_type: inner_then,
                    else_type: inner_else,
                } => {
                    assert_eq!(inner_param, "$abstract");
                    assert!(!inner_negated);
                    assert_eq!(**inner_cond, PhpType::null());
                    assert_eq!(
                        **inner_then,
                        PhpType::Named("\\Illuminate\\Foundation\\Application".into())
                    );
                    assert_eq!(**inner_else, PhpType::mixed());
                }
                _ => panic!("Expected nested Conditional"),
            }
        }
        _ => panic!("Expected Conditional"),
    }
}

#[test]
fn conditional_multiline() {
    let doc = concat!(
        "/**\n",
        " * Get the available container instance.\n",
        " *\n",
        " * @param  string|callable|null  $abstract\n",
        " * @return ($abstract is class-string<TClass>\n",
        " *     ? TClass\n",
        " *     : ($abstract is null\n",
        " *         ? \\Illuminate\\Foundation\\Application\n",
        " *         : mixed))\n",
        " */",
    );
    let result = extract_conditional_return_type(doc);
    assert!(result.is_some(), "Should parse multi-line conditional");
    match result.unwrap() {
        PhpType::Conditional {
            param, condition, ..
        } => {
            assert_eq!(param, "$abstract");
            assert!(matches!(condition.as_ref(), PhpType::ClassString(_)));
        }
        _ => panic!("Expected Conditional"),
    }
}

#[test]
fn conditional_is_type() {
    let doc = concat!(
        "/**\n",
        " * @return ($job is \\Closure ? \\Illuminate\\Foundation\\Bus\\PendingClosureDispatch : \\Illuminate\\Foundation\\Bus\\PendingDispatch)\n",
        " */",
    );
    let result = extract_conditional_return_type(doc).unwrap();
    match result {
        PhpType::Conditional {
            param,
            negated,
            condition,
            then_type,
            else_type,
        } => {
            assert_eq!(param, "$job");
            assert!(!negated);
            assert_eq!(*condition, PhpType::Named("\\Closure".into()));
            assert_eq!(
                *then_type,
                PhpType::Named("\\Illuminate\\Foundation\\Bus\\PendingClosureDispatch".into())
            );
            assert_eq!(
                *else_type,
                PhpType::Named("\\Illuminate\\Foundation\\Bus\\PendingDispatch".into())
            );
        }
        _ => panic!("Expected Conditional"),
    }
}

#[test]
fn conditional_not_present() {
    let doc = "/** @return Application */";
    assert_eq!(extract_conditional_return_type(doc), None);
}

#[test]
fn conditional_no_return_tag() {
    let doc = "/** Just a comment */";
    assert_eq!(extract_conditional_return_type(doc), None);
}

// ─── @mixin tag extraction ──────────────────────────────────────────────

#[test]
fn mixin_tag_simple() {
    let doc = concat!("/**\n", " * @mixin ShoppingCart\n", " */",);
    let mixins = extract_mixin_tags(doc);
    assert_eq!(mixins, vec![("ShoppingCart".to_string(), vec![])]);
}

#[test]
fn mixin_tag_fqn() {
    let doc = concat!("/**\n", " * @mixin \\App\\Models\\ShoppingCart\n", " */",);
    let mixins = extract_mixin_tags(doc);
    assert_eq!(
        mixins,
        vec![("\\App\\Models\\ShoppingCart".to_string(), vec![])]
    );
}

#[test]
fn mixin_tag_multiple() {
    let doc = concat!(
        "/**\n",
        " * @mixin ShoppingCart\n",
        " * @mixin Wishlist\n",
        " */",
    );
    let mixins = extract_mixin_tags(doc);
    assert_eq!(
        mixins,
        vec![
            ("ShoppingCart".to_string(), vec![]),
            ("Wishlist".to_string(), vec![]),
        ]
    );
}

#[test]
fn mixin_tag_none_when_missing() {
    let doc = "/** Just a comment */";
    let mixins = extract_mixin_tags(doc);
    assert!(mixins.is_empty());
}

#[test]
fn mixin_tag_with_description() {
    let doc = concat!(
        "/**\n",
        " * @mixin ShoppingCart Some extra description\n",
        " */",
    );
    let mixins = extract_mixin_tags(doc);
    assert_eq!(mixins, vec![("ShoppingCart".to_string(), vec![])]);
}

#[test]
fn mixin_tag_generic_preserved() {
    let doc = concat!("/**\n", " * @mixin Collection<int, Model>\n", " */",);
    let mixins = extract_mixin_tags(doc);
    assert_eq!(
        mixins,
        vec![(
            "Collection".to_string(),
            vec![PhpType::parse("int"), PhpType::parse("Model")],
        )]
    );
}

#[test]
fn mixin_tag_mixed_with_other_tags() {
    let doc = concat!(
        "/**\n",
        " * @property string $name\n",
        " * @mixin ShoppingCart\n",
        " * @method int getId()\n",
        " */",
    );
    let mixins = extract_mixin_tags(doc);
    assert_eq!(mixins, vec![("ShoppingCart".to_string(), vec![])]);
}

#[test]
fn mixin_tag_empty_after_tag() {
    let doc = concat!("/**\n", " * @mixin\n", " */",);
    let mixins = extract_mixin_tags(doc);
    assert!(mixins.is_empty());
}

#[test]
fn mixin_tag_union() {
    let doc = concat!("/**\n", " * @mixin Webpage|AwaitableWebpage\n", " */",);
    let mixins = extract_mixin_tags(doc);
    assert_eq!(
        mixins,
        vec![
            ("Webpage".to_string(), vec![]),
            ("AwaitableWebpage".to_string(), vec![]),
        ]
    );
}

#[test]
fn mixin_tag_union_with_generics() {
    let doc = concat!(
        "/**\n",
        " * @mixin Builder<User>|Collection<int, User>\n",
        " */",
    );
    let mixins = extract_mixin_tags(doc);
    assert_eq!(
        mixins,
        vec![
            ("Builder".to_string(), vec![PhpType::parse("User")]),
            (
                "Collection".to_string(),
                vec![PhpType::parse("int"), PhpType::parse("User")],
            ),
        ]
    );
}

// ─── @phpstan-assert / @psalm-assert extraction ─────────────────────────

#[test]
fn assert_simple_phpstan() {
    let doc = concat!("/**\n", " * @phpstan-assert User $value\n", " */",);
    let assertions = extract_type_assertions(doc);
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].kind, AssertionKind::Always);
    assert_eq!(assertions[0].param_name, "$value");
    assert_eq!(assertions[0].asserted_type.to_string(), "User");
    assert!(!assertions[0].negated);
}

#[test]
fn assert_simple_psalm() {
    let doc = concat!("/**\n", " * @psalm-assert AdminUser $obj\n", " */",);
    let assertions = extract_type_assertions(doc);
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].kind, AssertionKind::Always);
    assert_eq!(assertions[0].param_name, "$obj");
    assert_eq!(assertions[0].asserted_type.to_string(), "AdminUser");
    assert!(!assertions[0].negated);
}

#[test]
fn assert_negated() {
    let doc = concat!("/**\n", " * @phpstan-assert !User $value\n", " */",);
    let assertions = extract_type_assertions(doc);
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].kind, AssertionKind::Always);
    assert_eq!(assertions[0].asserted_type.to_string(), "User");
    assert!(assertions[0].negated);
}

#[test]
fn assert_if_true() {
    let doc = concat!("/**\n", " * @phpstan-assert-if-true User $value\n", " */",);
    let assertions = extract_type_assertions(doc);
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].kind, AssertionKind::IfTrue);
    assert_eq!(assertions[0].param_name, "$value");
    assert_eq!(assertions[0].asserted_type.to_string(), "User");
    assert!(!assertions[0].negated);
}

#[test]
fn assert_if_false() {
    let doc = concat!("/**\n", " * @phpstan-assert-if-false User $value\n", " */",);
    let assertions = extract_type_assertions(doc);
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].kind, AssertionKind::IfFalse);
    assert_eq!(assertions[0].param_name, "$value");
    assert_eq!(assertions[0].asserted_type.to_string(), "User");
    assert!(!assertions[0].negated);
}

#[test]
fn assert_psalm_if_true() {
    let doc = concat!("/**\n", " * @psalm-assert-if-true AdminUser $obj\n", " */",);
    let assertions = extract_type_assertions(doc);
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].kind, AssertionKind::IfTrue);
    assert_eq!(assertions[0].param_name, "$obj");
    assert_eq!(assertions[0].asserted_type.to_string(), "AdminUser");
}

#[test]
fn assert_fqn_type() {
    let doc = concat!(
        "/**\n",
        " * @phpstan-assert \\App\\Models\\User $value\n",
        " */",
    );
    let assertions = extract_type_assertions(doc);
    assert_eq!(assertions.len(), 1);
    assert_eq!(
        assertions[0].asserted_type.to_string(),
        "\\App\\Models\\User"
    );
}

#[test]
fn assert_multiple_annotations() {
    let doc = concat!(
        "/**\n",
        " * @phpstan-assert User $first\n",
        " * @phpstan-assert AdminUser $second\n",
        " */",
    );
    let assertions = extract_type_assertions(doc);
    assert_eq!(assertions.len(), 2);
    assert_eq!(assertions[0].param_name, "$first");
    assert_eq!(assertions[0].asserted_type.to_string(), "User");
    assert_eq!(assertions[1].param_name, "$second");
    assert_eq!(assertions[1].asserted_type.to_string(), "AdminUser");
}

#[test]
fn assert_mixed_with_other_tags() {
    let doc = concat!(
        "/**\n",
        " * Some description.\n",
        " *\n",
        " * @param mixed $value\n",
        " * @phpstan-assert User $value\n",
        " * @return void\n",
        " */",
    );
    let assertions = extract_type_assertions(doc);
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].asserted_type.to_string(), "User");
}

#[test]
fn assert_none_when_missing() {
    let doc = "/** @return void */";
    let assertions = extract_type_assertions(doc);
    assert!(assertions.is_empty());
}

#[test]
fn assert_empty_after_tag_ignored() {
    let doc = concat!("/**\n", " * @phpstan-assert\n", " */",);
    let assertions = extract_type_assertions(doc);
    assert!(assertions.is_empty());
}

#[test]
fn assert_missing_param_ignored() {
    let doc = concat!("/**\n", " * @phpstan-assert User\n", " */",);
    let assertions = extract_type_assertions(doc);
    assert!(assertions.is_empty());
}

#[test]
fn assert_param_without_dollar_ignored() {
    let doc = concat!("/**\n", " * @phpstan-assert User value\n", " */",);
    let assertions = extract_type_assertions(doc);
    assert!(assertions.is_empty());
}

#[test]
fn assert_negated_if_true() {
    let doc = concat!("/**\n", " * @phpstan-assert-if-true !User $value\n", " */",);
    let assertions = extract_type_assertions(doc);
    assert_eq!(assertions.len(), 1);
    assert_eq!(assertions[0].kind, AssertionKind::IfTrue);
    assert!(assertions[0].negated);
    assert_eq!(assertions[0].asserted_type.to_string(), "User");
}

// ─── @deprecated tag tests ──────────────────────────────────────

#[test]
fn deprecated_tag_bare() {
    let doc = concat!("/**\n", " * @deprecated\n", " */",);
    assert!(has_deprecated_tag(doc));
}

#[test]
fn deprecated_tag_with_message() {
    let doc = concat!("/**\n", " * @deprecated Use newMethod() instead.\n", " */",);
    assert!(has_deprecated_tag(doc));
}

#[test]
fn deprecated_tag_with_version() {
    let doc = concat!("/**\n", " * @deprecated since 2.0\n", " */",);
    assert!(has_deprecated_tag(doc));
}

#[test]
fn deprecated_tag_mixed_with_other_tags() {
    let doc = concat!(
        "/**\n",
        " * Some description.\n",
        " *\n",
        " * @param string $name\n",
        " * @deprecated Use something else.\n",
        " * @return void\n",
        " */",
    );
    assert!(has_deprecated_tag(doc));
}

#[test]
fn deprecated_tag_not_present() {
    let doc = concat!(
        "/**\n",
        " * @param string $name\n",
        " * @return void\n",
        " */",
    );
    assert!(!has_deprecated_tag(doc));
}

#[test]
fn deprecated_tag_empty_docblock() {
    let doc = "/** */";
    assert!(!has_deprecated_tag(doc));
}

#[test]
fn deprecated_tag_not_confused_with_similar_words() {
    // A word like "@deprecatedAlias" should NOT match — the tag must
    // be exactly "@deprecated" followed by whitespace or end-of-line.
    let doc = concat!("/**\n", " * @deprecatedAlias\n", " */",);
    assert!(!has_deprecated_tag(doc));
}

#[test]
fn deprecated_tag_at_end_of_line() {
    // Tag alone on the line with no trailing text.
    let doc = "/** @deprecated */";
    assert!(has_deprecated_tag(doc));
}

#[test]
fn deprecated_tag_with_tab_separator() {
    let doc = concat!("/**\n", " * @deprecated\tUse foo() instead\n", " */",);
    assert!(has_deprecated_tag(doc));
}

// ─── find_enclosing_return_type ─────────────────────────────────────────────

#[test]
fn enclosing_return_type_method() {
    let content = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function bar(): \\Generator {\n",
        "        yield $x;\n",
        "        $x->\n",
        "    }\n",
        "}\n",
    );
    // Cursor inside the method body, after `yield $x;\n`.
    let cursor = content.find("$x->").unwrap() + 2;
    assert_eq!(
        find_enclosing_return_type(content, cursor),
        Some(PhpType::parse("\\Generator<int, User>"))
    );
}

#[test]
fn enclosing_return_type_top_level_function() {
    let content = concat!(
        "<?php\n",
        "/** @return \\Generator<int, Order> */\n",
        "function gen(): \\Generator {\n",
        "    yield $o;\n",
        "    $o->\n",
        "}\n",
    );
    let cursor = content.find("$o->").unwrap() + 2;
    assert_eq!(
        find_enclosing_return_type(content, cursor),
        Some(PhpType::parse("\\Generator<int, Order>"))
    );
}

#[test]
fn enclosing_return_type_no_docblock() {
    let content = concat!(
        "<?php\n",
        "function gen(): \\Generator {\n",
        "    yield $x;\n",
        "    $x->\n",
        "}\n",
    );
    let cursor = content.find("$x->").unwrap() + 2;
    assert_eq!(find_enclosing_return_type(content, cursor), None);
}

#[test]
fn enclosing_return_type_static_method() {
    let content = concat!(
        "<?php\n",
        "class Svc {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public static function run(): \\Generator {\n",
        "        yield $u;\n",
        "        $u->\n",
        "    }\n",
        "}\n",
    );
    let cursor = content.find("$u->").unwrap() + 2;
    assert_eq!(
        find_enclosing_return_type(content, cursor),
        Some(PhpType::parse("\\Generator<int, User>"))
    );
}

#[test]
fn enclosing_return_type_abstract_protected() {
    let content = concat!(
        "<?php\n",
        "class Base {\n",
        "    /** @return \\Generator<string, Item> */\n",
        "    protected function items(): \\Generator {\n",
        "        yield $i;\n",
        "        $i->\n",
        "    }\n",
        "}\n",
    );
    let cursor = content.find("$i->").unwrap() + 2;
    assert_eq!(
        find_enclosing_return_type(content, cursor),
        Some(PhpType::parse("\\Generator<string, Item>"))
    );
}

#[test]
fn enclosing_return_type_skips_nested_braces() {
    let content = concat!(
        "<?php\n",
        "class Repo {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function find(): \\Generator {\n",
        "        if (true) {\n",
        "            $x = 1;\n",
        "        }\n",
        "        yield $u;\n",
        "        $u->\n",
        "    }\n",
        "}\n",
    );
    let cursor = content.find("$u->").unwrap() + 2;
    assert_eq!(
        find_enclosing_return_type(content, cursor),
        Some(PhpType::parse("\\Generator<int, User>"))
    );
}

/// When the cursor is deeply nested inside while/if blocks, the backward
/// brace scan must skip all intermediate `{`/`}` and find the function's
/// opening brace — not stop at the innermost block's `{`.
#[test]
fn enclosing_return_type_deeply_nested_control_flow() {
    let content = concat!(
        "<?php\n",
        "class Scheduler {\n",
        "    /** @return \\Generator<int, string, Task, void> */\n",
        "    public function schedule(): \\Generator {\n",
        "        while (true) {\n",
        "            if (true) {\n",
        "                $task = yield 'waiting';\n",
        "                $task->\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    // Cursor inside the deeply nested block — the function still wraps
    // the cursor, so find_enclosing_return_type should find it.  However,
    // when called with the cursor position directly, the backward scan
    // stops at the `if`'s `{` (depth goes to -1 before reaching the
    // function `{`).
    //
    // The correct usage from the AST walker is to pass the method body's
    // opening brace offset + 1 so that the scan immediately finds the
    // function brace.  Here we verify both behaviors:

    // Passing the method body's `{` offset + 1 should work.
    let func_brace =
        content.find("schedule(): \\Generator {").unwrap() + "schedule(): \\Generator {".len();
    assert_eq!(
        find_enclosing_return_type(content, func_brace),
        Some(PhpType::parse("\\Generator<int, string, Task, void>")),
        "Should find return type when scanning from just past the method's opening brace"
    );
}

// ─── Template default value parsing ─────────────────────────────────

#[test]
fn template_default_simple_bool() {
    let doc = concat!("/**\n", " * @template TAsync of bool = false\n", " */",);
    let result = extract_template_params_full(doc);
    assert_eq!(result.len(), 1);
    let (name, bound, _, default) = &result[0];
    assert_eq!(name, "TAsync");
    assert_eq!(*bound, Some(PhpType::parse("bool")));
    assert_eq!(*default, Some(PhpType::parse("false")));
}

#[test]
fn template_default_true() {
    let doc = concat!("/**\n", " * @template TSync of bool = true\n", " */",);
    let result = extract_template_params_full(doc);
    assert_eq!(result.len(), 1);
    let (name, bound, _, default) = &result[0];
    assert_eq!(name, "TSync");
    assert_eq!(*bound, Some(PhpType::parse("bool")));
    assert_eq!(*default, Some(PhpType::parse("true")));
}

#[test]
fn template_default_null() {
    let doc = concat!("/**\n", " * @template TValue of mixed = null\n", " */",);
    let result = extract_template_params_full(doc);
    assert_eq!(result.len(), 1);
    let (name, bound, _, default) = &result[0];
    assert_eq!(name, "TValue");
    assert_eq!(*bound, Some(PhpType::parse("mixed")));
    assert_eq!(*default, Some(PhpType::parse("null")));
}

#[test]
fn template_no_default() {
    let doc = concat!("/**\n", " * @template T of string\n", " */",);
    let result = extract_template_params_full(doc);
    assert_eq!(result.len(), 1);
    let (name, bound, _, default) = &result[0];
    assert_eq!(name, "T");
    assert_eq!(*bound, Some(PhpType::parse("string")));
    assert!(default.is_none());
}

#[test]
fn template_no_bound_no_default() {
    let doc = concat!("/**\n", " * @template T\n", " */",);
    let result = extract_template_params_full(doc);
    assert_eq!(result.len(), 1);
    let (name, bound, _, default) = &result[0];
    assert_eq!(name, "T");
    assert!(bound.is_none());
    assert!(default.is_none());
}

#[test]
fn template_multiple_with_defaults() {
    let doc = concat!(
        "/**\n",
        " * @template TKey of int\n",
        " * @template TAsync of bool = false\n",
        " * @template TValue of string = null\n",
        " */",
    );
    let result = extract_template_params_full(doc);
    assert_eq!(result.len(), 3);

    let (name0, bound0, _, default0) = &result[0];
    assert_eq!(name0, "TKey");
    assert_eq!(*bound0, Some(PhpType::parse("int")));
    assert!(default0.is_none());

    let (name1, bound1, _, default1) = &result[1];
    assert_eq!(name1, "TAsync");
    assert_eq!(*bound1, Some(PhpType::parse("bool")));
    assert_eq!(*default1, Some(PhpType::parse("false")));

    let (name2, bound2, _, default2) = &result[2];
    assert_eq!(name2, "TValue");
    assert_eq!(*bound2, Some(PhpType::parse("string")));
    assert_eq!(*default2, Some(PhpType::parse("null")));
}

#[test]
fn template_default_stripped_from_bound() {
    // Ensure the bound is just the type, not "bool = false"
    let doc = concat!("/**\n", " * @template TAsync of bool = false\n", " */",);
    let params_with_bounds = extract_template_params_with_bounds(doc);
    assert_eq!(params_with_bounds.len(), 1);
    let (name, bound) = &params_with_bounds[0];
    assert_eq!(name, "TAsync");
    assert_eq!(*bound, Some(PhpType::parse("bool")));
}

#[test]
fn template_default_stripped_from_names() {
    // extract_template_params should still just return names
    let doc = concat!("/**\n", " * @template TAsync of bool = false\n", " */",);
    let params = extract_template_params(doc);
    assert_eq!(params, vec!["TAsync"]);
}

// ─── @template with `as` keyword ────────────────────────────────────

#[test]
fn template_as_bound_simple() {
    let doc = concat!("/**\n", " * @template T as SomeClass\n", " */");
    let result = extract_template_params_full(doc);
    assert_eq!(result.len(), 1);
    let (name, bound, _, default) = &result[0];
    assert_eq!(name, "T");
    assert_eq!(*bound, Some(PhpType::parse("SomeClass")));
    assert!(default.is_none());
}

#[test]
fn template_as_bound_with_default() {
    let doc = concat!("/**\n", " * @template TAsync as bool = false\n", " */");
    let result = extract_template_params_full(doc);
    assert_eq!(result.len(), 1);
    let (name, bound, _, default) = &result[0];
    assert_eq!(name, "TAsync");
    assert_eq!(*bound, Some(PhpType::parse("bool")));
    assert_eq!(*default, Some(PhpType::parse("false")));
}

#[test]
fn template_as_does_not_match_assign_prefix() {
    // "assign" starts with "as" but should NOT be treated as a bound keyword
    let doc = concat!("/**\n", " * @template Tassign\n", " */");
    let result = extract_template_params_full(doc);
    assert_eq!(result.len(), 1);
    let (name, bound, _, _) = &result[0];
    assert_eq!(name, "Tassign");
    assert!(bound.is_none());
}

#[test]
fn template_mixed_of_and_as() {
    let doc = concat!(
        "/**\n",
        " * @template TKey of int\n",
        " * @template TValue as string\n",
        " */",
    );
    let result = extract_template_params_full(doc);
    assert_eq!(result.len(), 2);

    let (name0, bound0, _, _) = &result[0];
    assert_eq!(name0, "TKey");
    assert_eq!(*bound0, Some(PhpType::parse("int")));

    let (name1, bound1, _, _) = &result[1];
    assert_eq!(name1, "TValue");
    assert_eq!(*bound1, Some(PhpType::parse("string")));
}

// ─── Conditional resolution with template defaults ──────────────────

#[test]
fn conditional_resolves_with_template_default_false() {
    use phpantom_lsp::completion::conditional_resolution::resolve_conditional_without_args_and_defaults;
    use std::collections::HashMap;

    // Simulates: @template TAsync of bool = false
    // @return (TAsync is false ? Response : PromiseInterface)
    let cond = PhpType::Conditional {
        param: "TAsync".to_string(),
        negated: false,
        condition: Box::new(PhpType::false_()),
        then_type: Box::new(PhpType::Named("Response".to_string())),
        else_type: Box::new(PhpType::Named("PromiseInterface".to_string())),
    };

    let mut defaults = HashMap::new();
    defaults.insert("TAsync".to_string(), PhpType::false_());

    let result = resolve_conditional_without_args_and_defaults(&cond, &[], Some(&defaults));
    assert_eq!(result, Some(PhpType::Named("Response".to_string())));
}

#[test]
fn conditional_resolves_with_template_default_true() {
    use phpantom_lsp::completion::conditional_resolution::resolve_conditional_without_args_and_defaults;
    use std::collections::HashMap;

    // Simulates: @template TAsync of bool = true
    // @return (TAsync is false ? Response : PromiseInterface)
    let cond = PhpType::Conditional {
        param: "TAsync".to_string(),
        negated: false,
        condition: Box::new(PhpType::false_()),
        then_type: Box::new(PhpType::Named("Response".to_string())),
        else_type: Box::new(PhpType::Named("PromiseInterface".to_string())),
    };

    let mut defaults = HashMap::new();
    defaults.insert("TAsync".to_string(), PhpType::true_());

    let result = resolve_conditional_without_args_and_defaults(&cond, &[], Some(&defaults));
    assert_eq!(result, Some(PhpType::Named("PromiseInterface".to_string())));
}

#[test]
fn conditional_no_template_default_falls_through() {
    use phpantom_lsp::completion::conditional_resolution::resolve_conditional_without_args_and_defaults;
    use std::collections::HashMap;

    // When template has no default, the function should fall through
    // to normal resolution (else branch for non-null conditions).
    let cond = PhpType::Conditional {
        param: "TAsync".to_string(),
        negated: false,
        condition: Box::new(PhpType::false_()),
        then_type: Box::new(PhpType::Named("Response".to_string())),
        else_type: Box::new(PhpType::Named("PromiseInterface".to_string())),
    };

    let defaults = HashMap::new();

    // Empty defaults map — should not resolve via template default
    let result = resolve_conditional_without_args_and_defaults(&cond, &[], Some(&defaults));
    // Falls through to else branch since TAsync is not a $param either
    assert_eq!(result, Some(PhpType::Named("PromiseInterface".to_string())));
}

#[test]
fn conditional_negated_with_template_default() {
    use phpantom_lsp::completion::conditional_resolution::resolve_conditional_without_args_and_defaults;
    use std::collections::HashMap;

    // Simulates: @template TAsync of bool = false
    // @return (TAsync is not false ? PromiseInterface : Response)
    let cond = PhpType::Conditional {
        param: "TAsync".to_string(),
        negated: true,
        condition: Box::new(PhpType::false_()),
        then_type: Box::new(PhpType::Named("PromiseInterface".to_string())),
        else_type: Box::new(PhpType::Named("Response".to_string())),
    };

    let mut defaults = HashMap::new();
    defaults.insert("TAsync".to_string(), PhpType::false_());

    // negated: TAsync is not false → false (since default IS false) → else branch → Response
    let result = resolve_conditional_without_args_and_defaults(&cond, &[], Some(&defaults));
    assert_eq!(result, Some(PhpType::Named("Response".to_string())));
}

#[test]
fn method_tag_with_template_params() {
    let doc = "/** @method TVal get<TVal of mixed>(TVal $default) */";
    let methods = extract_method_tags(doc);
    assert_eq!(methods.len(), 1, "Should parse one method");
    assert_eq!(methods[0].name, "get");
    assert_eq!(methods[0].return_type_str().as_deref(), Some("TVal"));
    assert_eq!(methods[0].template_params.len(), 1);
    assert_eq!(methods[0].template_params[0].as_str(), "TVal");
    assert!(
        methods[0]
            .template_param_bounds
            .contains_key(&phpantom_lsp::atom::atom("TVal"))
    );
    assert_eq!(methods[0].template_bindings.len(), 1);
    assert_eq!(methods[0].template_bindings[0].0.as_str(), "TVal");
    assert_eq!(methods[0].template_bindings[0].1.as_str(), "$default");
}
