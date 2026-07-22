use super::*;

/// Helper: detect at a given line/character.
fn detect(content: &str, line: u32, character: u32) -> Option<TypeHintContext> {
    detect_type_hint_context(content, Position { line, character })
}

// ── Function parameter type hints ───────────────────────────────

#[test]
fn after_open_paren_in_function() {
    let src = "<?php\nfunction foo(Us) {}";
    let ctx = detect(src, 1, 15).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn empty_after_open_paren() {
    let src = "<?php\nfunction foo() {}";
    // cursor right after `(`
    let ctx = detect(src, 1, 13);
    assert!(ctx.is_some());
    assert_eq!(ctx.unwrap().partial, "");
}

#[test]
fn after_comma_in_function_params() {
    let src = "<?php\nfunction foo(string $a, Us) {}";
    let ctx = detect(src, 1, 26).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn after_comma_empty_partial() {
    let src = "<?php\nfunction foo(string $a, ) {}";
    let ctx = detect(src, 1, 24);
    assert!(ctx.is_some());
    assert_eq!(ctx.unwrap().partial, "");
}

#[test]
fn not_after_comma_incomplete_param() {
    // The first param has no $variable yet — the user is still typing
    // the type, so the comma doesn't indicate a new param type position.
    let src = "<?php\nfunction foo(string,) {}";
    let ctx = detect(src, 1, 20);
    assert!(ctx.is_none());
}

// ── Return type hints ───────────────────────────────────────────

#[test]
fn return_type_after_colon() {
    let src = "<?php\nfunction foo(): Us {}";
    let ctx = detect(src, 1, 18).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn return_type_empty() {
    let src = "<?php\nfunction foo():  {}";
    // cursor right after `: `
    let ctx = detect(src, 1, 16);
    assert!(ctx.is_some());
    assert_eq!(ctx.unwrap().partial, "");
}

// ── Nullable / union / intersection modifiers ───────────────────

#[test]
fn nullable_param_type() {
    let src = "<?php\nfunction foo(?Us) {}";
    let ctx = detect(src, 1, 16).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn union_param_type() {
    let src = "<?php\nfunction foo(string|Us) {}";
    let ctx = detect(src, 1, 22).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn intersection_param_type() {
    let src = "<?php\nfunction foo(A&Us) {}";
    let ctx = detect(src, 1, 17).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn union_return_type() {
    let src = "<?php\nfunction foo(): string|Us {}";
    let ctx = detect(src, 1, 25).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn nullable_return_type() {
    let src = "<?php\nfunction foo(): ?Us {}";
    let ctx = detect(src, 1, 19).unwrap();
    assert_eq!(ctx.partial, "Us");
}

// ── Method definitions ──────────────────────────────────────────

#[test]
fn method_param_type() {
    let src = "<?php\nclass Foo {\n    public function bar(Us) {}\n}";
    let ctx = detect(src, 2, 26).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn method_return_type() {
    let src = "<?php\nclass Foo {\n    public function bar(): Us {}\n}";
    let ctx = detect(src, 2, 29).unwrap();
    assert_eq!(ctx.partial, "Us");
}

// ── Property type hints ─────────────────────────────────────────

#[test]
fn property_after_public() {
    let src = "<?php\nclass Foo {\n    public Us\n}";
    let ctx = detect(src, 2, 13).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn no_type_hint_immediately_after_public_modifier() {
    let src = "<?php\nclass Foo {\n    public \n}";
    let ctx = detect(src, 2, 11);
    assert!(
        ctx.is_none(),
        "Empty partial right after `public` should defer to keyword completion"
    );
}

#[test]
fn property_after_private_readonly() {
    let src = "<?php\nclass Foo {\n    private readonly Us\n}";
    let ctx = detect(src, 2, 23).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn property_after_protected_static() {
    let src = "<?php\nclass Foo {\n    protected static Us\n}";
    let ctx = detect(src, 2, 23).unwrap();
    assert_eq!(ctx.partial, "Us");
}

// ── Promoted constructor parameters ─────────────────────────────

#[test]
fn promoted_param_after_modifier() {
    let src = "<?php\nclass Foo {\n    public function __construct(private Us) {}\n}";
    let ctx = detect(src, 2, 42).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn promoted_param_after_readonly() {
    let src = "<?php\nclass Foo {\n    public function __construct(private readonly Us) {}\n}";
    let ctx = detect(src, 2, 51).unwrap();
    assert_eq!(ctx.partial, "Us");
}

// ── Closures and arrow functions ────────────────────────────────

#[test]
fn closure_param_type() {
    let src = "<?php\n$f = function(Us) {};";
    let ctx = detect(src, 1, 16).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn arrow_fn_param_type() {
    let src = "<?php\n$f = fn(Us) => null;";
    let ctx = detect(src, 1, 10).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn closure_return_type() {
    let src = "<?php\n$f = function(): Us {};";
    let ctx = detect(src, 1, 19).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn arrow_fn_return_type() {
    let src = "<?php\n$f = fn(): Us => null;";
    let ctx = detect(src, 1, 13).unwrap();
    assert_eq!(ctx.partial, "Us");
}

// ── Multi-line function definitions ─────────────────────────────

#[test]
fn multiline_param_type() {
    let src = "<?php\nfunction foo(\n    string $a,\n    Us\n) {}";
    let ctx = detect(src, 3, 6).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn multiline_after_comma_empty() {
    let src = "<?php\nfunction foo(\n    string $a,\n    \n) {}";
    let ctx = detect(src, 3, 4);
    assert!(ctx.is_some());
    assert_eq!(ctx.unwrap().partial, "");
}

// ── Negative cases: should NOT detect ───────────────────────────

#[test]
fn not_in_function_call() {
    let src = "<?php\nfoo(Us);";
    let ctx = detect(src, 1, 6);
    assert!(ctx.is_none());
}

#[test]
fn not_in_method_call() {
    let src = "<?php\n$obj->foo(Us);";
    let ctx = detect(src, 1, 13);
    assert!(ctx.is_none());
}

#[test]
fn not_variable() {
    let src = "<?php\nfunction foo($us) {}";
    let ctx = detect(src, 1, 15);
    assert!(ctx.is_none());
}

#[test]
fn not_member_access() {
    let src = "<?php\n$this->Us";
    let ctx = detect(src, 1, 10);
    assert!(ctx.is_none());
}

#[test]
fn not_static_access() {
    let src = "<?php\nFoo::Us";
    let ctx = detect(src, 1, 8);
    assert!(ctx.is_none());
}

#[test]
fn not_assignment() {
    let src = "<?php\n$x = Us;";
    let ctx = detect(src, 1, 7);
    assert!(ctx.is_none());
}

#[test]
fn not_after_function_keyword() {
    // Typing the function name after `function` should not suggest types.
    let src = "<?php\npublic function Us";
    let ctx = detect(src, 1, 20);
    // `function` is not a modifier keyword, so this should not match.
    assert!(ctx.is_none());
}

#[test]
fn function_name_position_after_function_keyword() {
    use super::is_function_or_const_name_position;
    let src = "<?php\nclass Foo {\n    protected function getC\n}";
    // Cursor after `getC` ("    protected function getC" → len 27)
    assert!(is_function_or_const_name_position(
        src,
        Position {
            line: 2,
            character: 27
        }
    ));
    // Cursor right after `function `
    assert!(is_function_or_const_name_position(
        src,
        Position {
            line: 2,
            character: 23
        }
    ));
}

#[test]
fn function_name_position_not_in_call() {
    use super::is_function_or_const_name_position;
    let src = "<?php\ngetC();";
    assert!(!is_function_or_const_name_position(
        src,
        Position {
            line: 1,
            character: 4
        }
    ));
}

#[test]
fn const_name_position_after_const_keyword() {
    use super::is_function_or_const_name_position;
    let src = "<?php\nclass Foo {\n    public const FO\n}";
    assert!(is_function_or_const_name_position(
        src,
        Position {
            line: 2,
            character: 19
        }
    ));
}

#[test]
fn typed_const_name_position_after_const_type() {
    use super::is_function_or_const_name_position;
    let src = "<?php\nclass Foo {\n    public const string FO\n}";
    assert!(is_function_or_const_name_position(
        src,
        Position {
            line: 2,
            character: 26
        }
    ));
}

#[test]
fn case_name_position_in_enum() {
    use super::is_function_or_const_name_position;
    let src = "<?php\nenum Status {\n    case Pend\n}";
    assert!(is_function_or_const_name_position(
        src,
        Position {
            line: 2,
            character: 13
        }
    ));
}

#[test]
fn property_name_after_dollar_is_not_function_name_position() {
    use super::is_function_or_const_name_position;
    // Property names use `$`; class completion already suppresses on `$`.
    let src = "<?php\nclass Foo {\n    protected $pro\n}";
    assert!(!is_function_or_const_name_position(
        src,
        Position {
            line: 2,
            character: 17
        }
    ));
}

#[test]
fn type_after_visibility_is_not_function_name_position() {
    use super::is_function_or_const_name_position;
    // `protected User` is a type-hint position — classes should still be offered.
    let src = "<?php\nclass Foo {\n    protected Us\n}";
    assert!(!is_function_or_const_name_position(
        src,
        Position {
            line: 2,
            character: 16
        }
    ));
}

#[test]
fn use_const_import_is_not_const_name_position() {
    use super::is_function_or_const_name_position;
    let src = "<?php\nuse const APP_V";
    assert!(!is_function_or_const_name_position(
        src,
        Position {
            line: 1,
            character: 15
        }
    ));
}

#[test]
fn use_function_import_is_not_function_name_position() {
    use super::is_function_or_const_name_position;
    let src = "<?php\nuse function array_m";
    assert!(!is_function_or_const_name_position(
        src,
        Position {
            line: 1,
            character: 22
        }
    ));
}

#[test]
fn partial_is_function_keyword_after_modifier() {
    // `public function` — the partial "function" should be filtered out
    // so we don't offer type hints when the user is typing the keyword.
    let src = "<?php\nclass Foo {\n    public function\n}";
    let ctx = detect(src, 2, 19);
    assert!(ctx.is_none());
}

// ── Partial keyword prefixes after modifiers ────────────────────

#[test]
fn partial_fu_after_public_is_not_type_hint() {
    // `public fu` — "fu" is a prefix of "function", should defer to keyword completion.
    let src = "<?php\nclass Foo {\n    public fu\n}";
    let ctx = detect(src, 2, 13);
    assert!(
        ctx.is_none(),
        "partial 'fu' is a prefix of 'function' and should not trigger type hints"
    );
}

#[test]
fn partial_fun_after_public_is_not_type_hint() {
    let src = "<?php\nclass Foo {\n    public fun\n}";
    let ctx = detect(src, 2, 14);
    assert!(
        ctx.is_none(),
        "partial 'fun' is a prefix of 'function' and should not trigger type hints"
    );
}

#[test]
fn partial_func_after_public_is_not_type_hint() {
    let src = "<?php\nclass Foo {\n    public func\n}";
    let ctx = detect(src, 2, 15);
    assert!(
        ctx.is_none(),
        "partial 'func' is a prefix of 'function' and should not trigger type hints"
    );
}

#[test]
fn partial_con_after_public_is_not_type_hint() {
    // `public con` — "con" is a prefix of "const", should defer to keyword completion.
    let src = "<?php\nclass Foo {\n    public con\n}";
    let ctx = detect(src, 2, 14);
    assert!(
        ctx.is_none(),
        "partial 'con' is a prefix of 'const' and should not trigger type hints"
    );
}

#[test]
fn partial_st_after_public_is_not_type_hint() {
    // `public st` — "st" is a prefix of "static", should defer to keyword completion.
    let src = "<?php\nclass Foo {\n    public st\n}";
    let ctx = detect(src, 2, 13);
    assert!(
        ctx.is_none(),
        "partial 'st' is a prefix of 'static' and should not trigger type hints"
    );
}

#[test]
fn partial_f_after_public_is_not_type_hint() {
    // `public f` — "f" is a prefix of "function", "fn", "final".
    let src = "<?php\nclass Foo {\n    public f\n}";
    let ctx = detect(src, 2, 12);
    assert!(
        ctx.is_none(),
        "partial 'f' is a prefix of 'function'/'fn'/'final' and should not trigger type hints"
    );
}

#[test]
fn partial_cl_after_readonly_at_top_level_is_not_type_hint() {
    // `readonly cl` — "cl" is a prefix of "class", should defer to keyword completion.
    let src = "<?php\nreadonly cl";
    let ctx = detect(src, 1, 12);
    assert!(
        ctx.is_none(),
        "partial 'cl' is a prefix of 'class' and should not trigger type hints"
    );
}

#[test]
fn partial_ab_after_public_is_not_type_hint() {
    // `public ab` — "ab" is a prefix of "abstract".
    let src = "<?php\nclass Foo {\n    public ab\n}";
    let ctx = detect(src, 2, 13);
    assert!(
        ctx.is_none(),
        "partial 'ab' is a prefix of 'abstract' and should not trigger type hints"
    );
}

#[test]
fn partial_str_after_public_is_type_hint() {
    // `public str` — "str" is NOT a prefix of any declaration keyword,
    // so it should trigger type hints (e.g. offering `string`).
    let src = "<?php\nclass Foo {\n    public str\n}";
    let ctx = detect(src, 2, 14).unwrap();
    assert_eq!(ctx.partial, "str");
}

#[test]
fn partial_us_after_public_is_type_hint() {
    // `public Us` — "Us" is not a prefix of any declaration keyword,
    // so it should trigger type hints (e.g. offering `UserService`).
    let src = "<?php\nclass Foo {\n    public Us\n}";
    let ctx = detect(src, 2, 13).unwrap();
    assert_eq!(ctx.partial, "Us");
}

#[test]
fn partial_fn_after_public_is_not_type_hint() {
    // `public fn` — "fn" is an exact match for the keyword.
    let src = "<?php\nclass Foo {\n    public fn\n}";
    let ctx = detect(src, 2, 13);
    assert!(
        ctx.is_none(),
        "partial 'fn' matches declaration keyword and should not trigger type hints"
    );
}

#[test]
fn partial_use_after_public_is_not_type_hint() {
    // Inside a class, `public use` — "use" is a declaration keyword (trait import).
    // Note: not valid PHP but the keyword check should still exclude it.
    let src = "<?php\nclass Foo {\n    public use\n}";
    let ctx = detect(src, 2, 14);
    assert!(
        ctx.is_none(),
        "partial 'use' matches declaration keyword and should not trigger type hints"
    );
}

// ── Native types constant ───────────────────────────────────────

#[test]
fn native_types_includes_common_types() {
    assert!(PHP_NATIVE_TYPES.contains(&"string"));
    assert!(PHP_NATIVE_TYPES.contains(&"int"));
    assert!(PHP_NATIVE_TYPES.contains(&"float"));
    assert!(PHP_NATIVE_TYPES.contains(&"bool"));
    assert!(PHP_NATIVE_TYPES.contains(&"array"));
    assert!(PHP_NATIVE_TYPES.contains(&"mixed"));
    assert!(PHP_NATIVE_TYPES.contains(&"void"));
    assert!(PHP_NATIVE_TYPES.contains(&"never"));
    assert!(PHP_NATIVE_TYPES.contains(&"callable"));
    assert!(PHP_NATIVE_TYPES.contains(&"self"));
    assert!(PHP_NATIVE_TYPES.contains(&"static"));
    assert!(PHP_NATIVE_TYPES.contains(&"null"));
    assert!(PHP_NATIVE_TYPES.contains(&"true"));
    assert!(PHP_NATIVE_TYPES.contains(&"false"));
}

#[test]
fn native_types_excludes_phpstan_only() {
    assert!(!PHP_NATIVE_TYPES.contains(&"class-string"));
    assert!(!PHP_NATIVE_TYPES.contains(&"positive-int"));
    assert!(!PHP_NATIVE_TYPES.contains(&"non-empty-string"));
    assert!(!PHP_NATIVE_TYPES.contains(&"resource"));
}
