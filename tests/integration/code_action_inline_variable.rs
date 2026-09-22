//! Integration tests for the "Inline variable" code action
//! (`refactor.inline`).
//!
//! These tests exercise the full two-phase pipeline: parsing PHP source
//! and running the safety checks (single assignment, pure RHS, no later
//! writes) to decide whether the action is offered at all, then
//! resolving the deferred action into the `WorkspaceEdit` that deletes
//! the assignment and substitutes the RHS at every read.

use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action, get_code_actions_at, position_of,
    resolve_action,
};
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Run the inline variable action with the cursor at the start of
/// `cursor_at` in `content`, and return the resulting edits (if offered).
fn run_inline(content: &str, cursor_at: &str) -> Option<Vec<TextEdit>> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, content);

    let position = position_of(content, cursor_at);
    let actions = get_code_actions_at(&backend, uri, content, position.line, position.character);
    let action = find_action(&actions, "Inline variable")?;

    // Phase 1 should have data but no edit.
    assert!(action.edit.is_none(), "Phase 1 should not compute edits");
    assert!(action.data.is_some(), "Phase 1 should attach resolve data");

    // Phase 2: resolve the action to get the workspace edit.
    let resolved = resolve_action(&backend, uri, content, action);
    Some(extract_edits(&resolved))
}

// ── Basic inline ────────────────────────────────────────────────

#[test]
fn inline_simple_variable() {
    let content = r#"<?php
function foo() {
    $name = $user->getName();
    echo $name;
}
"#;
    let edits = run_inline(content, "$name = $user->getName()").expect("action should be offered");
    let result = apply_edits(content, &edits);
    assert!(!result.contains("$name = "), "assignment should be removed");
    assert!(
        result.contains("echo $user->getName();"),
        "read should be replaced with RHS: got:\n{}",
        result
    );
}

#[test]
fn inline_variable_multiple_reads() {
    let content = r#"<?php
function foo($user) {
    $name = $user->email;
    echo $name;
    return $name;
}
"#;
    let edits = run_inline(content, "$name = $user->email").expect("action should be offered");
    let result = apply_edits(content, &edits);
    assert!(!result.contains("$name = "), "assignment should be removed");
    assert!(
        result.contains("echo $user->email;"),
        "first read should be replaced: got:\n{}",
        result
    );
    assert!(
        result.contains("return $user->email;"),
        "second read should be replaced: got:\n{}",
        result
    );
}

// ── Safety: reject multiple writes ──────────────────────────────

#[test]
fn reject_multiple_writes() {
    let content = r#"<?php
function foo() {
    $name = 'hello';
    $name = 'world';
    echo $name;
}
"#;
    assert!(
        run_inline(content, "$name = 'hello'").is_none(),
        "should reject: variable is reassigned"
    );
}

// ── Safety: reject side-effectful RHS with multiple reads ───────

#[test]
fn reject_side_effects_multiple_reads() {
    let content = r#"<?php
function foo() {
    $val = getResult();
    echo $val;
    return $val;
}
"#;
    assert!(
        run_inline(content, "$val = getResult()").is_none(),
        "should reject: side-effectful RHS with multiple reads"
    );
}

#[test]
fn allow_side_effects_single_read() {
    let content = r#"<?php
function foo() {
    $val = getResult();
    echo $val;
}
"#;
    assert!(
        run_inline(content, "$val = getResult()").is_some(),
        "should allow: side-effectful RHS with single read"
    );
}

// ── Parenthesisation ────────────────────────────────────────────

#[test]
fn adds_parens_for_binary_expression() {
    let content = r#"<?php
function foo($a, $b) {
    $sum = $a + $b;
    echo $sum;
}
"#;
    let edits = run_inline(content, "$sum = $a + $b").expect("action should be offered");
    let result = apply_edits(content, &edits);
    assert!(
        result.contains("echo ($a + $b);"),
        "binary expression should be wrapped in parens: got:\n{}",
        result
    );
}

#[test]
fn no_parens_for_simple_expression() {
    let content = r#"<?php
function foo($user) {
    $name = $user->name;
    echo $name;
}
"#;
    let edits = run_inline(content, "$name = $user->name").expect("action should be offered");
    let result = apply_edits(content, &edits);
    assert!(
        result.contains("echo $user->name;"),
        "property access should NOT be wrapped in parens: got:\n{}",
        result
    );
}

// ── Compound assignment → reject ────────────────────────────────

#[test]
fn reject_compound_assignment() {
    // The cursor is on a compound assignment (`.=`), which is not a
    // simple `$var = expr` assignment — should not be offered.
    let content = r#"<?php
function foo() {
    $name = 'hello';
    $name .= ' world';
    echo $name;
}
"#;
    assert!(
        run_inline(content, "$name .= ' world'").is_none(),
        "should reject: compound assignment is not a simple assignment"
    );
}

// ── Method body ─────────────────────────────────────────────────

#[test]
fn inline_in_method_body() {
    let content = r#"<?php
class Foo {
    public function bar() {
        $x = 42;
        return $x;
    }
}
"#;
    let edits = run_inline(content, "$x = 42").expect("action should be offered");
    let result = apply_edits(content, &edits);
    assert!(
        result.contains("return 42;"),
        "read should be replaced: got:\n{}",
        result
    );
    assert!(
        !result.contains("$x = 42"),
        "assignment should be deleted: got:\n{}",
        result
    );
}

// ── Ternary expression needs parens ─────────────────────────────

#[test]
fn adds_parens_for_ternary() {
    let content = r#"<?php
function foo($a) {
    $val = $a ? 'yes' : 'no';
    echo $val;
}
"#;
    let edits = run_inline(content, "$val = $a ? 'yes' : 'no'").expect("action should be offered");
    let result = apply_edits(content, &edits);
    assert!(
        result.contains("echo ($a ? 'yes' : 'no');"),
        "ternary should be wrapped in parens: got:\n{}",
        result
    );
}

// ── Code action kind ────────────────────────────────────────────

#[test]
fn code_action_kind_is_refactor_inline() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo() {
    $x = 1;
    echo $x;
}
"#;
    backend.update_ast(uri, content);

    let position = position_of(content, "$x = 1;");
    let actions = get_code_actions_at(&backend, uri, content, position.line, position.character);
    let action = find_action(&actions, "Inline variable").expect("action should be offered");

    assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_INLINE));
    // Phase 1: no edit, has data.
    assert!(action.edit.is_none(), "Phase 1 should not compute edits");
    assert!(action.data.is_some(), "Phase 1 should attach resolve data");
}

// ── Title format ────────────────────────────────────────────────

#[test]
fn title_includes_variable_name() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
function foo() {
    $myVar = 1;
    echo $myVar;
}
"#;
    backend.update_ast(uri, content);

    let position = position_of(content, "$myVar = 1");
    let actions = get_code_actions_at(&backend, uri, content, position.line, position.character);
    let action = find_action(&actions, "Inline variable").expect("action should be offered");

    assert_eq!(action.title, "Inline variable $myVar");
}

// ── No reads → no action ────────────────────────────────────────

#[test]
fn reject_no_reads() {
    let content = r#"<?php
function foo() {
    $x = 1;
}
"#;
    assert!(
        run_inline(content, "$x = 1;").is_none(),
        "should reject: variable has no reads"
    );
}

// ── ReadWrite (e.g. $x++) → reject ──────────────────────────────

#[test]
fn reject_read_write_usage() {
    let content = r#"<?php
function foo() {
    $x = 0;
    $x++;
}
"#;
    assert!(
        run_inline(content, "$x = 0").is_none(),
        "should reject: variable has read-write access ($x++)"
    );
}

// ── String literal RHS ──────────────────────────────────────────

#[test]
fn inline_string_literal() {
    let content = r#"<?php
function foo() {
    $msg = 'hello world';
    echo $msg;
}
"#;
    let edits = run_inline(content, "$msg = 'hello world'").expect("action should be offered");
    let result = apply_edits(content, &edits);
    assert!(
        result.contains("echo 'hello world';"),
        "string literal should be inlined: got:\n{}",
        result
    );
}

// ── Pure expression helpers ─────────────────────────────────────

#[test]
fn side_effect_detection_literals_are_pure() {
    // This is implicitly tested via inline_string_literal and
    // inline_variable_multiple_reads, but we also verify the helper
    // accepts property access with multiple reads.
    let content = r#"<?php
function foo($obj) {
    $x = $obj->name;
    echo $x;
    return $x;
}
"#;
    assert!(
        run_inline(content, "$x = $obj->name").is_some(),
        "pure property access should be inlinable with multiple reads"
    );
}

// ── Inline in namespace ─────────────────────────────────────────

#[test]
fn inline_in_namespaced_function() {
    let content = r#"<?php
namespace App;

function bar() {
    $val = 123;
    return $val;
}
"#;
    let edits = run_inline(content, "$val = 123").expect("action should be offered");
    let result = apply_edits(content, &edits);
    assert!(
        result.contains("return 123;"),
        "should inline in namespaced function: got:\n{}",
        result
    );
}

// ── new expression is side-effectful ────────────────────────────

#[test]
fn reject_new_with_multiple_reads() {
    let content = r#"<?php
function foo() {
    $obj = new stdClass();
    echo $obj;
    return $obj;
}
"#;
    assert!(
        run_inline(content, "$obj = new stdClass()").is_none(),
        "should reject: `new` is side-effectful with multiple reads"
    );
}

#[test]
fn allow_new_with_single_read() {
    let content = r#"<?php
function foo() {
    $obj = new stdClass();
    return $obj;
}
"#;
    assert!(
        run_inline(content, "$obj = new stdClass()").is_some(),
        "should allow: `new` with single read"
    );
}

// ── String interpolation ────────────────────────────────────────

#[test]
fn inline_with_string_interpolation() {
    let content = r#"<?php
class OrderProcessor {
    public function processOrder(Order $order): string {
        $total = $order->getTotal();
        return "total {$total}";
    }
}
"#;
    assert!(
        run_inline(content, "$total = $order->getTotal()").is_some(),
        "should offer inline for variable read inside string interpolation"
    );
}

// ── Reassigned variable after earlier writes/read-writes ────────

#[test]
fn inline_reassigned_variable_after_array_appends() {
    // The variable has earlier writes ($badges = []) and read-writes
    // ($badges[] = ...), but the cursor is on a later reassignment
    // that overwrites the variable.  After that reassignment there is
    // only a single read (return $badges), so the inline is safe:
    // `return self::computeBadges($model, $badges);`
    let content = r#"<?php
class BadgeHelper {
    public static function getBadges($model, $lang): array {
        $badges = [];

        if ($model->isDerma()) {
            $badges[] = new BadgeViewModel('derma');
        }

        if ($model->isProHairCare()) {
            $badges[] = new BadgeViewModel('pro-hair');
        }

        $badges = self::computeBadges($model, $badges);

        return $badges;
    }
}
"#;
    let edits = run_inline(content, "$badges = self::computeBadges")
        .expect("action should be offered for reassigned variable");
    let result = apply_edits(content, &edits);
    assert!(
        !result.contains("$badges = self::computeBadges"),
        "assignment should be removed:\n{}",
        result
    );
    assert!(
        result.contains("return self::computeBadges($model, $badges);"),
        "return should inline the RHS:\n{}",
        result
    );
}

#[test]
fn reject_reassigned_variable_with_later_mutation() {
    // After the reassignment there is a read-write ($badges[] = ...),
    // so inlining is NOT safe.
    let content = r#"<?php
function getBadges($model) {
    $badges = [];
    $badges = self::computeBadges($model, $badges);
    $badges[] = new BadgeViewModel('extra');
    return $badges;
}
"#;
    assert!(
        run_inline(content, "$badges = self::computeBadges").is_none(),
        "should reject: variable has read-write access after the assignment"
    );
}

#[test]
fn reject_reassigned_variable_with_later_overwrite() {
    // After the reassignment there is another write, so inlining
    // would lose that overwrite.
    let content = r#"<?php
function getBadges($model) {
    $badges = [];
    $badges = self::computeBadges($model, $badges);
    $badges = array_unique($badges);
    return $badges;
}
"#;
    assert!(
        run_inline(content, "$badges = self::computeBadges").is_none(),
        "should reject: variable has another write after the assignment"
    );
}
