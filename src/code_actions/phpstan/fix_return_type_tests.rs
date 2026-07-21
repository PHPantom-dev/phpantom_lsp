use super::*;

// ── build_strip_return_expr_edit ────────────────────────────────

#[test]
fn removes_return_keeps_expression_omits_redundant_return() {
    // Last statement in function — no need for bare `return;`.
    let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    let edit = build_strip_return_expr_edit(content, 2).unwrap();
    assert_eq!(edit.range.start, Position::new(2, 4));
    assert_eq!(edit.range.end, Position::new(2, 14));
    assert_eq!(edit.new_text, "42;");
}

#[test]
fn removes_return_string() {
    // Last statement in function — no bare `return;`.
    let content = "<?php\nfunction foo(): void {\n    return 'hello';\n}\n";
    let edit = build_strip_return_expr_edit(content, 2).unwrap();
    assert_eq!(edit.new_text, "'hello';");
}

#[test]
fn removes_return_method_call() {
    // Last statement in method — no bare `return;`.
    let content = "<?php\nclass A {\n    public function run(): void {\n        return $this->doWork();\n    }\n}\n";
    let edit = build_strip_return_expr_edit(content, 3).unwrap();
    assert_eq!(edit.new_text, "$this->doWork();");
}

#[test]
fn removes_return_in_if_block_with_more_code() {
    // NOT the last statement — there's `echo 'more';` after the if block.
    let content = "<?php\nclass A {\n    public function run(): void {\n        if (true) {\n            return $this->doWork();\n        }\n        echo 'more';\n    }\n}\n";
    let edit = build_strip_return_expr_edit(content, 4).unwrap();
    assert_eq!(edit.new_text, "$this->doWork();\n            return;");
}

#[test]
fn return_null_becomes_bare_return() {
    // `return null;` → `return;` (null is not meaningful in void)
    let content = "<?php\nfunction foo(): void {\n    return null;\n}\n";
    let edit = build_strip_return_expr_edit(content, 2).unwrap();
    assert_eq!(edit.new_text, "return;");
}

#[test]
fn strips_return_expression_variable() {
    // Last statement — no bare `return;`.
    let content = "<?php\nfunction foo(): void {\n    return $value;\n}\n";
    let edit = build_strip_return_expr_edit(content, 2).unwrap();
    assert_eq!(edit.new_text, "$value;");
    assert_eq!(edit.range.start, Position::new(2, 4));
}

#[test]
fn strips_multiline_return_expression() {
    // Last statement — no bare `return;`.
    let content =
        "<?php\nfunction foo(): void {\n    return array(\n        1,\n        2\n    );\n}\n";
    let edit = build_strip_return_expr_edit(content, 2).unwrap();
    assert_eq!(edit.new_text, "array(\n        1,\n        2\n    );");
    assert_eq!(edit.range.start, Position::new(2, 4));
    // The `;` is on line 5 (0-indexed)
    assert_eq!(edit.range.end.line, 5);
}

#[test]
fn strips_return_in_if_block_last_statement() {
    // return is inside an if block, but it IS the last statement
    // in the function (only `}` closers follow).
    let content = "<?php\nclass A {\n    public function run(): void {\n        if (true) {\n            return $this->doWork();\n        }\n    }\n}\n";
    let edit = build_strip_return_expr_edit(content, 4).unwrap();
    assert_eq!(edit.new_text, "$this->doWork();");
}

#[test]
fn returns_none_when_already_bare_return() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(build_strip_return_expr_edit(content, 2).is_none());
}

#[test]
fn returns_none_for_invalid_line() {
    let content = "<?php\n";
    assert!(build_strip_return_expr_edit(content, 5).is_none());
}

#[test]
fn returns_none_when_no_return_on_line() {
    let content = "<?php\nfunction foo(): void {\n    $x = 1;\n}\n";
    assert!(build_strip_return_expr_edit(content, 2).is_none());
}

// ── build_change_return_type_edits_to ───────────────────────────

#[test]
fn changes_return_type_to_void() {
    let content = "<?php\nfunction foo(): int {\n    return;\n}\n";
    let edits = build_change_return_type_edits_to(content, 2, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    assert_eq!(edit.new_text, ": void");
    // Verify it replaces `: int`
    let lines: Vec<&str> = content.lines().collect();
    let replaced = &lines[edit.range.start.line as usize]
        [edit.range.start.character as usize..edit.range.end.character as usize];
    assert_eq!(replaced, ": int");
}

#[test]
fn changes_return_type_string() {
    let content = "<?php\nfunction foo(): string {\n    return;\n}\n";
    let edits = build_change_return_type_edits_to(content, 2, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": void");
}

#[test]
fn changes_return_type_to_actual() {
    let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    let edits = build_change_return_type_edits_to(content, 2, &PhpType::parse("int")).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": int");
}

#[test]
fn changes_void_to_string() {
    let content = "<?php\nfunction foo(): void {\n    return 'hello';\n}\n";
    let edits = build_change_return_type_edits_to(content, 2, &PhpType::parse("string")).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": string");
}

#[test]
fn changes_nullable_return_type() {
    let content = "<?php\nfunction foo(): ?string {\n    return;\n}\n";
    let edits = build_change_return_type_edits_to(content, 2, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": void");
}

#[test]
fn changes_return_type_and_removes_return_tag() {
    let content =
        "<?php\n/**\n * @return int The value\n */\nfunction foo(): int {\n    return;\n}\n";
    let edits = build_change_return_type_edits_to(content, 5, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 2);

    // One edit replaces the type, one removes the @return line.
    let type_edit = edits.iter().find(|e| e.new_text == ": void").unwrap();
    let tag_edit = edits.iter().find(|e| e.new_text.is_empty()).unwrap();

    // The type edit should be on the function line (line 4).
    assert_eq!(type_edit.range.start.line, 4);

    // The @return tag is on line 2.
    assert_eq!(tag_edit.range.start.line, 2);
    assert_eq!(tag_edit.range.end.line, 3);
}

#[test]
fn does_not_change_when_already_void() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(build_change_return_type_edits_to(content, 2, &PhpType::void()).is_none());
}

#[test]
fn does_not_change_when_already_matches_actual() {
    let content = "<?php\nfunction foo(): int {\n    return 42;\n}\n";
    assert!(build_change_return_type_edits_to(content, 2, &PhpType::parse("int")).is_none());
}

#[test]
fn returns_none_when_no_function_found() {
    let content = "<?php\nreturn;\n";
    assert!(build_change_return_type_edits_to(content, 1, &PhpType::void()).is_none());
}

#[test]
fn changes_method_return_type() {
    let content =
        "<?php\nclass Foo {\n    public function bar(): string {\n        return;\n    }\n}\n";
    let edits = build_change_return_type_edits_to(content, 3, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": void");
}

// ── is_fix_return_type_stale ───────────────────────────────────

#[test]
fn stale_when_return_has_no_expression() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(is_fix_return_type_stale(content, 2, "return.void"));
}

#[test]
fn not_stale_when_return_has_expression() {
    let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    assert!(!is_fix_return_type_stale(content, 2, "return.void"));
}

#[test]
fn stale_return_empty_when_type_is_void() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(is_fix_return_type_stale(content, 2, "return.empty"));
}

#[test]
fn not_stale_return_empty_when_type_is_not_void() {
    let content = "<?php\nfunction foo(): int {\n    return;\n}\n";
    assert!(!is_fix_return_type_stale(content, 2, "return.empty"));
}

#[test]
fn stale_when_line_gone() {
    let content = "<?php\n";
    assert!(is_fix_return_type_stale(content, 5, "return.void"));
    assert!(is_fix_return_type_stale(content, 5, "return.empty"));
}

#[test]
fn not_stale_for_unknown_identifier() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(!is_fix_return_type_stale(content, 2, "other.id"));
}

// ── Message matching ───────────────────────────────────────────

#[test]
fn matches_return_void_message() {
    let msg = "Method Foo::bar() with return type void returns int but should not return anything.";
    assert!(msg.ends_with(RETURN_VOID_MSG_SUFFIX));
}

// ── extract_actual_type ─────────────────────────────────────────

#[test]
fn extracts_actual_type_int() {
    let msg = "Method Foo::bar() with return type void returns int but should not return anything.";
    assert_eq!(extract_actual_type(msg), Some(PhpType::parse("int")));
}

#[test]
fn extracts_actual_type_string() {
    let msg = "Function foo() with return type void returns string but should not return anything.";
    assert_eq!(extract_actual_type(msg), Some(PhpType::parse("string")));
}

#[test]
fn extracts_actual_type_union() {
    let msg =
        "Method X::y() with return type void returns int|string but should not return anything.";
    assert_eq!(extract_actual_type(msg), Some(PhpType::parse("int|string")));
}

#[test]
fn extracts_actual_type_null() {
    let msg = "Method X::y() with return type void returns null but should not return anything.";
    assert_eq!(extract_actual_type(msg), Some(PhpType::parse("null")));
}

#[test]
fn extract_actual_type_returns_none_for_unrelated_message() {
    let msg = "Some other message.";
    assert_eq!(extract_actual_type(msg), None);
}

// ── extract_return_type_actual (return.type) ────────────────────

#[test]
fn extracts_return_type_actual_int() {
    let msg = "Method Foo::bar() should return string but returns int.";
    assert_eq!(extract_return_type_actual(msg), Some(PhpType::parse("int")));
}

#[test]
fn extracts_return_type_actual_union() {
    let msg = "Function foo() should return int but returns int|string.";
    assert_eq!(
        extract_return_type_actual(msg),
        Some(PhpType::parse("int|string"))
    );
}

#[test]
fn extracts_return_type_actual_class() {
    let msg = "Method X::y() should return self but returns App\\Models\\User.";
    assert_eq!(
        extract_return_type_actual(msg),
        Some(PhpType::parse("App\\Models\\User"))
    );
}

#[test]
fn extract_return_type_actual_returns_none_for_unrelated() {
    let msg = "Some other message.";
    assert_eq!(extract_return_type_actual(msg), None);
}

// ── has_return_type_between ─────────────────────────────────────

#[test]
fn detects_existing_return_type() {
    let lines = vec!["function foo(): int {"];
    // paren at col 13 (the ')'), brace_line = 0
    assert!(has_return_type_between(&lines, 0, 13, 0));
}

#[test]
fn detects_no_return_type() {
    let lines = vec!["function foo() {"];
    // paren at col 13, brace_line = 0
    assert!(!has_return_type_between(&lines, 0, 13, 0));
}

// ── infer_type_from_literal ─────────────────────────────────────

#[test]
fn literal_int() {
    assert_eq!(
        infer_type_from_literal("42").map(|t| t.to_string()),
        Some("int".to_string())
    );
    assert_eq!(
        infer_type_from_literal("-1").map(|t| t.to_string()),
        Some("int".to_string())
    );
}

#[test]
fn literal_float() {
    assert_eq!(
        infer_type_from_literal("1.5").map(|t| t.to_string()),
        Some("float".to_string())
    );
}

#[test]
fn literal_bool() {
    assert_eq!(
        infer_type_from_literal("true").map(|t| t.to_string()),
        Some("bool".to_string())
    );
    assert_eq!(
        infer_type_from_literal("false").map(|t| t.to_string()),
        Some("bool".to_string())
    );
}

#[test]
fn literal_string() {
    assert_eq!(
        infer_type_from_literal("'hello'").map(|t| t.to_string()),
        Some("string".to_string())
    );
    assert_eq!(
        infer_type_from_literal("\"world\"").map(|t| t.to_string()),
        Some("string".to_string())
    );
}

#[test]
fn literal_array_empty() {
    assert_eq!(
        infer_type_from_literal("[]").map(|t| t.to_string()),
        Some("array".to_string())
    );
}

#[test]
fn literal_array_of_strings() {
    assert_eq!(
        infer_type_from_literal("['string']").map(|t| t.to_string()),
        Some("list<string>".to_string())
    );
    assert_eq!(
        infer_type_from_literal("['a', 'b', 'c']").map(|t| t.to_string()),
        Some("list<string>".to_string())
    );
}

#[test]
fn literal_array_of_ints() {
    assert_eq!(
        infer_type_from_literal("[1, 2, 3]").map(|t| t.to_string()),
        Some("list<int>".to_string())
    );
}

#[test]
fn literal_array_mixed_scalars() {
    assert_eq!(
        infer_type_from_literal("['a', 1]").map(|t| t.to_string()),
        Some("list<string|int>".to_string())
    );
}

#[test]
fn literal_array_with_string_keys() {
    assert_eq!(
        infer_type_from_literal("['key' => 'value']").map(|t| t.to_string()),
        Some("array<string, string>".to_string())
    );
    assert_eq!(
        infer_type_from_literal("['name' => 'Alice', 'age' => 42]").map(|t| t.to_string()),
        Some("array<string, string|int>".to_string())
    );
}

#[test]
fn literal_array_nested() {
    assert_eq!(
        infer_type_from_literal("[['a'], ['b']]").map(|t| t.to_string()),
        Some("list<list<string>>".to_string())
    );
}

#[test]
fn literal_array_with_variable_falls_back() {
    assert_eq!(
        infer_type_from_literal("[$var, 'a']").map(|t| t.to_string()),
        Some("array".to_string())
    );
}

#[test]
fn literal_array_legacy_syntax() {
    assert_eq!(
        infer_type_from_literal("array('a', 'b')").map(|t| t.to_string()),
        Some("list<string>".to_string())
    );
}

#[test]
fn literal_array_new_objects() {
    assert_eq!(
        infer_type_from_literal("[new Foo(), new Foo()]").map(|t| t.to_string()),
        Some("list<Foo>".to_string())
    );
}

#[test]
fn literal_array_trailing_comma() {
    assert_eq!(
        infer_type_from_literal("['a', 'b',]").map(|t| t.to_string()),
        Some("list<string>".to_string())
    );
}

#[test]
fn literal_new_class() {
    assert_eq!(
        infer_type_from_literal("new Foo()").map(|t| t.to_string()),
        Some("Foo".to_string())
    );
}

#[test]
fn literal_null() {
    assert_eq!(
        infer_type_from_literal("null").map(|t| t.to_string()),
        Some("null".to_string())
    );
}

#[test]
fn non_literal_returns_none() {
    assert_eq!(infer_type_from_literal("$var"), None);
    assert_eq!(infer_type_from_literal("$this->bar()"), None);
    assert_eq!(infer_type_from_literal("foo()"), None);
    assert_eq!(infer_type_from_literal("Str::toUpper($x)"), None);
}

// ── stale detection for new identifiers ─────────────────────────

#[test]
fn return_type_never_stale_via_heuristic() {
    // return.type is only cleared by codeAction/resolve, not by
    // content heuristics, because the right fix might be to change
    // the code rather than the type.
    let content = "<?php\nfunction foo(): int {\n    $x = 1;\n}\n";
    assert!(!is_fix_return_type_stale(content, 2, "return.type"));

    let content2 = "<?php\nfunction foo(): int {\n    return 'hello';\n}\n";
    assert!(!is_fix_return_type_stale(content2, 2, "return.type"));
}

#[test]
fn stale_missing_type_when_type_added() {
    let content = "<?php\nfunction foo(): int {\n    return 1;\n}\n";
    // missingType.return is reported on the function declaration line
    assert!(is_fix_return_type_stale(content, 1, "missingType.return"));
}

#[test]
fn stale_missing_type_multiline_signature() {
    let content = "<?php\nfunction foo(\n    int $x\n): int {\n    return $x;\n}\n";
    // The diagnostic is on the `function` line (line 1), but the
    // `)` and `: int` are on line 3.  PHPStan reports on the
    // function keyword line.  Our simple check looks at the diag
    // line for `)...:`  which won't find it on line 1.  That's
    // acceptable — the diagnostic will be cleared by the next
    // PHPStan run instead of eagerly.
    assert!(!is_fix_return_type_stale(content, 1, "missingType.return"));
}

#[test]
fn not_stale_missing_type_when_no_type() {
    let content = "<?php\nfunction foo() {\n    return 1;\n}\n";
    assert!(!is_fix_return_type_stale(content, 1, "missingType.return"));
}

#[test]
fn matches_return_empty_message() {
    let msg = "Method App\\Foo::bar() should return int but empty return statement found.";
    assert!(msg.contains(RETURN_EMPTY_MSG_FRAGMENT));
}

#[test]
fn rejects_unrelated_message() {
    let msg = "Call to function assert() with true will always evaluate to true.";
    assert!(!msg.ends_with(RETURN_VOID_MSG_SUFFIX));
    assert!(!msg.contains(RETURN_EMPTY_MSG_FRAGMENT));
}

// ── Helper tests ───────────────────────────────────────────────

// ── Docblock @return removal ───────────────────────────────────

#[test]
fn change_to_actual_does_not_remove_return_tag() {
    let content =
        "<?php\n/**\n * @return int The value\n */\nfunction foo(): void {\n    return 42;\n}\n";
    let edits = build_change_return_type_edits_to(content, 5, &PhpType::parse("int")).unwrap();
    // Should only change the type hint, NOT remove the @return tag
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": int");
}

// ── add return type (missingType.return) ────────────────────────

#[test]
fn add_return_type_inserts_after_close_paren_helper() {
    let content = "<?php\nfunction foo() {\n    return 1;\n}\n";
    let lines: Vec<&str> = content.lines().collect();
    let brace_line = find_function_open_brace_line(&lines, 2).unwrap();
    let (paren_line, paren_col) = find_close_paren_before_brace(&lines, brace_line).unwrap();
    assert!(!has_return_type_between(
        &lines, paren_line, paren_col, brace_line
    ));
    assert_eq!(paren_line, 1);
    assert_eq!(paren_col, 13);
}

#[test]
fn removes_return_tag_from_multiline_docblock() {
    let content = "<?php\n/**\n * Does something.\n * @return int\n */\nfunction foo(): int {\n    return;\n}\n";
    let edits = build_change_return_type_edits_to(content, 6, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 2);
    let tag_edit = edits.iter().find(|e| e.new_text.is_empty()).unwrap();
    assert_eq!(tag_edit.range.start.line, 3);
    assert_eq!(tag_edit.range.end.line, 4);
}

#[test]
fn no_return_tag_edit_when_no_docblock() {
    let content = "<?php\nfunction foo(): int {\n    return;\n}\n";
    let edits = build_change_return_type_edits_to(content, 2, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1); // Only the type edit, no tag edit.
}

#[test]
fn no_return_tag_edit_when_docblock_has_no_return() {
    let content = "<?php\n/**\n * Does something.\n */\nfunction foo(): int {\n    return;\n}\n";
    let edits = build_change_return_type_edits_to(content, 5, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1); // Only the type edit, no tag edit.
}

// ── Integration: apply strip edit ──────────────────────────────

#[test]
fn apply_strip_edit_produces_correct_content() {
    // `return 42;` is the last statement → replaced with just `42;`
    // (no redundant `return;` since it's the last statement).
    let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    let edit = build_strip_return_expr_edit(content, 2).unwrap();

    // Apply the edit manually.
    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        if i == edit.range.start.line as usize {
            let prefix = &line[..edit.range.start.character as usize];
            let suffix = if edit.range.end.line as usize == i {
                &line[edit.range.end.character as usize..]
            } else {
                ""
            };
            result.push_str(prefix);
            result.push_str(&edit.new_text);
            result.push_str(suffix);
        } else {
            result.push_str(line);
        }
    }
    result.push('\n');

    assert_eq!(result, "<?php\nfunction foo(): void {\n    42;\n}\n");
}

#[test]
fn apply_strip_edit_null_produces_bare_return() {
    let content = "<?php\nfunction foo(): void {\n    return null;\n}\n";
    let edit = build_strip_return_expr_edit(content, 2).unwrap();

    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        if i == edit.range.start.line as usize {
            let prefix = &line[..edit.range.start.character as usize];
            let suffix = if edit.range.end.line as usize == i {
                &line[edit.range.end.character as usize..]
            } else {
                ""
            };
            result.push_str(prefix);
            result.push_str(&edit.new_text);
            result.push_str(suffix);
        } else {
            result.push_str(line);
        }
    }
    result.push('\n');

    assert_eq!(result, "<?php\nfunction foo(): void {\n    return;\n}\n");
}

// ── Stale detection after strip fix ─────────────────────────────

#[test]
fn stale_after_strip_fix() {
    // Before fix: not stale.
    let before = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    assert!(!is_fix_return_type_stale(before, 2, "return.void"));

    // After fix (expression kept, no redundant return;): stale
    // because the line no longer has `return ` (it now has `42;`).
    let after = "<?php\nfunction foo(): void {\n    42;\n}\n";
    assert!(is_fix_return_type_stale(after, 2, "return.void"));
}

// ── PhpType::to_native_hint (replaces strip_generic_params) ────

#[test]
fn strip_generic_simple_type() {
    let parsed = PhpType::parse("int");
    assert_eq!(
        parsed.to_native_hint().unwrap_or_else(|| "int".to_string()),
        "int"
    );
}

#[test]
fn strip_generic_array_with_params() {
    let parsed = PhpType::parse("array<int, string>");
    assert_eq!(
        parsed
            .to_native_hint()
            .unwrap_or_else(|| "array<int, string>".to_string()),
        "array"
    );
}

#[test]
fn strip_generic_nested() {
    let parsed = PhpType::parse("array<int, array<string, bool>>");
    assert_eq!(
        parsed
            .to_native_hint()
            .unwrap_or_else(|| "array<int, array<string, bool>>".to_string()),
        "array"
    );
}

#[test]
fn strip_generic_union_no_generics() {
    let parsed = PhpType::parse("int|string");
    assert_eq!(
        parsed
            .to_native_hint()
            .unwrap_or_else(|| "int|string".to_string()),
        "int|string"
    );
}

// ── split_type_token (replaces find_phpdoc_type_end) ───────────

#[test]
fn phpdoc_type_end_simple() {
    let (tok, _) = split_type_token("int The value");
    assert_eq!(tok, "int");
}

#[test]
fn phpdoc_type_end_generic() {
    let (tok, _) = split_type_token("array<int, string> The value");
    assert_eq!(tok, "array<int, string>");
}

#[test]
fn phpdoc_type_end_nested_generic() {
    let (tok, _) = split_type_token("array<int, array<string, bool>> desc");
    assert_eq!(tok, "array<int, array<string, bool>>");
}

#[test]
fn phpdoc_type_end_no_description() {
    let (tok, _) = split_type_token("int");
    assert_eq!(tok, "int");
}

#[test]
fn phpdoc_type_end_generic_no_description() {
    let (tok, _) = split_type_token("array<int, string>");
    assert_eq!(tok, "array<int, string>");
}

// ── build_update_return_type_edits ─────────────────────────────

#[test]
fn update_return_type_simple_changes_native_only() {
    // Simple type (no generics) — only native type hint changes.
    let content = "<?php\nfunction foo(): string {\n    return 42;\n}\n";
    let edits = build_update_return_type_edits(content, 2, &PhpType::parse("int")).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": int");
}

#[test]
fn update_return_type_generic_keeps_native_adds_docblock() {
    // Generic type — native stays `array`, docblock gets `array<int, int>`.
    let content = "<?php\nfunction foo(): array {\n    return [1, 2, 3];\n}\n";
    let edits =
        build_update_return_type_edits(content, 2, &PhpType::parse("array<int, int>")).unwrap();
    // Should only have docblock edit (native `array` already matches).
    assert_eq!(edits.len(), 1);
    assert!(
        edits[0].new_text.contains("@return array<int, int>"),
        "should create @return with generic type: {:?}",
        edits[0].new_text
    );
}

#[test]
fn update_return_type_generic_changes_native_and_docblock() {
    // Native type differs AND has generics.
    let content = "<?php\nfunction foo(): string {\n    return [1, 2, 3];\n}\n";
    let edits =
        build_update_return_type_edits(content, 2, &PhpType::parse("array<int, int>")).unwrap();
    assert_eq!(edits.len(), 2);
    // One edit for the native type, one for the docblock.
    let type_edit = edits.iter().find(|e| e.new_text == ": array").unwrap();
    assert!(type_edit.range.start.line == 1);
    let doc_edit = edits
        .iter()
        .find(|e| e.new_text.contains("@return array<int, int>"))
        .unwrap();
    assert!(doc_edit.range.start.line == 1); // inserted before function
}

#[test]
fn update_return_type_replaces_existing_generic_return_tag() {
    // Existing @return with generics — should be fully replaced.
    let content = "<?php\n/**\n * @return array<int, string>\n */\nfunction foo(): array {\n    return [1, 2, 3];\n}\n";
    let edits =
        build_update_return_type_edits(content, 5, &PhpType::parse("array<int, int>")).unwrap();
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    assert!(
        edit.new_text.contains("@return array<int, int>"),
        "should replace generic type: {}",
        edit.new_text
    );
    // Old type should not remain.
    assert!(
        !edit.new_text.contains("string>"),
        "old generic params should be gone: {}",
        edit.new_text
    );
}

#[test]
fn update_return_type_preserves_description_with_generics() {
    let content = "<?php\n/**\n * @return array<int, string> The data\n */\nfunction foo(): array {\n    return [1, 2, 3];\n}\n";
    let edits =
        build_update_return_type_edits(content, 5, &PhpType::parse("array<int, int>")).unwrap();
    assert_eq!(edits.len(), 1);
    assert!(
        edits[0].new_text.contains("@return array<int, int>"),
        "should have new type: {}",
        edits[0].new_text
    );
    assert!(
        edits[0].new_text.contains("The data"),
        "should preserve description: {}",
        edits[0].new_text
    );
}

#[test]
fn update_return_type_returns_none_when_already_correct() {
    let content = "<?php\nfunction foo(): int {\n    return 42;\n}\n";
    assert!(build_update_return_type_edits(content, 2, &PhpType::parse("int")).is_none());
}
