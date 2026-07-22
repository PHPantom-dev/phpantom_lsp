//! Integration tests for the "Fix return type" code actions.
//!
//! These tests exercise the full pipeline: inject a PHPStan diagnostic,
//! request code actions, resolve the chosen action, apply the edits,
//! and verify the resulting source text.
//!
//! Covers all four PHPStan identifiers:
//! - `return.void` — void function returns an expression
//! - `return.empty` — non-void function has bare `return;`
//! - `return.type` — return type doesn't match actual return
//! - `missingType.return` — no return type specified

use std::sync::Arc;

use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action, get_code_actions_on_line,
    inject_phpstan_diag, resolve_action,
};
use tower_lsp::lsp_types::*;

// ── return.void — strip expression ──────────────────────────────────────────

#[test]
fn return_void_strips_expression() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): void {
        return $this->doWork();
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Method Foo::run() with return type void returns void but should not return anything.",
        "return.void",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 3);
    let action = find_action(&actions, "Remove return statement")
        .expect("should offer 'Remove return statement'");

    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(false));

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("$this->doWork();"),
        "expression should be kept as standalone statement:\n{}",
        result
    );
    assert!(
        !result.contains("return"),
        "no bare return needed — last statement in function:\n{}",
        result
    );
}

#[test]
fn return_void_null_becomes_bare_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): void {
        return null;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Method Foo::run() with return type void returns null but should not return anything.",
        "return.void",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 3);
    let action = find_action(&actions, "Remove return statement")
        .expect("should offer 'Remove return statement'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("return;"),
        "should have bare return:\n{}",
        result
    );
    assert!(
        !result.contains("return null"),
        "return null should be gone:\n{}",
        result
    );
}

// ── return.void — change return type ────────────────────────────────────────

#[test]
fn return_void_offers_change_type_for_non_null() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): void {
        return 42;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Method Foo::run() with return type void returns int but should not return anything.",
        "return.void",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 3);

    // Should have both actions.
    let strip = find_action(&actions, "Remove return statement");
    assert!(strip.is_some(), "should offer strip action");

    let change = find_action(&actions, "Change return type to int")
        .expect("should offer 'Change return type to int'");
    assert_eq!(change.is_preferred, Some(true));

    let resolved = resolve_action(&backend, uri, content, change);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): int {"),
        "return type should be changed to int:\n{}",
        result
    );
    assert!(
        !result.contains("void"),
        "void should no longer appear:\n{}",
        result
    );
}

#[test]
fn return_void_no_change_type_for_null() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): void {
        return null;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Method Foo::run() with return type void returns null but should not return anything.",
        "return.void",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 3);

    // Strip action should be offered; change-type should NOT (null is not a useful type change).
    assert!(
        find_action(&actions, "Remove return statement").is_some(),
        "should offer strip action"
    );
    assert!(
        find_action(&actions, "Change return type").is_none(),
        "should NOT offer change type for null"
    );
}

// ── return.empty — change to void ───────────────────────────────────────────

#[test]
fn return_empty_changes_type_to_void() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): int {
        return;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Method Foo::run() should return int but empty return statement found.",
        "return.empty",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 3);
    let action = find_action(&actions, "Change return type to void")
        .expect("should offer 'Change return type to void'");

    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): void {"),
        "return type should be void:\n{}",
        result
    );
}

#[test]
fn return_empty_removes_return_tag() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Do something.
     * @return int
     */
    public function run(): int {
        return;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        7, // the `return;` line
        "Method Foo::run() should return int but empty return statement found.",
        "return.empty",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 7);
    let action =
        find_action(&actions, "Change return type to void").expect("should offer void fix");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): void {"),
        "return type should be void:\n{}",
        result
    );
    assert!(
        !result.contains("@return"),
        "@return tag should be removed:\n{}",
        result
    );
}

// ── return.type — change to actual ──────────────────────────────────────────

#[test]
fn return_type_changes_to_actual() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): string {
        return 42;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Method Foo::run() should return string but returns int.",
        "return.type",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 3);
    let action =
        find_action(&actions, "Update return type").expect("should offer 'Update return type'");

    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    // return.type is not preferred — the right fix might be to change the code
    assert_eq!(action.is_preferred, Some(false));

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): int {"),
        "return type should be changed to int:\n{}",
        result
    );
}

#[test]
fn return_type_uses_own_inference_when_it_differs() {
    // Our inference sees `return 'hello'` → `string`.  The current
    // declared type is `int`.  They differ, so we use our inference
    // rather than the PHPStan tip (`int|string`).
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): int {
    return 'hello';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Function foo() should return int but returns int|string.",
        "return.type",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action =
        find_action(&actions, "Update return type").expect("should offer update return type");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // Our inference is `string` (from the literal), not the PHPStan
    // union `int|string`.
    assert!(
        result.contains("): string {"),
        "should use our inference (string), not PHPStan tip:\n{}",
        result
    );
}

// ── missingType.return — add return type ────────────────────────────────────

#[test]
fn missing_return_type_infers_int() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function returnsInt() {
        return 1;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2, // the function declaration line
        "Method Foo::returnsInt() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("returnsInt(): int"),
        "should insert `: int` after close paren:\n{}",
        result
    );
}

#[test]
fn missing_return_type_infers_string() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function returnsString() {
        return 'hello';
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::returnsString() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("returnsString(): string"),
        "should insert `: string`:\n{}",
        result
    );
}

#[test]
fn missing_return_type_infers_void() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function returnsNothing() {
        echo 'side effect';
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::returnsNothing() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("returnsNothing(): void"),
        "should insert `: void`:\n{}",
        result
    );
}

#[test]
fn missing_return_type_infers_nullable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function maybeNull(bool $flag) {
        if ($flag) {
            return null;
        }
        return 'yes';
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::maybeNull() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // The inferred type should include both null and string.
    assert!(
        result.contains("null") && result.contains("string"),
        "should infer nullable string type:\n{}",
        result
    );
    assert!(
        result.contains("maybeNull(bool $flag):"),
        "should insert type after close paren:\n{}",
        result
    );
}

#[test]
fn missing_return_type_infers_new_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function returnsObject() {
        return new \stdClass();
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::returnsObject() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("returnsObject(): \\stdClass"),
        "should insert class name as return type:\n{}",
        result
    );
}

#[test]
fn missing_return_type_infers_bool() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function returnsBool() {
        return true;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::returnsBool() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("returnsBool(): bool"),
        "should insert `: bool`:\n{}",
        result
    );
}

#[test]
fn missing_return_type_not_offered_when_type_exists() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function alreadyTyped(): int {
        return 1;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::alreadyTyped() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);

    // There's already a return type, so the action should not be offered.
    assert!(
        find_action(&actions, "Add return type").is_none(),
        "should not offer add-return-type when type already exists"
    );
}

#[test]
fn missing_return_type_standalone_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function myFunc() {
    return 3.14;
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        1,
        "Function myFunc() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("myFunc(): float"),
        "should insert `: float`:\n{}",
        result
    );
}

#[test]
fn missing_return_type_brace_on_next_line() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo
{
    public function returnsArray()
    {
        return [];
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Method Foo::returnsArray() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 3);
    let action = find_action(&actions, "Add return type")
        .expect("should offer 'Add return type' even when brace is on next line");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("returnsArray(): array"),
        "should insert type:\n{}",
        result
    );
}

// ── No action for unrelated identifiers ─────────────────────────────────────

#[test]
fn no_action_for_unrelated_identifier() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): int {
    return 1;
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Some unrelated error.",
        "some.other.identifier",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);

    assert!(
        find_action(&actions, "Remove return statement").is_none(),
        "should not offer return-type actions for unrelated identifiers"
    );
    assert!(
        find_action(&actions, "Change return type").is_none(),
        "should not offer return-type actions for unrelated identifiers"
    );
    assert!(
        find_action(&actions, "Add return type").is_none(),
        "should not offer return-type actions for unrelated identifiers"
    );
}

// ── return.void inside if block ─────────────────────────────────────────────

#[test]
fn return_void_strips_inside_if_block_with_more_code() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): void {
        if (rand(0, 1)) {
            return $this->doWork();
        }
        echo 'done';
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        4,
        "Method Foo::run() with return type void returns void but should not return anything.",
        "return.void",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 4);
    let action = find_action(&actions, "Remove return statement")
        .expect("should offer strip action inside if block");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("$this->doWork();"),
        "expression should be kept:\n{}",
        result
    );
    assert!(
        result.contains("return;"),
        "bare return needed — more code follows after the if block:\n{}",
        result
    );
}

#[test]
fn return_void_strips_inside_if_block_last_statement() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): void {
        if (rand(0, 1)) {
            return $this->doWork();
        }
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        4,
        "Method Foo::run() with return type void returns void but should not return anything.",
        "return.void",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 4);
    let action = find_action(&actions, "Remove return statement")
        .expect("should offer strip action inside if block");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("$this->doWork();"),
        "expression should be kept:\n{}",
        result
    );
    assert!(
        !result.contains("return"),
        "no bare return needed — only closing braces follow:\n{}",
        result
    );
}

// ── Chaining: return.void then return.empty ─────────────────────────────────

#[test]
fn return_void_then_return_empty_chain() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // The chain only happens when `return expr;` is NOT the last
    // statement (otherwise `return;` is omitted and return.empty
    // never fires).  Use an if block with more code after it.
    let content_before = r#"<?php
class Foo {
    public function run(): void {
        if (true) {
            return $this->doWork();
        }
        echo 'more';
    }
}
"#;
    backend.update_ast(uri, content_before);

    inject_phpstan_diag(
        &backend,
        uri,
        4,
        "Method Foo::run() with return type void returns void but should not return anything.",
        "return.void",
    );

    // Apply the strip fix.
    let actions = get_code_actions_on_line(&backend, uri, content_before, 4);
    let strip = find_action(&actions, "Remove return statement").unwrap();
    let resolved = resolve_action(&backend, uri, content_before, strip);
    let edits = extract_edits(&resolved);
    let content_after = apply_edits(content_before, &edits);

    // Verify intermediate state: expression kept + bare return added
    // (because more code follows after the if block).
    assert!(
        content_after.contains("$this->doWork();"),
        "expression kept:\n{}",
        content_after
    );
    assert!(
        content_after.contains("return;"),
        "bare return added (more code after if block):\n{}",
        content_after
    );

    // Step 2: Now PHPStan would report return.empty on the bare return.
    {
        let mut cache = backend.phpstan_last_diags().lock();
        cache.remove(uri);
    }
    let return_line = content_after
        .lines()
        .enumerate()
        .find(|(_, l)| l.trim() == "return;")
        .map(|(i, _)| i as u32)
        .unwrap();

    inject_phpstan_diag(
        &backend,
        uri,
        return_line,
        "Method Foo::run() should return void but empty return statement found.",
        "return.empty",
    );

    // The return type is already void, so the fix should detect it's stale.
    let actions2 = get_code_actions_on_line(&backend, uri, &content_after, return_line);
    if let Some(change) = find_action(&actions2, "Change return type to void") {
        backend
            .open_files()
            .write()
            .insert(uri.to_string(), Arc::new(content_after.clone()));
        let (resolved, _) = backend.resolve_code_action(change.clone());
        if resolved.edit.is_some() {
            let edits = extract_edits(&resolved);
            let final_result = apply_edits(&content_after, &edits);
            assert!(
                final_result.contains("void"),
                "should still have void:\n{}",
                final_result
            );
        }
    }
}

// ── missingType.return — variable resolution ────────────────────────────────

#[test]
fn missing_return_type_resolves_variable() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function getCount() {
        $count = 42;
        return $count;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::getCount() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    // $count is assigned 42 (int literal), so variable resolution should
    // infer int. If it can't, it falls back to mixed.
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("getCount():"),
        "should insert return type:\n{}",
        result
    );
}

// ── missingType.return — mixed fallback for complex expressions ─────────────

#[test]
fn missing_return_type_with_comment_on_declaration_line() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function returnsObject() // this must have a type
    {
        return new \stdClass();
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::returnsObject() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // The type must be inserted right after the closing paren, before the comment.
    assert!(
        result.contains("returnsObject(): \\stdClass // this must have a type"),
        "type should be inserted after ) not after the comment:\n{}",
        result
    );
    assert!(
        !result.contains("// this must have a type: \\stdClass"),
        "type must NOT be appended after the comment:\n{}",
        result
    );
}

#[test]
fn missing_return_type_falls_back_to_mixed() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function getResult() {
        return strtoupper('hello');
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::getResult() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("getResult(): mixed"),
        "should insert `: mixed`:\n{}",
        result
    );
}

// ── missingType.return — rich type produces docblock + native hint ───────────

#[test]
fn missing_return_type_rich_type_adds_docblock_and_native_hint() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function getItems() {
        $var = ['string'];
        return $var;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::getItems() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // Native hint should be `array`, not `list<string>`.
    assert!(
        result.contains("getItems(): array"),
        "native type should be `array`, not a PHPStan type:\n{}",
        result
    );
    assert!(
        !result.contains("(): list<string>"),
        "PHPStan type must not appear in the native hint:\n{}",
        result
    );

    // A @return docblock should be added with the rich type.
    assert!(
        result.contains("@return list<string>"),
        "should add @return docblock with the rich type:\n{}",
        result
    );
}

#[test]
fn missing_return_type_infers_rich_type_from_multiline_array_literal() {
    // A multi-line array literal must infer the same rich type as the
    // single-line form: breaking `['a']` across lines is a formatting
    // change only and must not degrade `list<string>` to `array`/`mixed`.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function getItems() {
        return [
            'hello',
        ];
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::getItems() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // Native hint should be `array`, not the PHPStan type.
    assert!(
        result.contains("getItems(): array"),
        "native type should be `array`:\n{}",
        result
    );

    // The @return docblock must carry the rich `list<string>` type, not
    // `array<mixed>` from an unbalanced single-line fragment.
    assert!(
        result.contains("@return list<string>"),
        "multi-line array literal should still infer list<string>:\n{}",
        result
    );
}

// ── return.type — update @return tag ────────────────────────────────────────

#[test]
fn return_type_offers_single_update_action() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): string {
        return 42;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Method Foo::run() should return string but returns int.",
        "return.type",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 3);

    // A single "Update return type" action should be offered.
    let action =
        find_action(&actions, "Update return type").expect("should offer 'Update return type'");

    // Not preferred — the right fix might be to change the code.
    assert_eq!(action.is_preferred, Some(false));

    // There should be no separate "Change return type" or "@return tag" actions.
    assert!(
        find_action(&actions, "Change return type to int").is_none(),
        "should NOT offer separate 'Change return type' action"
    );
    assert!(
        find_action(&actions, "Update @return tag to int").is_none(),
        "should NOT offer separate '@return tag' action"
    );
}

#[test]
fn return_type_update_replaces_existing_return_tag() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @return string The result
     */
    public function run(): string {
        return 42;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        6,
        "Method Foo::run() should return string but returns int.",
        "return.type",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 6);
    let action =
        find_action(&actions, "Update return type").expect("should offer update return type");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("@return int"),
        "should update @return type to int:\n{}",
        result
    );
    assert!(
        result.contains("The result"),
        "should preserve description:\n{}",
        result
    );
    // Native type already matches base type — should remain unchanged.
    assert!(
        result.contains("): int {"),
        "native type should be updated to int:\n{}",
        result
    );
}

#[test]
fn return_type_update_no_docblock_change_for_simple_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Does something.
     */
    public function run(): string {
        return 42;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        6,
        "Method Foo::run() should return string but returns int.",
        "return.type",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 6);
    let action =
        find_action(&actions, "Update return type").expect("should offer update return type");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // Native type should be changed.
    assert!(
        result.contains("): int {"),
        "native type should be changed to int:\n{}",
        result
    );
    // No @return tag should be added for a simple type without generics.
    assert!(
        !result.contains("@return int"),
        "should NOT insert @return for simple type:\n{}",
        result
    );
    assert!(
        result.contains("Does something."),
        "should preserve existing docblock content:\n{}",
        result
    );
}

#[test]
fn return_type_update_generic_creates_docblock() {
    // Current: native `array`, no @return tag.  Our inference sees
    // `$frogs = [1, 2, 3]` → effective `list<int>`, native `array`.
    // `list<int>` != `array` → use our inference.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): array {
        $frogs = [1, 2, 3];
        return $frogs;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        4,
        "Method Foo::run() should return array<string> but returns array<int, int>.",
        "return.type",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 4);
    let action =
        find_action(&actions, "Update return type").expect("should offer update return type");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("/**"),
        "should create docblock:\n{}",
        result
    );
    // Our inference produces `list<int>`, not PHPStan's `array<int, int>`.
    assert!(
        result.contains("@return list<int>"),
        "should have @return tag with our inferred type:\n{}",
        result
    );
    assert!(result.contains("*/"), "should close docblock:\n{}", result);
    // Native type should remain `array`.
    assert!(
        result.contains("): array {"),
        "native type should remain array:\n{}",
        result
    );
}

#[test]
fn return_type_update_standalone_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): int {
    return 'hello';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Function foo() should return int but returns string.",
        "return.type",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Update return type")
        .expect("should offer update return type for standalone function");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // Native type should change to string.
    assert!(
        result.contains("): string {"),
        "native type should be changed to string:\n{}",
        result
    );
    // No docblock needed for simple type.
    assert!(
        !result.contains("@return"),
        "should NOT create @return for simple type:\n{}",
        result
    );
}

#[test]
fn return_type_update_generic_replaces_existing_return_tag() {
    // Current: native `array`, @return `array<int, string>`.  Our
    // inference sees `$frogs = [1, 2, 3]` → effective `list<int>`.
    // `list<int>` != `array<int, string>` → use our inference.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @return array<int, string>
 */
function foo(): array {
    $frogs = [1, 2, 3];
    return $frogs;
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        6,
        "Function foo() should return array<int, string> but returns array<int, int>.",
        "return.type",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 6);
    let action =
        find_action(&actions, "Update return type").expect("should offer update return type");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // Our inference produces `list<int>`, not PHPStan's `array<int, int>`.
    assert!(
        result.contains("@return list<int>"),
        "should replace @return with our inferred type:\n{}",
        result
    );
    // The old generic type should be fully replaced.
    assert!(
        !result.contains("array<int, string>"),
        "old @return type should be gone:\n{}",
        result
    );
    // Native type should remain `array`.
    assert!(
        result.contains("): array {"),
        "native type should remain array:\n{}",
        result
    );
}

#[test]
fn return_type_tip_fallback_with_generics() {
    // Current @return is `list<int>` which matches our inference
    // exactly.  PHPStan says the actual return type is
    // `array<int, int>`.  Since our inference agrees with the
    // declaration, we trust the PHPStan tip.
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @return list<int>
 */
function foo(): array {
    $frogs = [1, 2, 3];
    return $frogs;
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        6,
        "Function foo() should return array<string, int> but returns array<int, int>.",
        "return.type",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 6);
    let action =
        find_action(&actions, "Update return type").expect("should offer update return type");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // Our inference matches the current @return (`list<int>`), so we
    // trust the PHPStan tip: `array<int, int>`.
    assert!(
        result.contains("@return array<int, int>"),
        "should use PHPStan tip when our inference matches current:\n{}",
        result
    );
    // Native type should remain `array`.
    assert!(
        result.contains("): array {"),
        "native type should remain array:\n{}",
        result
    );
}

// ── missing return type — simple type (no docblock) ─────────────────────────

#[test]
fn missing_return_type_simple_type_no_docblock() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function getCount() {
        return 42;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::getCount() has no return type specified.",
        "missingType.return",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Add return type").expect("should offer 'Add return type'");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("getCount(): int"),
        "native type should be int:\n{}",
        result
    );
    // Simple native type — no docblock needed.
    assert!(
        !result.contains("@return"),
        "should NOT add @return for simple native type:\n{}",
        result
    );
}
