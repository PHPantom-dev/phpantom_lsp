use super::*;
use crate::docblock::type_strings::split_type_token;
use tower_lsp::lsp_types::Position;

use super::super::target::{Anchor, target_function};

/// The function whose `return` on `line` a diagnostic is about.
fn target_at(content: &str, line: usize) -> Option<TargetFunction> {
    let message = "Function foo() should return int but returns string.";
    target_function(content, line, Anchor::Return, message)
}

fn strip(content: &str, line: usize) -> Option<TextEdit> {
    build_strip_return_expr_edit(content, &target_at(content, line)?)
}

fn change_to(content: &str, line: usize, target_type: &PhpType) -> Option<Vec<TextEdit>> {
    build_change_return_type_edits_to(content, &target_at(content, line)?, target_type)
}

fn update(content: &str, line: usize, actual_type: &PhpType) -> Option<Vec<TextEdit>> {
    build_update_return_type_edits(content, &target_at(content, line)?, actual_type)
}

// ── build_strip_return_expr_edit ────────────────────────────────

#[test]
fn removes_return_keeps_expression_omits_redundant_return() {
    // Last statement in function — no need for bare `return;`.
    let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    let edit = strip(content, 2).unwrap();
    assert_eq!(edit.range.start, Position::new(2, 4));
    assert_eq!(edit.range.end, Position::new(2, 14));
    assert_eq!(edit.new_text, "42;");
}

#[test]
fn removes_return_string() {
    // Last statement in function — no bare `return;`.
    let content = "<?php\nfunction foo(): void {\n    return 'hello';\n}\n";
    let edit = strip(content, 2).unwrap();
    assert_eq!(edit.new_text, "'hello';");
}

#[test]
fn removes_return_method_call() {
    // Last statement in method — no bare `return;`.
    let content = "<?php\nclass A {\n    public function run(): void {\n        return $this->doWork();\n    }\n}\n";
    let edit = strip(content, 3).unwrap();
    assert_eq!(edit.new_text, "$this->doWork();");
}

#[test]
fn removes_return_in_if_block_with_more_code() {
    // NOT the last statement — there's `echo 'more';` after the if block.
    let content = "<?php\nclass A {\n    public function run(): void {\n        if (true) {\n            return $this->doWork();\n        }\n        echo 'more';\n    }\n}\n";
    let edit = strip(content, 4).unwrap();
    assert_eq!(edit.new_text, "$this->doWork();\n            return;");
}

#[test]
fn return_null_becomes_bare_return() {
    // `return null;` → `return;` (null is not meaningful in void)
    let content = "<?php\nfunction foo(): void {\n    return null;\n}\n";
    let edit = strip(content, 2).unwrap();
    assert_eq!(edit.new_text, "return;");
}

#[test]
fn strips_return_expression_variable() {
    // Last statement — no bare `return;`.
    let content = "<?php\nfunction foo(): void {\n    return $value;\n}\n";
    let edit = strip(content, 2).unwrap();
    assert_eq!(edit.new_text, "$value;");
    assert_eq!(edit.range.start, Position::new(2, 4));
}

#[test]
fn strips_multiline_return_expression() {
    // Last statement — no bare `return;`.
    let content =
        "<?php\nfunction foo(): void {\n    return array(\n        1,\n        2\n    );\n}\n";
    let edit = strip(content, 2).unwrap();
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
    let edit = strip(content, 4).unwrap();
    assert_eq!(edit.new_text, "$this->doWork();");
}

fn apply_strip(content: &str, line: usize) -> String {
    let edit = strip(content, line).unwrap();
    crate::text_position::apply_text_edits(content, &[edit])
}

#[test]
fn braces_a_braceless_if_body_on_its_own_line() {
    let content = "<?php\nclass A {\n    public function run(): void {\n        if ($this->ready())\n            return $this->save();\n        $this->log();\n    }\n}\n";
    assert_eq!(
        apply_strip(content, 4),
        "<?php\nclass A {\n    public function run(): void {\n        if ($this->ready()) {\n            $this->save();\n            return;\n        }\n        $this->log();\n    }\n}\n"
    );
}

#[test]
fn braces_a_braceless_if_body_on_the_same_line() {
    let content = "<?php\nfunction foo(): void {\n    if ($x) return $v;\n    bar();\n}\n";
    assert_eq!(
        apply_strip(content, 2),
        "<?php\nfunction foo(): void {\n    if ($x) { $v; return; }\n    bar();\n}\n"
    );
}

#[test]
fn braces_braceless_elseif_and_else_bodies() {
    let content = "<?php\nfunction foo(): void {\n    if ($a) bar();\n    elseif ($b) return $v;\n    else\n        return $w;\n    bar();\n}\n";
    assert_eq!(
        apply_strip(content, 3),
        "<?php\nfunction foo(): void {\n    if ($a) bar();\n    elseif ($b) { $v; return; }\n    else\n        return $w;\n    bar();\n}\n"
    );
    assert_eq!(
        apply_strip(content, 5),
        "<?php\nfunction foo(): void {\n    if ($a) bar();\n    elseif ($b) return $v;\n    else {\n        $w;\n        return;\n    }\n    bar();\n}\n"
    );
}

#[test]
fn braces_a_braceless_body_behind_a_comment_on_its_line() {
    let content =
        "<?php\nfunction foo(): void {\n    if ($x) // done\n        return $v;\n    bar();\n}\n";
    assert_eq!(
        apply_strip(content, 3),
        "<?php\nfunction foo(): void {\n    if ($x) // done\n        { $v; return; }\n    bar();\n}\n"
    );
}

#[test]
fn braceless_if_body_as_the_last_statement_needs_no_return() {
    let content = "<?php\nfunction foo(): void {\n    if ($x) return $v;\n}\n";
    assert_eq!(
        apply_strip(content, 2),
        "<?php\nfunction foo(): void {\n    if ($x) $v;\n}\n"
    );
}

#[test]
fn keeps_the_return_of_a_loop_that_ends_the_function() {
    let content = "<?php\nfunction foo(array $a): void {\n    foreach ($a as $x) return $x;\n}\n";
    assert_eq!(
        apply_strip(content, 2),
        "<?php\nfunction foo(array $a): void {\n    foreach ($a as $x) { $x; return; }\n}\n"
    );
    let content =
        "<?php\nfunction foo(array $a): void {\n    while (true) {\n        return 1;\n    }\n}\n";
    assert_eq!(
        apply_strip(content, 3),
        "<?php\nfunction foo(array $a): void {\n    while (true) {\n        1;\n        return;\n    }\n}\n"
    );
}

#[test]
fn a_closure_in_a_loop_ends_with_its_last_statement() {
    let content = "<?php\nfunction run(): void {\n    foreach ([1] as $x) {\n        $f = function (): void {\n            return 42;\n        };\n    }\n}\n";
    let message =
        "Anonymous function with return type void returns int but should not return anything.";
    let target = target_function(content, 4, Anchor::Return, message).unwrap();
    let edit = build_strip_return_expr_edit(content, &target).unwrap();
    assert_eq!(edit.new_text, "42;");
}

#[test]
fn last_statement_of_a_method_followed_by_another() {
    // What comes after the method's closing brace is not in the method.
    let content = "<?php\nclass A {\n    public function run(): void {\n        return $this->doWork();\n    }\n\n    public function other(): void {}\n}\n";
    let edit = strip(content, 3).unwrap();
    assert_eq!(edit.new_text, "$this->doWork();");
}

#[test]
fn last_statement_of_a_closure() {
    let content = "<?php\nfunction run(): void {\n    $f = function (): void {\n        return 42;\n    };\n    $f();\n}\n";
    let message =
        "Anonymous function with return type void returns int but should not return anything.";
    let target = target_function(content, 3, Anchor::Return, message).unwrap();
    let edit = build_strip_return_expr_edit(content, &target).unwrap();
    assert_eq!(edit.new_text, "42;");
}

#[test]
fn strips_the_return_of_the_closure_the_message_names() {
    let content = "<?php\nfunction run(): array {\n    return array_map(function ($x): void { return $x; }, [1]);\n}\n";
    let message =
        "Anonymous function with return type void returns int but should not return anything.";
    let target = target_function(content, 2, Anchor::Return, message).unwrap();
    let edit = build_strip_return_expr_edit(content, &target).unwrap();
    assert_eq!(
        crate::text_position::apply_text_edits(content, &[edit]),
        "<?php\nfunction run(): array {\n    return array_map(function ($x): void { $x; }, [1]);\n}\n"
    );
}

#[test]
fn nothing_to_strip_from_an_arrow_function() {
    let content =
        "<?php\nfunction run(): void {\n    return array_map(fn ($x): void => $x, [1]);\n}\n";
    let message =
        "Anonymous function with return type void returns int but should not return anything.";
    let target = target_function(content, 2, Anchor::Return, message).unwrap();
    assert!(build_strip_return_expr_edit(content, &target).is_none());
}

#[test]
fn returns_none_when_already_bare_return() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(strip(content, 2).is_none());
}

#[test]
fn returns_none_for_invalid_line() {
    let content = "<?php\n";
    assert!(strip(content, 5).is_none());
}

#[test]
fn returns_none_when_no_return_on_line() {
    let content = "<?php\nfunction foo(): void {\n    $x = 1;\n}\n";
    assert!(strip(content, 2).is_none());
}

// ── build_change_return_type_edits_to ───────────────────────────

#[test]
fn changes_return_type_to_void() {
    let content = "<?php\nfunction foo(): int {\n    return;\n}\n";
    let edits = change_to(content, 2, &PhpType::void()).unwrap();
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
    let edits = change_to(content, 2, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": void");
}

#[test]
fn changes_return_type_to_actual() {
    let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    let edits = change_to(content, 2, &PhpType::parse("int")).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": int");
}

#[test]
fn changes_void_to_string() {
    let content = "<?php\nfunction foo(): void {\n    return 'hello';\n}\n";
    let edits = change_to(content, 2, &PhpType::parse("string")).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": string");
}

#[test]
fn changes_nullable_return_type() {
    let content = "<?php\nfunction foo(): ?string {\n    return;\n}\n";
    let edits = change_to(content, 2, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": void");
}

#[test]
fn changes_return_type_and_removes_return_tag() {
    let content =
        "<?php\n/**\n * @return int The value\n */\nfunction foo(): int {\n    return;\n}\n";
    let edits = change_to(content, 5, &PhpType::void()).unwrap();
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
    assert!(change_to(content, 2, &PhpType::void()).is_none());
}

#[test]
fn does_not_change_when_already_matches_actual() {
    let content = "<?php\nfunction foo(): int {\n    return 42;\n}\n";
    assert!(change_to(content, 2, &PhpType::parse("int")).is_none());
}

#[test]
fn returns_none_when_no_function_found() {
    let content = "<?php\nreturn;\n";
    assert!(change_to(content, 1, &PhpType::void()).is_none());
}

#[test]
fn changes_method_return_type() {
    let content =
        "<?php\nclass Foo {\n    public function bar(): string {\n        return;\n    }\n}\n";
    let edits = change_to(content, 3, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": void");
}

// ── Docblock @return removal ───────────────────────────────────

#[test]
fn change_to_actual_does_not_remove_return_tag() {
    let content =
        "<?php\n/**\n * @return int The value\n */\nfunction foo(): void {\n    return 42;\n}\n";
    let edits = change_to(content, 5, &PhpType::parse("int")).unwrap();
    // Should only change the type hint, NOT remove the @return tag
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": int");
}

#[test]
fn removes_return_tag_from_multiline_docblock() {
    let content = "<?php\n/**\n * Does something.\n * @return int\n */\nfunction foo(): int {\n    return;\n}\n";
    let edits = change_to(content, 6, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 2);
    let tag_edit = edits.iter().find(|e| e.new_text.is_empty()).unwrap();
    assert_eq!(tag_edit.range.start.line, 3);
    assert_eq!(tag_edit.range.end.line, 4);
}

#[test]
fn no_return_tag_edit_when_no_docblock() {
    let content = "<?php\nfunction foo(): int {\n    return;\n}\n";
    let edits = change_to(content, 2, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1); // Only the type edit, no tag edit.
}

#[test]
fn no_return_tag_edit_when_docblock_has_no_return() {
    let content = "<?php\n/**\n * Does something.\n */\nfunction foo(): int {\n    return;\n}\n";
    let edits = change_to(content, 5, &PhpType::void()).unwrap();
    assert_eq!(edits.len(), 1); // Only the type edit, no tag edit.
}

// ── Integration: apply strip edit ──────────────────────────────

#[test]
fn apply_strip_edit_produces_correct_content() {
    // `return 42;` is the last statement → replaced with just `42;`
    // (no redundant `return;` since it's the last statement).
    let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    let edit = strip(content, 2).unwrap();

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
    let edit = strip(content, 2).unwrap();

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
    let edits = update(content, 2, &PhpType::parse("int")).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, ": int");
}

#[test]
fn update_return_type_generic_keeps_native_adds_docblock() {
    // Generic type — native stays `array`, docblock gets `array<int, int>`.
    let content = "<?php\nfunction foo(): array {\n    return [1, 2, 3];\n}\n";
    let edits = update(content, 2, &PhpType::parse("array<int, int>")).unwrap();
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
    let edits = update(content, 2, &PhpType::parse("array<int, int>")).unwrap();
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
    let edits = update(content, 5, &PhpType::parse("array<int, int>")).unwrap();
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
    let edits = update(content, 5, &PhpType::parse("array<int, int>")).unwrap();
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
    assert!(update(content, 2, &PhpType::parse("int")).is_none());
}

#[test]
fn update_closure_return_type_writes_no_docblock() {
    // PHPStan reads no docblock on a closure, so only its native type
    // changes, and nothing is written above the statement holding it.
    let content = "<?php\nfunction run(): array {\n    $f = function (): string {\n        return [1];\n    };\n    return [];\n}\n";
    let message = "Anonymous function should return string but returns array<int, int>.";
    let target = target_function(content, 3, Anchor::Return, message).unwrap();
    let edits =
        build_update_return_type_edits(content, &target, &PhpType::parse("array<int, int>"))
            .unwrap();
    assert_eq!(
        crate::text_position::apply_text_edits(content, &edits),
        "<?php\nfunction run(): array {\n    $f = function (): array {\n        return [1];\n    };\n    return [];\n}\n"
    );
}

#[test]
fn update_closure_keeps_its_own_return_tag_in_step() {
    let content = "<?php\nfunction run(): array {\n    $f =\n        /** @return string */\n        function (): string {\n            return 1;\n        };\n    return [];\n}\n";
    let message = "Anonymous function should return string but returns int.";
    let target = target_function(content, 5, Anchor::Return, message).unwrap();
    let edits = build_update_return_type_edits(content, &target, &PhpType::parse("int")).unwrap();
    assert_eq!(
        crate::text_position::apply_text_edits(content, &edits),
        "<?php\nfunction run(): array {\n    $f =\n        /** @return int */\n        function (): int {\n            return 1;\n        };\n    return [];\n}\n"
    );
}
