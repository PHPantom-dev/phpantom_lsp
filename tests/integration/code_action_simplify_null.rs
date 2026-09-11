//! Integration tests for the "Simplify with null coalescing / null-safe
//! operator" code action.
//!
//! These tests exercise the full pipeline: parsing PHP source, walking
//! the AST to find simplifiable ternary expressions at the cursor, and
//! generating the `WorkspaceEdit` that replaces the ternary with `??`
//! or `?->`.

use crate::common::{
    create_test_backend, extract_edit_text, get_code_actions_at, lsp_pos_to_offset,
};
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Find all "Simplify to …" code actions from a list of actions.
fn find_simplify_actions(actions: &[CodeActionOrCommand]) -> Vec<&CodeAction> {
    actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with("Simplify to ") => Some(ca),
            _ => None,
        })
        .collect()
}

/// Extract the range being replaced from a code action's workspace edit.
fn extract_edit_range(action: &CodeAction) -> Range {
    let edit = action.edit.as_ref().expect("action should have an edit");
    let changes = edit.changes.as_ref().expect("edit should have changes");
    let edits: Vec<&TextEdit> = changes.values().flat_map(|v| v.iter()).collect();
    assert_eq!(edits.len(), 1, "expected exactly one text edit");
    edits[0].range
}

// ─── isset($x) ? $x : $default → $x ?? $default ────────────────────────────

#[test]
fn isset_ternary_simplifies_to_coalescing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = isset($x) ? $x : 'default';
"#;
    backend.update_ast(uri, content);

    // Cursor on the `isset` keyword (line 1).
    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1, "actions: {:?}", simplify);
    assert_eq!(simplify[0].title, "Simplify to ??");
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'default'");
}

#[test]
fn isset_ternary_with_array_access() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = isset($data['key']) ? $data['key'] : null;
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$data['key'] ?? null");
}

#[test]
fn isset_ternary_no_match_when_then_differs() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = isset($x) ? $y : 'default';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert!(
        simplify.is_empty(),
        "should not offer simplification when then-branch differs from isset arg"
    );
}

#[test]
fn isset_multi_arg_not_simplified() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = isset($x, $y) ? $x : 'default';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert!(
        simplify.is_empty(),
        "should not simplify multi-argument isset"
    );
}

// ─── $x !== null ? $x : $default → $x ?? $default ──────────────────────────

#[test]
fn not_identical_null_simplifies_to_coalescing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = $x !== null ? $x : 'fallback';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(simplify[0].title, "Simplify to ??");
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'fallback'");
}

#[test]
fn null_not_identical_reversed_simplifies_to_coalescing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = null !== $x ? $x : 'fallback';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'fallback'");
}

// ─── $x === null ? $default : $x → $x ?? $default ──────────────────────────

#[test]
fn identical_null_simplifies_to_coalescing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = $x === null ? 'default' : $x;
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(simplify[0].title, "Simplify to ??");
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'default'");
}

#[test]
fn null_identical_reversed_simplifies_to_coalescing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = null === $x ? 'default' : $x;
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'default'");
}

// ─── $x !== null ? $x->foo() : null → $x?->foo() ───────────────────────────

#[test]
fn not_null_method_call_simplifies_to_nullsafe() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = $x !== null ? $x->getName() : null;
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(simplify[0].title, "Simplify to ?->");
    assert_eq!(extract_edit_text(simplify[0]), "$x?->getName()");
}

#[test]
fn not_null_property_access_simplifies_to_nullsafe() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = $x !== null ? $x->name : null;
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(simplify[0].title, "Simplify to ?->");
    assert_eq!(extract_edit_text(simplify[0]), "$x?->name");
}

#[test]
fn identical_null_else_method_call_simplifies_to_nullsafe() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // $x === null ? null : $x->getName()  →  $x?->getName()
    let content = r#"<?php
$result = $x === null ? null : $x->getName();
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(simplify[0].title, "Simplify to ?->");
    assert_eq!(extract_edit_text(simplify[0]), "$x?->getName()");
}

// ─── Action metadata ───────────────────────────────────────────────────────

#[test]
fn action_kind_is_refactor_rewrite() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = isset($x) ? $x : 'default';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(
        simplify[0].kind,
        Some(CodeActionKind::new("refactor.rewrite"))
    );
}

#[test]
fn action_is_marked_preferred() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = $x !== null ? $x : 'default';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(simplify[0].is_preferred, Some(true));
}

// ─── Edit range correctness ────────────────────────────────────────────────

#[test]
fn edit_range_covers_entire_ternary() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = isset($x) ? $x : 'default';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);
    assert_eq!(simplify.len(), 1);

    let range = extract_edit_range(simplify[0]);

    // The ternary starts at `isset` and ends at `'default'`.
    // Verify that the range spans the full ternary expression.
    let start_offset = lsp_pos_to_offset(content, range.start);
    let end_offset = lsp_pos_to_offset(content, range.end);
    let replaced = &content[start_offset..end_offset];

    assert!(
        replaced.contains("isset"),
        "range should include isset: got {:?}",
        replaced
    );
    assert!(
        replaced.contains("'default'"),
        "range should include the else branch: got {:?}",
        replaced
    );
}

// ─── Context: inside functions and methods ──────────────────────────────────

#[test]
fn works_inside_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo($x) {
    return isset($x) ? $x : 'default';
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 14);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'default'");
}

#[test]
fn works_inside_class_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar($x) {
        return $x !== null ? $x : 'default';
    }
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 16);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'default'");
}

#[test]
fn works_inside_namespace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App;

function foo($x) {
    return isset($x) ? $x : null;
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 4, 14);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? null");
}

// ─── Context: control structures ────────────────────────────────────────────

#[test]
fn works_inside_if_body() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo($x) {
    if (true) {
        $result = isset($x) ? $x : 'fallback';
    }
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 20);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'fallback'");
}

#[test]
fn works_inside_foreach() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo($items) {
    foreach ($items as $item) {
        $name = $item !== null ? $item->name : null;
    }
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 20);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(simplify[0].title, "Simplify to ?->");
    assert_eq!(extract_edit_text(simplify[0]), "$item?->name");
}

// ─── Negative cases ────────────────────────────────────────────────────────

#[test]
fn no_action_for_unrelated_ternary() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = $x > 0 ? 'positive' : 'negative';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert!(simplify.is_empty(), "should not simplify unrelated ternary");
}

#[test]
fn no_action_for_short_ternary() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = $x ?: 'default';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert!(
        simplify.is_empty(),
        "should not simplify short ternary (?:)"
    );
}

#[test]
fn no_action_when_cursor_not_on_ternary() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$a = 1;
$result = isset($x) ? $x : 'default';
"#;
    backend.update_ast(uri, content);

    // Cursor on `$a = 1` (line 1), not on the ternary (line 2).
    let actions = get_code_actions_at(&backend, uri, content, 1, 2);
    let simplify = find_simplify_actions(&actions);

    assert!(
        simplify.is_empty(),
        "should not offer action when cursor is not on the ternary"
    );
}

#[test]
fn no_action_when_not_identical_but_then_differs() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // $x !== null ? $y : 'default'  — then-branch is $y, not $x
    let content = r#"<?php
$result = $x !== null ? $y : 'default';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert!(
        simplify.is_empty(),
        "should not simplify when then-branch differs from comparison subject"
    );
}

// ─── Parenthesized conditions ───────────────────────────────────────────────

#[test]
fn parenthesized_condition_still_simplifies() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = ($x !== null) ? $x : 'default';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'default'");
}

#[test]
fn parenthesized_null_still_simplifies() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$result = $x !== (null) ? $x : 'default';
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 12);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'default'");
}

// ─── Complex expressions ───────────────────────────────────────────────────

#[test]
fn this_property_coalescing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    private $bar;
    public function baz() {
        return $this->bar !== null ? $this->bar : 'default';
    }
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 4, 18);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$this->bar ?? 'default'");
}

#[test]
fn this_property_nullsafe_method_call() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    private $bar;
    public function baz() {
        return $this->bar !== null ? $this->bar->getName() : null;
    }
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 4, 18);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(simplify[0].title, "Simplify to ?->");
    assert_eq!(extract_edit_text(simplify[0]), "$this->bar?->getName()");
}

// ─── In assignment context ─────────────────────────────────────────────────

#[test]
fn simplifies_ternary_on_rhs_of_assignment() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
$name = $user !== null ? $user : $defaultUser;
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 10);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$user ?? $defaultUser");
}

// ─── In function call argument ─────────────────────────────────────────────

#[test]
fn simplifies_ternary_inside_function_call_arg() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
foo(isset($x) ? $x : 'default');
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 1, 6);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'default'");
}

// ─── In return statement ───────────────────────────────────────────────────

#[test]
fn simplifies_ternary_in_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo($x) {
    return $x === null ? 'none' : $x;
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 14);
    let simplify = find_simplify_actions(&actions);

    assert_eq!(simplify.len(), 1);
    assert_eq!(extract_edit_text(simplify[0]), "$x ?? 'none'");
}
