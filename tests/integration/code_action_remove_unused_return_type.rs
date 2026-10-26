//! Integration tests for the "Remove unused return type" code action.
//!
//! These tests exercise the full pipeline: inject a PHPStan diagnostic,
//! request code actions, resolve the chosen action, apply the edits,
//! and verify the resulting source text.
//!
//! Covers:
//! - `return.unusedType` — a union/intersection member is never returned

use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action, get_code_actions_on_line,
    inject_phpstan_diag, resolve_action,
};
use tower_lsp_server::gen_lsp_types::*;

// ── return.unusedType — remove unused type from union ────────────────────────

#[test]
fn removes_null_from_string_null_union() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): string|null {
    return 'hello';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        1,
        "Function foo() never returns null so it can be removed from the return type.",
        "return.unusedType",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let action = find_action(&actions, "Remove 'null' from return type")
        .expect("should offer remove unused type fix");

    assert_eq!(action.kind, Some(CodeActionKind::QuickFix));
    assert_eq!(action.is_preferred, Some(true));

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): string {"),
        "should remove null from union:\n{}",
        result
    );
}

#[test]
fn removes_string_from_string_null_union() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): string|null {
    return null;
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        1,
        "Function foo() never returns string so it can be removed from the return type.",
        "return.unusedType",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let action = find_action(&actions, "Remove 'string' from return type")
        .expect("should offer remove unused type fix");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): null {"),
        "should remove string from union:\n{}",
        result
    );
}

#[test]
fn removes_from_three_member_union() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): string|int|null {
    return 'hello';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        1,
        "Function foo() never returns null so it can be removed from the return type.",
        "return.unusedType",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let action = find_action(&actions, "Remove 'null' from return type")
        .expect("should offer remove unused type fix");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): int|string {") || result.contains("): string|int {"),
        "should remove null leaving two-member union:\n{}",
        result
    );
}

#[test]
fn removes_nullable_shorthand() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): ?string {
    return 'hello';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        1,
        "Function foo() never returns null so it can be removed from the return type.",
        "return.unusedType",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let action = find_action(&actions, "Remove 'null' from return type")
        .expect("should offer remove unused type fix for nullable shorthand");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): string {"),
        "should remove null from nullable shorthand:\n{}",
        result
    );
}

#[test]
fn removes_from_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): string|null {
        return 'hello';
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::bar() never returns null so it can be removed from the return type.",
        "return.unusedType",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 2);
    let action = find_action(&actions, "Remove 'null' from return type")
        .expect("should offer remove unused type fix for method");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): string {"),
        "should remove null from method return type:\n{}",
        result
    );
}

#[test]
fn updates_docblock_return_tag_too() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @return string|null The value
     */
    public function bar(): string|null {
        return 'hello';
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        5,
        "Method Foo::bar() never returns null so it can be removed from the return type.",
        "return.unusedType",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 5);
    let action = find_action(&actions, "Remove 'null' from return type")
        .expect("should offer remove unused type fix");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): string {"),
        "should update native return type:\n{}",
        result
    );
    assert!(
        result.contains("@return string"),
        "should update @return tag:\n{}",
        result
    );
    assert!(
        result.contains("The value"),
        "should preserve description:\n{}",
        result
    );
}

#[test]
fn no_action_for_single_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): string {
    return 'hello';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        1,
        "Function foo() never returns string so it can be removed from the return type.",
        "return.unusedType",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let action = find_action(&actions, "Remove 'string' from return type");
    assert!(
        action.is_none(),
        "should not offer action when removing would leave empty type"
    );
}

#[test]
fn no_action_for_unrelated_identifier() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): string|null {
    return 'hello';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        1,
        "Some other PHPStan message.",
        "other.identifier",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let action = find_action(&actions, "Remove");
    assert!(
        action.is_none(),
        "should not offer action for unrelated identifier"
    );
}

#[test]
fn removes_class_type_from_union() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): Foo|null {
    return null;
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        1,
        "Function foo() never returns Foo so it can be removed from the return type.",
        "return.unusedType",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 1);
    let action = find_action(&actions, "Remove 'Foo' from return type")
        .expect("should offer remove class type fix");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("): null {"),
        "should remove Foo from union:\n{}",
        result
    );
}

#[test]
fn removes_null_from_an_arrow_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function run(): array {
        $f = fn (): ?string => 'x';
        if ($this->ready()) {
            return [];
        }
        return [$f];
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Anonymous function never returns null so it can be removed from the return type.",
        "return.unusedType",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 3);
    let action = find_action(&actions, "Remove 'null' from return type")
        .expect("should offer remove unused type fix for an arrow function");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("$f = fn (): string => 'x';"),
        "should remove null from the arrow function's type:\n{}",
        result
    );
    assert!(
        result.contains("public function run(): array {"),
        "should leave the method's type alone:\n{}",
        result
    );
}
