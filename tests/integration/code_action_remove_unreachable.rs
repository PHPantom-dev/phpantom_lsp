//! Integration tests for the "Remove unreachable code" code action.
//!
//! These tests exercise the full pipeline: inject a PHPStan diagnostic
//! with identifier `deadCode.unreachable`, request code actions, resolve
//! the chosen action, apply the edits, and verify the resulting source.
//!
//! The action removes ALL dead code from the diagnostic line through the
//! enclosing block's closing `}`, not just a single statement.  PHPStan
//! reports one diagnostic per block (on the first dead line).

use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action, get_code_actions_on_line,
    inject_phpstan_diag, resolve_action,
};
use tower_lsp_server::gen_lsp_types::*;

const UNREACHABLE_MSG: &str = "Unreachable statement - code above always terminates.";
const UNREACHABLE_ID: &str = "deadCode.unreachable";

// ── Removes all dead code in the block ──────────────────────────────────────

#[test]
fn removes_all_dead_code_after_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): int {
        return 1;
        $b = 'second';
        $a = 'first';
        echo $a . $b;
    }
}
"#;
    backend.update_ast(uri, content);

    // PHPStan reports one diagnostic on the first unreachable line.
    inject_phpstan_diag(&backend, uri, 4, UNREACHABLE_MSG, UNREACHABLE_ID);

    let actions = get_code_actions_on_line(&backend, uri, content, 4);
    let action =
        find_action(&actions, "Remove unreachable code").expect("should offer removal action");

    assert_eq!(action.kind, Some(CodeActionKind::QuickFix));
    assert_eq!(action.is_preferred, Some(true));

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // ALL dead code should be gone, not just the first statement.
    assert!(
        !result.contains("$b = 'second'"),
        "first dead statement should be removed:\n{}",
        result
    );
    assert!(
        !result.contains("$a = 'first'"),
        "second dead statement should be removed:\n{}",
        result
    );
    assert!(
        !result.contains("echo $a"),
        "third dead statement should be removed:\n{}",
        result
    );
    assert!(
        result.contains("return 1;"),
        "live code should remain:\n{}",
        result
    );
    // The closing brace should be preserved.
    assert!(
        result.contains("}"),
        "closing brace should be preserved:\n{}",
        result
    );
}

#[test]
fn removes_single_dead_statement() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): int {
    return 1;
    echo 'dead';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(&backend, uri, 3, UNREACHABLE_MSG, UNREACHABLE_ID);

    let actions = get_code_actions_on_line(&backend, uri, content, 3);
    let action =
        find_action(&actions, "Remove unreachable code").expect("should offer removal action");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        !result.contains("echo 'dead'"),
        "dead statement should be removed:\n{}",
        result
    );
    assert!(
        result.contains("return 1;"),
        "return should remain:\n{}",
        result
    );
}

// ── Dead code inside nested blocks ──────────────────────────────────────────

#[test]
fn removes_dead_code_inside_if_block_only() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): int {
        if (true) {
            return 1;
            echo 'dead inside if';
        }
        return 0;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(&backend, uri, 5, UNREACHABLE_MSG, UNREACHABLE_ID);

    let actions = get_code_actions_on_line(&backend, uri, content, 5);
    let action =
        find_action(&actions, "Remove unreachable code").expect("should handle nested blocks");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        !result.contains("echo 'dead inside if'"),
        "dead code inside if should be removed:\n{}",
        result
    );
    assert!(
        result.contains("return 1;"),
        "return inside if should remain:\n{}",
        result
    );
    assert!(
        result.contains("return 0;"),
        "code after the if block should remain:\n{}",
        result
    );
}

#[test]
fn removes_dead_code_with_nested_braces_in_dead_region() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): int {
    return 1;
    if (true) {
        echo 'nested dead';
    }
    echo 'also dead';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(&backend, uri, 3, UNREACHABLE_MSG, UNREACHABLE_ID);

    let actions = get_code_actions_on_line(&backend, uri, content, 3);
    let action =
        find_action(&actions, "Remove unreachable code").expect("should handle dead if blocks");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        !result.contains("if (true)"),
        "dead if block should be removed:\n{}",
        result
    );
    assert!(
        !result.contains("echo 'nested dead'"),
        "nested dead code should be removed:\n{}",
        result
    );
    assert!(
        !result.contains("echo 'also dead'"),
        "dead code after nested block should be removed:\n{}",
        result
    );
    assert!(
        result.contains("return 1;"),
        "live code should remain:\n{}",
        result
    );
}

// ── No action for wrong identifier ──────────────────────────────────────────

#[test]
fn no_action_for_unrelated_identifier() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): int {
    return 1;
    echo 'dead';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Some other error.",
        "some.other.identifier",
    );

    let actions = get_code_actions_on_line(&backend, uri, content, 3);

    assert!(
        find_action(&actions, "Remove unreachable").is_none(),
        "should not offer removal for unrelated identifiers"
    );
}

// ── No action when cursor is on a different line ────────────────────────────

#[test]
fn no_action_when_cursor_not_on_diagnostic_line() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo(): int {
    return 1;
    echo 'dead';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(&backend, uri, 3, UNREACHABLE_MSG, UNREACHABLE_ID);

    // Request actions on line 2 (the return line), not line 3.
    let actions = get_code_actions_on_line(&backend, uri, content, 2);

    assert!(
        find_action(&actions, "Remove unreachable").is_none(),
        "should not offer removal when cursor is on a different line"
    );
}

// ── Method with only dead code after return ─────────────────────────────────

#[test]
fn result_is_clean_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): int {
        return 1;
        $b = 'second';
        $a = 'first';
        echo $a . $b;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(&backend, uri, 4, UNREACHABLE_MSG, UNREACHABLE_ID);

    let actions = get_code_actions_on_line(&backend, uri, content, 4);
    let action = find_action(&actions, "Remove unreachable code").unwrap();

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    // The result should be a clean method with just the return.
    let expected = r#"<?php
class Foo {
    public function bar(): int {
        return 1;
    }
}
"#;
    assert_eq!(result, expected, "result should be a clean method");
}
