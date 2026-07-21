use super::*;

// ── Enclosing return type resolution ────────────────────────────

#[test]
fn resolve_return_type_standalone_function() {
    let php = "<?php\nfunction classify(int $code): string\n{\n    if ($code < 0) return 'negative';\n    return 'ok';\n}\n";
    let offset = php.find("if ($code").unwrap() as u32;
    let result = resolve_enclosing_return_type(php, offset);
    assert_eq!(
        result,
        PhpType::parse("string"),
        "should resolve enclosing function return type"
    );
}

#[test]
fn resolve_return_type_method() {
    let php =
        "<?php\nclass Foo {\n    public function bar(): int\n    {\n        return 42;\n    }\n}\n";
    let offset = php.find("return 42").unwrap() as u32;
    let result = resolve_enclosing_return_type(php, offset);
    assert_eq!(
        result,
        PhpType::parse("int"),
        "should resolve enclosing method return type"
    );
}

// ── Statement boundary validation ───────────────────────────────

#[test]
fn complete_statements_single() {
    let php = "<?php\nfunction foo() {\n    $x = 1;\n    $y = 2;\n}\n";
    // Select `$x = 1;`
    let start = php.find("$x = 1;").unwrap();
    let end = start + "$x = 1;".len();
    assert!(selection_covers_complete_statements(php, start, end));
}

#[test]
fn complete_statements_multiple() {
    let php = "<?php\nfunction foo() {\n    $x = 1;\n    $y = 2;\n    $z = 3;\n}\n";
    let start = php.find("$x = 1;").unwrap();
    let end = php.find("$y = 2;").unwrap() + "$y = 2;".len();
    assert!(selection_covers_complete_statements(php, start, end));
}

#[test]
fn incomplete_statement_rejected() {
    let php = "<?php\nfunction foo() {\n    $x = 1;\n}\n";
    // Select just `$x = ` (incomplete).
    let start = php.find("$x = 1;").unwrap();
    let end = start + "$x =".len();
    assert!(!selection_covers_complete_statements(php, start, end));
}

#[test]
fn partial_if_rejected() {
    let php = "<?php\nfunction foo() {\n    if ($x) {\n        $y = 1;\n    }\n}\n";
    // Select just the body of the if without the if itself.
    let start = php.find("$y = 1;").unwrap();
    let end = start + "$y = 1;".len();
    // This is inside the if body — those ARE complete statements
    // within the if block, but they're not top-level statements in
    // the function body.  The validator checks against the function
    // body's direct children, so this should fail.
    assert!(!selection_covers_complete_statements(php, start, end));
}

#[test]
fn complete_if_accepted() {
    let php = "<?php\nfunction foo() {\n    if ($x) {\n        $y = 1;\n    }\n    $z = 2;\n}\n";
    // Select the entire if statement.
    let start = php.find("if ($x)").unwrap();
    let end = php.find("    }\n    $z").unwrap() + "    }".len();
    assert!(selection_covers_complete_statements(php, start, end));
}

// ── Selection trimming ──────────────────────────────────────────

#[test]
fn trim_whitespace() {
    let content = "  hello world  ";
    let result = trim_selection(content, 0, content.len());
    assert_eq!(result, Some((2, 13)));
}

#[test]
fn trim_empty_rejected() {
    let content = "   ";
    assert_eq!(trim_selection(content, 0, content.len()), None);
}

// ── Return detection ────────────────────────────────────────────

#[test]
fn detects_trailing_return() {
    let php = "<?php\nfunction foo() {\n    $x = 1;\n    return $x;\n}\n";
    let start = php.find("$x = 1;").unwrap();
    let end = php.find("return $x;").unwrap() + "return $x;".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(strategy, ReturnStrategy::TrailingReturn);
}

#[test]
fn detects_unsafe_return_without_trailing() {
    // `return 1;` followed by `$x = 2;` — the return doesn't end
    // the selection, and the values are mixed (not guard clauses),
    // so this can use sentinel-null (1 is not null).
    let php = "<?php\nfunction foo() {\n    return 1;\n    $x = 2;\n}\n";
    let start = php.find("return 1;").unwrap();
    let end = php.find("$x = 2;").unwrap() + "$x = 2;".len();
    let strategy = analyse_returns(php, start, end, 0);
    // `$x = 2;` is NOT a return, but there IS a return in the
    // selection that doesn't end it.  The only return value is `1`
    // → uniform guards with value "1".
    assert_eq!(
        strategy,
        ReturnStrategy::UniformGuards("1".to_string()),
        "single non-null return value should use uniform guards"
    );
}

#[test]
fn no_false_positive_on_return_in_identifier() {
    let php = "<?php\nfunction foo() {\n    $returnValue = 1;\n}\n";
    let start = php.find("$returnValue").unwrap();
    let end = start + "$returnValue = 1;".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(strategy, ReturnStrategy::None);
}

#[test]
fn nested_return_safe_when_trailing_return_present() {
    // Guard clause pattern: `if (!$x) return 0;` followed by
    // a trailing `return $result;`.  Since the selection ends
    // with return, ALL returns are safe (call site will be
    // `return extracted(…)`).
    let php =
        "<?php\nfunction foo($x) {\n    if (!$x) return 0;\n    $r = $x * 2;\n    return $r;\n}\n";
    let start = php.find("if (!$x)").unwrap();
    let end = php.find("return $r;").unwrap() + "return $r;".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(strategy, ReturnStrategy::TrailingReturn);
}

#[test]
fn nested_return_unsafe_without_trailing_return() {
    // Return inside an if, but the selection does NOT end with return.
    // The return value is `1` (not null) → uses sentinel-null since
    // there are no modified variables.
    let php =
        "<?php\nfunction foo($x) {\n    if ($x) {\n        return 1;\n    }\n    echo 'done';\n}\n";
    let start = php.find("if ($x)").unwrap();
    let end = php.find("echo 'done';").unwrap() + "echo 'done';".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(
        strategy,
        ReturnStrategy::UniformGuards("1".to_string()),
        "single non-null return should use uniform guards"
    );
}

// ── Guard return strategies ─────────────────────────────────────

#[test]
fn void_guards_strategy() {
    // All returns are bare `return;` → VoidGuards.
    let php = "<?php\nfunction foo($x, $y) {\n    if (!$x) return;\n    if (!$y) return;\n    echo 'ok';\n}\n";
    let start = php.find("if (!$x)").unwrap();
    let end = php.find("echo 'ok';").unwrap() + "echo 'ok';".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(strategy, ReturnStrategy::VoidGuards);
}

#[test]
fn uniform_false_guards_strategy() {
    // All returns are `return false;` → UniformGuards("false").
    let php = "<?php\nfunction foo($x, $y) {\n    if (!$x) return false;\n    if (!$y) return false;\n    echo 'ok';\n}\n";
    let start = php.find("if (!$x)").unwrap();
    let end = php.find("echo 'ok';").unwrap() + "echo 'ok';".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(strategy, ReturnStrategy::UniformGuards("false".to_string()));
}

#[test]
fn uniform_null_guards_strategy() {
    // All returns are `return null;` → UniformGuards("null").
    // This works because the bool-flag approach doesn't need null
    // as a sentinel.
    let php = "<?php\nfunction foo($id) {\n    if ($id <= 0) return null;\n    if (!$this->exists($id)) return null;\n    echo 'ok';\n}\n";
    let start = php.find("if ($id").unwrap();
    let end = php.find("echo 'ok';").unwrap() + "echo 'ok';".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(strategy, ReturnStrategy::UniformGuards("null".to_string()));
}

#[test]
fn sentinel_null_strategy() {
    // Different non-null return values → SentinelNull.
    let php = "<?php\nfunction foo($x) {\n    if ($x < 0) return 'negative';\n    if ($x > 100) return 'overflow';\n    echo 'ok';\n}\n";
    let start = php.find("if ($x < 0)").unwrap();
    let end = php.find("echo 'ok';").unwrap() + "echo 'ok';".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(strategy, ReturnStrategy::SentinelNull);
}

#[test]
fn uniform_literal_guard_value_uses_uniform_guards() {
    // A uniform literal return value (`42`) is safe to reproduce at
    // the call site → UniformGuards.
    let php = "<?php\nfunction foo($x, $y) {\n    if (!$x) return 42;\n    if (!$y) return 42;\n    echo 'ok';\n}\n";
    let start = php.find("if (!$x)").unwrap();
    let end = php.find("echo 'ok';").unwrap() + "echo 'ok';".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(strategy, ReturnStrategy::UniformGuards("42".to_string()));
}

#[test]
fn uniform_non_literal_guard_value_uses_sentinel() {
    // A uniform return value that references a variable or a call
    // cannot be reproduced at the call site (the variable may be
    // selection-local).  It must use the null sentinel so the
    // expression stays inside the extracted function.
    let php = "<?php\nfunction foo($x) {\n    $u = lookup($x);\n    if ($u) {\n        return build($u);\n    }\n    echo 'ok';\n}\n";
    let start = php.find("$u = lookup").unwrap();
    let end = php.find("echo 'ok';").unwrap() + "echo 'ok';".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(strategy, ReturnStrategy::SentinelNull);
}

#[test]
fn reproducible_guard_value_classification() {
    assert!(is_reproducible_guard_value("42"));
    assert!(is_reproducible_guard_value("-1"));
    assert!(is_reproducible_guard_value("3.14"));
    assert!(is_reproducible_guard_value("'overflow'"));
    assert!(is_reproducible_guard_value("\"text\""));
    assert!(is_reproducible_guard_value("Status::Bad"));
    assert!(is_reproducible_guard_value("self::FOO"));
    // Variables and calls are not reproducible at the call site.
    assert!(!is_reproducible_guard_value("$url"));
    assert!(!is_reproducible_guard_value("redirect($url, 301)"));
    assert!(!is_reproducible_guard_value("compute()"));
    assert!(!is_reproducible_guard_value("$this->value"));
    assert!(!is_reproducible_guard_value(""));
}

#[test]
fn mixed_null_and_other_values_is_unsafe() {
    // Returns include null AND other values → Unsafe (can't use
    // null as sentinel when null is also a valid return).
    let php = "<?php\nfunction foo($x) {\n    if ($x < 0) return null;\n    if ($x > 100) return 'overflow';\n    echo 'ok';\n}\n";
    let start = php.find("if ($x < 0)").unwrap();
    let end = php.find("echo 'ok';").unwrap() + "echo 'ok';".len();
    let strategy = analyse_returns(php, start, end, 0);
    assert_eq!(strategy, ReturnStrategy::Unsafe);
}

#[test]
fn guard_with_return_values_is_unsafe() {
    // Selection has return values (modified variables read after
    // the selection) — can't use guard strategies unless all
    // guards return null and there's exactly 1 return value.
    let php = "<?php\nfunction foo($x) {\n    if (!$x) return false;\n    echo 'ok';\n}\n";
    let start = php.find("if (!$x)").unwrap();
    let end = php.find("echo 'ok';").unwrap() + "echo 'ok';".len();
    let strategy = analyse_returns(php, start, end, 1);
    assert_eq!(strategy, ReturnStrategy::Unsafe);
}

#[test]
fn guard_with_multiple_return_values_is_unsafe() {
    // More than 1 return value — even null guards can't help.
    let php = "<?php\nfunction foo($x) {\n    if (!$x) return null;\n    $a = 1;\n    $b = 2;\n}\n";
    let start = php.find("if (!$x)").unwrap();
    let end = php.find("$b = 2;").unwrap() + "$b = 2;".len();
    let strategy = analyse_returns(php, start, end, 2);
    assert_eq!(strategy, ReturnStrategy::Unsafe);
}

#[test]
fn null_guard_with_single_return_value() {
    // All guards return null, exactly 1 return value →
    // NullGuardWithValue(false).
    let php = "<?php\nfunction foo($obj) {\n    if (!$obj) return null;\n    $val = $obj->compute();\n}\n";
    let start = php.find("if (!$obj)").unwrap();
    let end = php.find("$val = $obj->compute();").unwrap() + "$val = $obj->compute();".len();
    let strategy = analyse_returns(php, start, end, 1);
    assert_eq!(strategy, ReturnStrategy::NullGuardWithValue(false));
}

#[test]
fn void_guard_with_single_return_value() {
    // All guards are bare `return;`, exactly 1 return value →
    // NullGuardWithValue(true).
    let php =
        "<?php\nfunction foo($obj) {\n    if (!$obj) return;\n    $val = $obj->compute();\n}\n";
    let start = php.find("if (!$obj)").unwrap();
    let end = php.find("$val = $obj->compute();").unwrap() + "$val = $obj->compute();".len();
    let strategy = analyse_returns(php, start, end, 1);
    assert_eq!(strategy, ReturnStrategy::NullGuardWithValue(true));
}

#[test]
fn non_null_guard_with_return_value_is_unsafe() {
    // Guards return false (not null) with a return value — can't
    // use NullGuardWithValue, and other strategies can't handle
    // return values.
    let php = "<?php\nfunction foo($obj) {\n    if (!$obj) return false;\n    $val = $obj->compute();\n}\n";
    let start = php.find("if (!$obj)").unwrap();
    let end = php.find("$val = $obj->compute();").unwrap() + "$val = $obj->compute();".len();
    let strategy = analyse_returns(php, start, end, 1);
    assert_eq!(strategy, ReturnStrategy::Unsafe);
}

// ── Type hint validation ────────────────────────────────────────

#[test]
fn clean_scalar_types() {
    assert_eq!(clean_type_for_signature("int"), "int");
    assert_eq!(clean_type_for_signature("string"), "string");
    assert_eq!(clean_type_for_signature("bool"), "bool");
    assert_eq!(clean_type_for_signature("float"), "float");
    assert_eq!(clean_type_for_signature("array"), "array");
    assert_eq!(clean_type_for_signature("void"), "void");
    assert_eq!(clean_type_for_signature("mixed"), "mixed");
}

#[test]
fn clean_nullable_types() {
    assert_eq!(clean_type_for_signature("?int"), "?int");
    assert_eq!(clean_type_for_signature("?string"), "?string");
}

#[test]
fn clean_class_types() {
    assert_eq!(clean_type_for_signature("Foo"), "Foo");
    assert_eq!(
        clean_type_for_signature("\\App\\Models\\User"),
        "\\App\\Models\\User"
    );
}

#[test]
fn clean_union_types() {
    assert_eq!(clean_type_for_signature("int|string"), "int|string");
    assert_eq!(clean_type_for_signature("Foo|null"), "Foo|null");
}

#[test]
fn clean_empty_and_unparseable() {
    assert_eq!(clean_type_for_signature(""), "");
}

#[test]
fn clean_generic_stripped() {
    assert_eq!(clean_type_for_signature("array<string>"), "array");
    assert_eq!(
        clean_type_for_signature("Collection<int, string>"),
        "Collection"
    );
}

#[test]
fn clean_callable_types() {
    assert_eq!(
        clean_type_for_signature("callable(int): string"),
        "callable"
    );
    assert_eq!(clean_type_for_signature("Closure(int): void"), "Closure");
}

#[test]
fn clean_array_slice_syntax() {
    assert_eq!(clean_type_for_signature("int[]"), "array");
}

// ── Build param list ────────────────────────────────────────────

#[test]
fn param_list_empty() {
    assert_eq!(build_param_list(&[]), "");
}

#[test]
fn param_list_untyped() {
    let params = vec![("$x".to_string(), PhpType::untyped())];
    assert_eq!(build_param_list(&params), "$x");
}

#[test]
fn param_list_typed() {
    let params = vec![
        ("$x".to_string(), PhpType::parse("int")),
        ("$y".to_string(), PhpType::parse("string")),
    ];
    assert_eq!(build_param_list(&params), "int $x, string $y");
}

// ── Return type ─────────────────────────────────────────────────

#[test]
fn return_type_void() {
    let info = ExtractionInfo {
        name: String::new(),
        params: vec![],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    assert_eq!(build_return_type(&info), "void");
}

#[test]
fn return_type_single() {
    let info = ExtractionInfo {
        name: String::new(),
        params: vec![],
        returns: vec![("$x".to_string(), PhpType::parse("int"))],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    assert_eq!(build_return_type(&info), "int");
}

#[test]
fn return_type_multiple() {
    let info = ExtractionInfo {
        name: String::new(),
        params: vec![],
        returns: vec![
            ("$x".to_string(), PhpType::parse("int")),
            ("$y".to_string(), PhpType::parse("string")),
        ],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    assert_eq!(build_return_type(&info), "array");
}

#[test]
fn return_type_trailing_return() {
    let info = ExtractionInfo {
        name: String::new(),
        params: vec![],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::TrailingReturn,
        trailing_return_type: PhpType::parse("string"),
        docblock: String::new(),
    };
    assert_eq!(build_return_type(&info), "string");
}

#[test]
fn return_type_void_guards() {
    let info = ExtractionInfo {
        name: String::new(),
        params: vec![],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::VoidGuards,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    assert_eq!(build_return_type(&info), "bool");
}

#[test]
fn return_type_uniform_guards() {
    let info = ExtractionInfo {
        name: String::new(),
        params: vec![],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::UniformGuards("false".to_string()),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    assert_eq!(build_return_type(&info), "bool");
}

#[test]
fn return_type_sentinel_null_with_type() {
    let info = ExtractionInfo {
        name: String::new(),
        params: vec![],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::SentinelNull,
        trailing_return_type: PhpType::parse("string"),
        docblock: String::new(),
    };
    assert_eq!(build_return_type(&info), "?string");
}

#[test]
fn return_type_null_guard_with_value() {
    let info = ExtractionInfo {
        name: String::new(),
        params: vec![],
        returns: vec![("$sound".to_string(), PhpType::parse("string"))],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::NullGuardWithValue(false),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    assert_eq!(build_return_type(&info), "?string");
}

#[test]
fn return_type_null_guard_with_value_already_nullable() {
    let info = ExtractionInfo {
        name: String::new(),
        params: vec![],
        returns: vec![("$val".to_string(), PhpType::parse("?int"))],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::NullGuardWithValue(false),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    assert_eq!(build_return_type(&info), "?int");
}

#[test]
fn return_type_void_guard_with_value() {
    // Void guards with a computed value — return type is still
    // nullable (the extracted function returns null on guard-fire).
    let info = ExtractionInfo {
        name: String::new(),
        params: vec![],
        returns: vec![("$sound".to_string(), PhpType::parse("string"))],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::NullGuardWithValue(true),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    assert_eq!(build_return_type(&info), "?string");
}

// ── Name generation ──────────────────────────────────────────────

#[test]
fn generates_unique_name() {
    let content = "<?php\nfunction extracted() {}\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Function,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: String::new(),
        sibling_method_names: Vec::new(),
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "",
        return_strategy: &ReturnStrategy::None,
        body_text: "echo 'hello';",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "extracted2");
}

#[test]
fn generates_base_name_when_no_conflict() {
    let content = "<?php\nfunction foo() {}\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Function,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: String::new(),
        sibling_method_names: Vec::new(),
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "",
        return_strategy: &ReturnStrategy::None,
        body_text: "$x = 1;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "extracted");
}

#[test]
fn name_guard_from_void_guards() {
    let content = "<?php\nclass Foo { function run() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "run".to_string(),
        sibling_method_names: vec!["run".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "run",
        return_strategy: &ReturnStrategy::VoidGuards,
        body_text: "if (!$x) return;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "runGuard");
}

#[test]
fn name_guard_dedup_against_class() {
    let content = "<?php\nclass Foo { function run() {} function runGuard() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "run".to_string(),
        sibling_method_names: vec!["run".to_string(), "runGuard".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "run",
        return_strategy: &ReturnStrategy::VoidGuards,
        body_text: "if (!$x) return;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "runGuard2");
}

#[test]
fn name_try_from_sentinel_null() {
    let content = "<?php\nclass Foo { function fetch() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "fetch".to_string(),
        sibling_method_names: vec!["fetch".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "fetch",
        return_strategy: &ReturnStrategy::SentinelNull,
        body_text: "return $result;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "tryFetch");
}

#[test]
fn name_factory_from_trailing_return() {
    let content = "<?php\nclass Foo { function build() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "build".to_string(),
        sibling_method_names: vec!["build".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "build",
        return_strategy: &ReturnStrategy::TrailingReturn,
        body_text: "$u = new User('Alice');\nreturn $u;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    // Variable `$u` is too short (≤2 chars) → falls back to class name
    assert_eq!(name, "createUser");
}

#[test]
fn name_ends_with_output() {
    let content = "<?php\nclass Foo { function process() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "process".to_string(),
        sibling_method_names: vec!["process".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "process",
        return_strategy: &ReturnStrategy::None,
        body_text: "$first = $users->first();\necho $first->name;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "renderProcess");
}

#[test]
fn name_single_method_call() {
    let content = "<?php\nclass Foo { function run() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "run".to_string(),
        sibling_method_names: vec!["run".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "run",
        return_strategy: &ReturnStrategy::None,
        body_text: "$this->execute($fn);",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "execute");
}

#[test]
fn name_single_function_call() {
    let content = "<?php\nfunction foo() {}\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Function,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "foo".to_string(),
        sibling_method_names: Vec::new(),
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "foo",
        return_strategy: &ReturnStrategy::None,
        body_text: "doSomething($x);",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "doSomething");
}

#[test]
fn name_single_call_with_assignment_is_not_detected() {
    // `$result = $this->execute($fn)` is an assignment, not a
    // pure delegation — should fall through.
    let content = "<?php\nclass Foo { function run() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "run".to_string(),
        sibling_method_names: vec!["run".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "run",
        return_strategy: &ReturnStrategy::None,
        body_text: "$result = $this->execute($fn);",
        return_var_names: &["$result".to_string()],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    // Single return var → computeResult (not "execute")
    assert_eq!(name, "computeResult");
}

#[test]
fn name_factory_prefers_assigned_over_nested() {
    // `new User('Alice')` is an argument to ->add(), not the thing
    // being constructed.  The variable `$users` is what gets
    // returned, so the name should be `createUsers`.
    let content = "<?php\nclass Foo { function getUsers() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "getUsers".to_string(),
        sibling_method_names: vec!["getUsers".to_string()],
    };
    let trailing_rt = PhpType::parse("Collection");
    let naming = NamingContext {
        enclosing_name: "getUsers",
        return_strategy: &ReturnStrategy::TrailingReturn,
        body_text: "$users = new Collection();\n$users->add(new User('Alice'));\nreturn $users;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "createUsers");
}

#[test]
fn name_factory_prefers_return_new_over_assignment() {
    // `return new Product(…)` is a direct return — no variable to
    // take a name from, so the class name is used.
    let content = "<?php\nclass Foo { function build() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "build".to_string(),
        sibling_method_names: vec!["build".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "build",
        return_strategy: &ReturnStrategy::TrailingReturn,
        body_text: "$tmp = new Builder();\nreturn new Product($tmp);",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "createProduct");
}

#[test]
fn name_factory_direct_return_new_uses_class_name() {
    // `return new User(…)` with no variable — class name is used.
    let content = "<?php\nclass Foo { function make() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "make".to_string(),
        sibling_method_names: vec!["make".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "make",
        return_strategy: &ReturnStrategy::TrailingReturn,
        body_text: "return new User('Alice');",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "createUser");
}

#[test]
fn name_render_from_pure_output() {
    let content = "<?php\nclass Foo { function show() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "show".to_string(),
        sibling_method_names: vec!["show".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "show",
        return_strategy: &ReturnStrategy::None,
        body_text: "echo $name;\necho $age;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "renderShow");
}

#[test]
fn name_compute_from_single_return_var() {
    let content = "<?php\nfunction calc() {}\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Function,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "calc".to_string(),
        sibling_method_names: Vec::new(),
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "calc",
        return_strategy: &ReturnStrategy::None,
        body_text: "$total = $a + $b;",
        return_var_names: &["$total".to_string()],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "computeTotal");
}

#[test]
fn name_method_dedup_scoped_to_class() {
    // "extracted" exists as a function elsewhere in the file, but
    // the class has no method called "extracted" → no dedup needed.
    let content = "<?php\nfunction extracted() {}\nclass Foo { function run() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 50,
        is_static: false,
        enclosing_name: String::new(),
        sibling_method_names: vec!["run".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "",
        return_strategy: &ReturnStrategy::None,
        body_text: "$x = 1;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "extracted");
}

#[test]
fn name_trailing_return_with_return_type() {
    let content = "<?php\nclass Foo { function getUsers() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "getUsers".to_string(),
        sibling_method_names: vec!["getUsers".to_string()],
    };
    let trailing_rt = PhpType::parse("Collection");
    let naming = NamingContext {
        enclosing_name: "getUsers",
        return_strategy: &ReturnStrategy::TrailingReturn,
        body_text: "$users = query();\nreturn $users;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "getCollection");
}

#[test]
fn name_uniform_guards() {
    let content = "<?php\nclass Foo { function validate() {} }\n";
    let ctx = EnclosingContext {
        target: ExtractionTarget::Method,
        insert_offset: content.len(),
        body_start: 20,
        is_static: false,
        enclosing_name: "validate".to_string(),
        sibling_method_names: vec!["validate".to_string()],
    };
    let trailing_rt = PhpType::untyped();
    let naming = NamingContext {
        enclosing_name: "validate",
        return_strategy: &ReturnStrategy::UniformGuards("false".to_string()),
        body_text: "if (!$x) return false;",
        return_var_names: &[],
        trailing_return_type: &trailing_rt,
    };
    let name = generate_function_name(content, &ctx, &naming);
    assert_eq!(name, "validateGuard");
}

// ── Call site generation ────────────────────────────────────────

#[test]
fn call_site_no_returns() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$x".to_string(), PhpType::parse("int"))],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "    ");
    assert_eq!(result, "    extracted($x);\n");
}

#[test]
fn call_site_single_return() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$x".to_string(), PhpType::parse("int"))],
        returns: vec![("$result".to_string(), PhpType::parse("int"))],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: "    ".to_string(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "    ");
    assert_eq!(result, "    $result = extracted($x);\n");
}

#[test]
fn call_site_multiple_returns() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![],
        returns: vec![
            ("$a".to_string(), PhpType::untyped()),
            ("$b".to_string(), PhpType::untyped()),
        ],
        body: String::new(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: String::new(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "    ");
    assert_eq!(result, "    [$a, $b] = extracted();\n");
}

#[test]
fn call_site_method() {
    let info = ExtractionInfo {
        name: "runGuard".to_string(),
        params: vec![("$x".to_string(), PhpType::parse("int"))],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "        ");
    assert_eq!(result, "        $this->runGuard($x);\n");
}

#[test]
fn call_site_static_method() {
    let info = ExtractionInfo {
        name: "computeTotal".to_string(),
        params: vec![],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Method,
        is_static: true,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "        ");
    assert_eq!(result, "        self::computeTotal();\n");
}

#[test]
fn call_site_trailing_return() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$x".to_string(), PhpType::parse("int"))],
        returns: vec![],
        body: "return $x * 2;".to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::TrailingReturn,
        trailing_return_type: PhpType::parse("int"),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "        ");
    assert_eq!(result, "        return $this->extracted($x);\n");
}

#[test]
fn call_site_void_guards() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$x".to_string(), PhpType::untyped())],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::VoidGuards,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "        ");
    assert_eq!(result, "        if (!$this->extracted($x)) return;\n");
}

#[test]
fn call_site_uniform_false_guards() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$x".to_string(), PhpType::untyped())],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::UniformGuards("false".to_string()),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "        ");
    assert_eq!(result, "        if (!$this->extracted($x)) return false;\n");
}

#[test]
fn call_site_sentinel_null() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$x".to_string(), PhpType::untyped())],
        returns: vec![],
        body: String::new(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::SentinelNull,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "        ");
    assert_eq!(
        result,
        "        $result = $this->extracted($x);\n        if ($result !== null) return $result;\n"
    );
}

#[test]
fn call_site_null_guard_with_value() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$obj".to_string(), PhpType::untyped())],
        returns: vec![("$sound".to_string(), PhpType::parse("string"))],
        body: String::new(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::NullGuardWithValue(false),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "        ");
    assert_eq!(
        result,
        "        $sound = $this->extracted($obj);\n        if ($sound === null) return null;\n"
    );
}

#[test]
fn call_site_void_guard_with_value() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$obj".to_string(), PhpType::untyped())],
        returns: vec![("$sound".to_string(), PhpType::parse("string"))],
        body: String::new(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::NullGuardWithValue(true),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_call_site(&info, "        ");
    assert_eq!(
        result,
        "        $sound = $this->extracted($obj);\n        if ($sound === null) return;\n"
    );
}

// ── Definition generation ───────────────────────────────────────

#[test]
fn definition_method_no_params_void() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![],
        returns: vec![],
        body: "        echo 'hello';\n".to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains("private function extracted(): void"),
        "got: {result}"
    );
    assert!(result.contains("echo 'hello';"), "got: {result}");
}

#[test]
fn definition_function_with_params_and_return() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$x".to_string(), PhpType::parse("int"))],
        returns: vec![("$result".to_string(), PhpType::parse("string"))],
        body: "$result = strval($x);".to_string(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: "    ".to_string(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains("function extracted(int $x): string"),
        "got: {result}"
    );
    assert!(result.contains("return $result;"), "got: {result}");
}

#[test]
fn definition_static_method() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$x".to_string(), PhpType::parse("int"))],
        returns: vec![],
        body: "        echo $x;\n".to_string(),
        target: ExtractionTarget::Method,
        is_static: true,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains("private static function extracted(int $x): void"),
        "got: {result}"
    );
}

#[test]
fn definition_with_trailing_return() {
    let info = ExtractionInfo {
        name: "extracted".to_string(),
        params: vec![("$x".to_string(), PhpType::parse("int"))],
        returns: vec![],
        body: "        return $x * 2;\n".to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::TrailingReturn,
        trailing_return_type: PhpType::parse("int"),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains("private function extracted(int $x): int"),
        "should carry enclosing return type: {result}"
    );
    // Body already contains the return — no extra return appended.
    assert!(
        result.contains("return $x * 2;"),
        "body should keep the return statement: {result}"
    );
    // Should not have a duplicate return.
    assert_eq!(
        result.matches("return").count(),
        1,
        "should have exactly one return: {result}"
    );
}

#[test]
fn definition_void_guards_appends_return_true() {
    let info = ExtractionInfo {
        name: "validate".to_string(),
        params: vec![("$x".to_string(), PhpType::untyped())],
        returns: vec![],
        body: "if (!$x) return;".to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::VoidGuards,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains(": bool"),
        "should have bool return type: {result}"
    );
    assert!(
        result.contains("return true;"),
        "should append return true as fall-through: {result}"
    );
}

#[test]
fn definition_uniform_false_guards_appends_return_true() {
    let info = ExtractionInfo {
        name: "validate".to_string(),
        params: vec![("$x".to_string(), PhpType::untyped())],
        returns: vec![],
        body: "if (!$x) return false;".to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::UniformGuards("false".to_string()),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains(": bool"),
        "should have bool return type: {result}"
    );
    assert!(
        result.contains("return true;"),
        "should append return true (inverse of false) as sentinel: {result}"
    );
}

#[test]
fn definition_uniform_true_guards_appends_return_false() {
    let info = ExtractionInfo {
        name: "validate".to_string(),
        params: vec![("$x".to_string(), PhpType::untyped())],
        returns: vec![],
        body: "if (!$x) return true;".to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::UniformGuards("true".to_string()),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains("return false;"),
        "should append return false (inverse of true) as sentinel: {result}"
    );
}

#[test]
fn definition_sentinel_null_appends_return_null() {
    let info = ExtractionInfo {
        name: "classify".to_string(),
        params: vec![("$x".to_string(), PhpType::untyped())],
        returns: vec![],
        body: "if ($x < 0) return 'negative';".to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::SentinelNull,
        trailing_return_type: PhpType::parse("string"),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains(": ?string"),
        "should have nullable return type: {result}"
    );
    assert!(
        result.contains("return null;"),
        "should append return null as sentinel: {result}"
    );
}

#[test]
fn definition_null_guard_with_value_appends_return_variable() {
    let info = ExtractionInfo {
        name: "getSound".to_string(),
        params: vec![],
        returns: vec![("$sound".to_string(), PhpType::parse("string"))],
        body: "        if ($this->muted) return null;\n        $sound = $this->makeSound();\n"
            .to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::NullGuardWithValue(false),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains(": ?string"),
        "should have nullable return type: {result}"
    );
    assert!(
        result.contains("return $sound;"),
        "should append return $sound as fall-through: {result}"
    );
    assert!(
        result.contains("return null;"),
        "should keep the guard's return null: {result}"
    );
}

#[test]
fn definition_void_guard_with_value_rewrites_returns() {
    // Void guards + return value: bare `return;` → `return null;`
    let info = ExtractionInfo {
        name: "getSound".to_string(),
        params: vec![],
        returns: vec![("$sound".to_string(), PhpType::parse("string"))],
        body: "        if ($this->muted) return;\n        $sound = $this->makeSound();\n"
            .to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::NullGuardWithValue(true),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains(": ?string"),
        "should have nullable return type: {result}"
    );
    assert!(
        result.contains("return $sound;"),
        "should append return $sound as fall-through: {result}"
    );
    // Bare `return;` should be rewritten to `return null;`.
    assert!(
        result.contains("return null;"),
        "void guard should be rewritten to return null: {result}"
    );
    // Should NOT contain bare `return;`.
    assert_eq!(
        result.matches("return;").count(),
        0,
        "should not contain bare return: {result}"
    );
}

// ── Void return rewriting ───────────────────────────────────────

#[test]
fn rewrite_void_returns_to_null_basic() {
    let body = "if (!$x) return;\nif (!$y) return;";
    let result = rewrite_void_returns_to_null(body);
    assert_eq!(result, "if (!$x) return null;\nif (!$y) return null;");
}

#[test]
fn rewrite_void_returns_to_null_preserves_valued_returns() {
    let body = "if (!$x) return;\nreturn $result;";
    let result = rewrite_void_returns_to_null(body);
    assert_eq!(result, "if (!$x) return null;\nreturn $result;");
}

#[test]
fn rewrite_void_returns_to_null_ignores_identifiers() {
    let body = "$returnValue = 1;\nif (!$x) return;";
    let result = rewrite_void_returns_to_null(body);
    assert_eq!(result, "$returnValue = 1;\nif (!$x) return null;");
}

// ── Guard return rewriting ──────────────────────────────────────

#[test]
fn rewrite_void_guards_to_false() {
    let body = "if (!$x) return;\nif (!$y) return;";
    let result = rewrite_guard_returns(body, None);
    assert_eq!(result, "if (!$x) return false;\nif (!$y) return false;");
}

#[test]
fn rewrite_void_guards_preserves_non_bare_returns() {
    let body = "if (!$x) return;\nreturn $result;";
    let result = rewrite_guard_returns(body, None);
    assert_eq!(
        result, "if (!$x) return false;\nreturn $result;",
        "should only rewrite bare returns"
    );
}

#[test]
fn rewrite_void_guards_ignores_return_in_identifiers() {
    let body = "$returnValue = 1;\nif (!$x) return;";
    let result = rewrite_guard_returns(body, None);
    assert_eq!(result, "$returnValue = 1;\nif (!$x) return false;");
}

#[test]
fn rewrite_uniform_null_to_false() {
    let body = "if ($id <= 0) return null;\nif (!$org) return null;";
    let result = rewrite_guard_returns(body, Some("null"));
    assert_eq!(
        result,
        "if ($id <= 0) return false;\nif (!$org) return false;"
    );
}

#[test]
fn rewrite_uniform_value_preserves_other_returns() {
    let body = "if ($id <= 0) return null;\nreturn $result;";
    let result = rewrite_guard_returns(body, Some("null"));
    assert_eq!(
        result, "if ($id <= 0) return false;\nreturn $result;",
        "should only rewrite matching return values"
    );
}

#[test]
fn rewrite_uniform_numeric_to_false() {
    let body = "if ($x < 0) return 0;\nif ($x > 100) return 0;";
    let result = rewrite_guard_returns(body, Some("0"));
    assert_eq!(
        result,
        "if ($x < 0) return false;\nif ($x > 100) return false;"
    );
}

#[test]
fn void_guards_definition_rewrites_body() {
    // End-to-end: the definition should contain `return false;`
    // for the guards and `return true;` for the fall-through.
    let info = ExtractionInfo {
        name: "validate".to_string(),
        params: vec![("$x".to_string(), PhpType::untyped())],
        returns: vec![],
        body: "if (!$x) return;\nif (!$y) return;".to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::VoidGuards,
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains("return false;"),
        "guards should be rewritten to return false: {result}"
    );
    assert!(
        result.contains("return true;"),
        "fall-through should be return true: {result}"
    );
    // Should NOT contain bare `return;` (the original void return).
    let bare_return_count = result.matches("return;").count();
    assert_eq!(
        bare_return_count, 0,
        "should not contain bare return: {result}"
    );
}

#[test]
fn uniform_null_definition_rewrites_body() {
    // `return null;` guards should become `return false;` in the
    // extracted function since the return type is bool.
    let info = ExtractionInfo {
        name: "validate".to_string(),
        params: vec![("$id".to_string(), PhpType::untyped())],
        returns: vec![],
        body: "if ($id <= 0) return null;\nif (!$this->exists($id)) return null;".to_string(),
        target: ExtractionTarget::Method,
        is_static: false,
        member_indent: "    ".to_string(),
        body_indent: "        ".to_string(),
        return_strategy: ReturnStrategy::UniformGuards("null".to_string()),
        trailing_return_type: PhpType::untyped(),
        docblock: String::new(),
    };
    let result = build_extracted_definition(&info);
    assert!(
        result.contains("return false;"),
        "null guards should be rewritten to return false: {result}"
    );
    assert!(
        result.contains("return true;"),
        "fall-through should be return true: {result}"
    );
    // Should NOT contain `return null;`.
    let null_return_count = result.matches("return null;").count();
    assert_eq!(
        null_return_count, 0,
        "should not contain return null: {result}"
    );
}

// ── Integration: code action on Backend ─────────────────────────

#[test]
fn extract_function_action_offered_for_complete_statements() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
    $y = $x + 2;
    echo $y;
}
";
    // Select `$x = 1;\n    $y = $x + 2;`
    let start_line = 2; // `    $x = 1;`
    let end_line = 3; // `    $y = $x + 2;`

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(start_line, 4),
            end: Position::new(end_line, 16),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_action = actions
            .iter()
            .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Extract function")));
    assert!(
        extract_action.is_some(),
        "should offer extract function action, got: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                CodeActionOrCommand::Command(cmd) => cmd.title.clone(),
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn extract_function_not_offered_for_empty_selection() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
}
";
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 4),
            end: Position::new(2, 4), // empty selection
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Extract function") || ca.title.starts_with("Extract method")))
            .collect();
    assert!(
        extract_actions.is_empty(),
        "should not offer extract for empty selection"
    );
}

#[test]
fn extract_function_not_offered_for_partial_statement() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1 + 2;
}
";
    // Select just `1 + 2` — not a complete statement.
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 9),
            end: Position::new(2, 14),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_actions: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Extract function") || ca.title.starts_with("Extract method")))
            .collect();
    assert!(
        extract_actions.is_empty(),
        "should not offer extract for partial statement"
    );
}

#[test]
fn extract_method_offered_when_using_this() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private int $value = 0;

    public function bar() {
        $x = $this->value;
        echo $x;
    }
}
";
    // Select `$x = $this->value;\n        echo $x;`
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(5, 8),
            end: Position::new(6, 16),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_method = actions
            .iter()
            .find(|a| matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Extract method")));
    assert!(
        extract_method.is_some(),
        "should offer extract method when $this is used, got: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                CodeActionOrCommand::Command(cmd) => cmd.title.clone(),
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn extract_function_offered_for_trailing_return() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
    return $x;
}
";
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 4),
            end: Position::new(3, 14),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_action = actions.iter().find(|a| {
            matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Extract function") || ca.title.starts_with("Extract method"))
        });
    assert!(
        extract_action.is_some(),
        "should offer extract when return is the last selected statement"
    );
}

#[test]
fn extract_function_offered_for_guard_clause_return() {
    // Non-trailing returns that form guard clauses should now be
    // offered with the appropriate guard strategy.
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo($x) {
    if ($x) {
        return 1;
    }
    echo 'done';
}
";
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 4),
            end: Position::new(5, 17),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract_action = actions.iter().find(|a| {
            matches!(a, CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Extract function") || ca.title.starts_with("Extract method"))
        });
    assert!(
        extract_action.is_some(),
        "should offer extract for guard clause return pattern, got: {:?}",
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                CodeActionOrCommand::Command(cmd) => cmd.title.clone(),
            })
            .collect::<Vec<_>>()
    );
}

// ── Indent detection ────────────────────────────────────────────

#[test]
fn detect_indent_unit_spaces() {
    let content = "<?php\n    function foo() {\n        $x = 1;\n    }\n";
    assert_eq!(detect_indent_unit(content), "    ");
}

#[test]
fn detect_indent_unit_tabs() {
    let content = "<?php\n\tfunction foo() {\n\t\t$x = 1;\n\t}\n";
    assert_eq!(detect_indent_unit(content), "\t");
}

#[test]
fn indent_at_line() {
    let content = "<?php\n    $x = 1;\n";
    let offset = content.find("$x").unwrap();
    assert_eq!(indent_at(content, offset), "    ");
}

#[test]
fn detect_line_indent_method() {
    let content = "<?php\nclass Foo {\n    public function bar() {\n        $x = 1;\n    }\n}\n";
    // body_start is the `{` after `bar()`
    let offset = content.find("{\n        $x").unwrap();
    assert_eq!(detect_line_indent(content, offset), "    ");
}

// ── Extraction context ──────────────────────────────────────────

#[test]
fn detects_function_context() {
    let content = "<?php\nfunction foo() {\n    $x = 1;\n}\n";
    let offset = content.find("$x").unwrap() as u32;
    let ctx = find_enclosing_context(content, offset, false);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.target, ExtractionTarget::Function);
}

#[test]
fn detects_method_context() {
    let content = "<?php\nclass Foo {\n    public function bar() {\n        $x = 1;\n    }\n}\n";
    let offset = content.find("$x").unwrap() as u32;
    let ctx = find_enclosing_context(content, offset, false);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.target, ExtractionTarget::Method);
}

#[test]
fn detects_method_context_with_this() {
    let content =
        "<?php\nclass Foo {\n    public function bar() {\n        $this->baz();\n    }\n}\n";
    let offset = content.find("$this").unwrap() as u32;
    let ctx = find_enclosing_context(content, offset, true);
    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.target, ExtractionTarget::Method);
}

// ── PHPDoc generation on extracted method ───────────────────────

fn no_classes(_name: &str) -> Option<Arc<ClassInfo>> {
    None
}

#[test]
fn docblock_not_generated_for_scalar_types() {
    let params = vec![
        (
            "$x".to_string(),
            PhpType::parse("int"),
            PhpType::parse("int"),
        ),
        (
            "$y".to_string(),
            PhpType::parse("string"),
            PhpType::parse("string"),
        ),
    ];
    let result = build_docblock_for_extraction(
        &params,
        &PhpType::parse("void"),
        &PhpType::parse("void"),
        "    ",
        &no_classes,
    );
    assert!(
        result.is_empty(),
        "scalar types should not trigger docblock, got: {result}"
    );
}

#[test]
fn docblock_generated_for_array_param() {
    let params = vec![(
        "$items".to_string(),
        PhpType::parse("array"),
        PhpType::parse("array"),
    )];
    let result = build_docblock_for_extraction(
        &params,
        &PhpType::parse("void"),
        &PhpType::parse("void"),
        "    ",
        &no_classes,
    );
    assert!(
        result.contains("@param"),
        "array param should trigger @param enrichment, got: {result}"
    );
    assert!(result.contains("$items"));
    assert!(result.starts_with("    /**"));
    assert!(result.contains("     */"));
}

#[test]
fn docblock_generated_for_callable_param() {
    let params = vec![(
        "$fn".to_string(),
        PhpType::parse("Closure"),
        PhpType::parse("Closure"),
    )];
    let result = build_docblock_for_extraction(
        &params,
        &PhpType::parse("void"),
        &PhpType::parse("void"),
        "    ",
        &no_classes,
    );
    assert!(
        result.contains("@param"),
        "Closure param should trigger @param enrichment, got: {result}"
    );
    assert!(result.contains("$fn"));
}

#[test]
fn docblock_not_generated_for_empty_types() {
    let params = vec![("$x".to_string(), PhpType::untyped(), PhpType::untyped())];
    let result = build_docblock_for_extraction(
        &params,
        &PhpType::untyped(),
        &PhpType::untyped(),
        "",
        &no_classes,
    );
    assert!(
        result.is_empty(),
        "empty types should not trigger docblock, got: {result}"
    );
}

#[test]
fn docblock_aligns_param_names() {
    let params = vec![
        (
            "$items".to_string(),
            PhpType::parse("array"),
            PhpType::parse("array<string, User>"),
        ),
        (
            "$x".to_string(),
            PhpType::parse("Closure"),
            PhpType::parse("Closure"),
        ),
    ];
    let result = build_docblock_for_extraction(
        &params,
        &PhpType::parse("void"),
        &PhpType::parse("void"),
        "",
        &no_classes,
    );
    // Both @param tags should be present.
    let param_lines: Vec<&str> = result.lines().filter(|l| l.contains("@param")).collect();
    assert_eq!(
        param_lines.len(),
        2,
        "expected 2 @param lines, got: {result}"
    );
    // The $-names should be aligned (both start at the same column).
    let dollar_positions: Vec<usize> = param_lines.iter().map(|l| l.find('$').unwrap()).collect();
    assert_eq!(
        dollar_positions[0], dollar_positions[1],
        "param names should be aligned, got: {result}"
    );
}

#[test]
fn docblock_return_type_hint_for_docblock_trailing() {
    let result = build_return_type_hint_for_docblock(
        &ReturnStrategy::TrailingReturn,
        &PhpType::parse("string"),
        &[],
    );
    assert_eq!(result, PhpType::parse("string"));
}

#[test]
fn docblock_return_type_hint_for_docblock_void_guards() {
    let result =
        build_return_type_hint_for_docblock(&ReturnStrategy::VoidGuards, &PhpType::untyped(), &[]);
    assert_eq!(result, PhpType::parse("bool"));
}

#[test]
fn docblock_return_type_hint_for_docblock_none_void() {
    let result =
        build_return_type_hint_for_docblock(&ReturnStrategy::None, &PhpType::untyped(), &[]);
    assert_eq!(result, PhpType::parse("void"));
}

#[test]
fn docblock_return_type_hint_for_docblock_single_return() {
    let returns = vec![(
        "$x".to_string(),
        PhpType::parse("array"),
        PhpType::parse("array"),
    )];
    let result =
        build_return_type_hint_for_docblock(&ReturnStrategy::None, &PhpType::untyped(), &returns);
    assert_eq!(result, PhpType::parse("array"));
}

#[test]
fn definition_includes_docblock_for_array_param() {
    let info = ExtractionInfo {
        name: "process".to_string(),
        params: vec![("$items".to_string(), PhpType::parse("array"))],
        returns: vec![],
        body: "foreach ($items as $item) {}".to_string(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: "    ".to_string(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: build_docblock_for_extraction(
            &[(
                "$items".to_string(),
                PhpType::parse("array"),
                PhpType::parse("array"),
            )],
            &PhpType::parse("void"),
            &PhpType::parse("void"),
            "",
            &no_classes,
        ),
    };
    let def = build_extracted_definition(&info);
    assert!(
        def.contains("/**"),
        "definition should include docblock for array param, got:\n{def}"
    );
    assert!(
        def.contains("@param"),
        "definition should include @param tag, got:\n{def}"
    );
    // Docblock should appear before the function keyword.
    let doc_pos = def.find("/**").unwrap();
    let fn_pos = def.find("function").unwrap();
    assert!(doc_pos < fn_pos, "docblock should precede function keyword");
}

#[test]
fn definition_no_docblock_for_scalar_params() {
    let info = ExtractionInfo {
        name: "add".to_string(),
        params: vec![
            ("$a".to_string(), PhpType::parse("int")),
            ("$b".to_string(), PhpType::parse("int")),
        ],
        returns: vec![("$sum".to_string(), PhpType::parse("int"))],
        body: "$sum = $a + $b;".to_string(),
        target: ExtractionTarget::Function,
        is_static: false,
        member_indent: String::new(),
        body_indent: "    ".to_string(),
        return_strategy: ReturnStrategy::None,
        trailing_return_type: PhpType::untyped(),
        docblock: build_docblock_for_extraction(
            &[
                (
                    "$a".to_string(),
                    PhpType::parse("int"),
                    PhpType::parse("int"),
                ),
                (
                    "$b".to_string(),
                    PhpType::parse("int"),
                    PhpType::parse("int"),
                ),
            ],
            &PhpType::parse("int"),
            &PhpType::parse("int"),
            "",
            &no_classes,
        ),
    };
    let def = build_extracted_definition(&info);
    assert!(
        !def.contains("/**"),
        "definition should NOT include docblock for scalar types, got:\n{def}"
    );
}

// ── Disabled code action with rejection reason ──────────────────

#[test]
fn unsafe_returns_resolve_produces_no_edit() {
    // Phase 1 no longer emits disabled actions (validation is
    // deferred to resolve).  Instead it offers a normal action
    // and resolve returns None when the return strategy is unsafe.
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    if ($a) return 1;
    if ($b) return null;
    echo 'done';
}
";
    backend
        .open_files
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(content.to_string()));

    // Select the three statements (mixed return values including
    // null → Unsafe strategy).
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 4),
            end: Position::new(4, 17),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let extract = actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca)
            if ca.kind == Some(CodeActionKind::REFACTOR_EXTRACT)
                && ca.title.contains("Extract") =>
        {
            Some(ca)
        }
        _ => None,
    });
    assert!(
        extract.is_some(),
        "Phase 1 should still offer the action (validation deferred to resolve)"
    );

    let action = extract.unwrap();
    assert!(action.edit.is_none(), "Phase 1 should not have an edit");
    assert!(
        action.data.is_some(),
        "Phase 1 should have data for resolve"
    );

    // Phase 2: resolve should produce no edit because the return
    // strategy is unsafe.
    let (resolved, _) = backend.resolve_code_action(action.clone());
    assert!(
        resolved.edit.is_none(),
        "resolve should produce no edit for unsafe returns"
    );
}

#[test]
fn no_disabled_action_for_empty_selection() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = 1;
}
";
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 4),
            end: Position::new(2, 4), // empty selection
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let disabled_extract = actions.iter().find(|a| {
        matches!(a, CodeActionOrCommand::CodeAction(ca)
                if ca.disabled.is_some()
                    && ca.kind == Some(CodeActionKind::REFACTOR_EXTRACT)
                    && ca.title.contains("Extract"))
    });
    assert!(
        disabled_extract.is_none(),
        "should NOT emit a disabled extract action for empty selection"
    );
}

#[test]
fn no_disabled_action_for_partial_statement() {
    let backend = crate::Backend::new_test();
    let uri = "file:///test.php";
    let content = "\
<?php
function foo() {
    $x = some_function($a, $b);
}
";
    // Select partial statement (just the function call, not the assignment).
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(2, 9),
            end: Position::new(2, 30),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };

    let actions = backend.handle_code_action(uri, content, &params);
    let disabled_extract = actions.iter().find(|a| {
        matches!(a, CodeActionOrCommand::CodeAction(ca)
                if ca.disabled.is_some()
                    && ca.kind == Some(CodeActionKind::REFACTOR_EXTRACT)
                    && ca.title.contains("Extract"))
    });
    assert!(
        disabled_extract.is_none(),
        "should NOT emit a disabled extract action for partial statement"
    );
}
