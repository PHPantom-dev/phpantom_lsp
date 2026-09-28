//! Integration tests for the "Add `#[\ReturnTypeWillChange]`" code action.
//!
//! These tests exercise the full two-phase pipeline: a PHPStan
//! diagnostic with identifier `method.tentativeReturnType` triggers a
//! lightweight code action, and resolving it computes the edit that
//! inserts the attribute above the method declaration.

use crate::common::{
    create_test_backend, extract_edits, find_action_containing, get_code_actions_at,
    inject_phpstan_diag, resolve_action,
};
use tower_lsp::lsp_types::*;

// ── Integration: full code action via Backend ───────────────────

#[test]
fn offers_add_rtwc_action() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class MyCollection implements \Countable {
    public function count(): int { return 0; }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Return type (int) of method MyCollection::count() should be covariant with return type (int) of method Countable::count()\nMake it covariant, or use the #[\\ReturnTypeWillChange] attribute to temporarily suppress the error.",
        "method.tentativeReturnType",
    );

    let actions = get_code_actions_at(&backend, uri, content, 2, 4);
    let rtwc_action = find_action_containing(&actions, "ReturnTypeWillChange");

    assert!(
        rtwc_action.is_some(),
        "should offer Add #[\\ReturnTypeWillChange] action"
    );

    let action = rtwc_action.unwrap();
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));
    assert!(
        action.title.contains("count"),
        "title should mention method name: {}",
        action.title
    );

    // Phase 1: edit should be None, data should be Some.
    assert!(action.edit.is_none(), "Phase 1 should not compute the edit");
    assert!(
        action.data.is_some(),
        "Phase 1 should set data for deferred resolve"
    );

    // Phase 2: resolve the action to get the edit.
    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    assert_eq!(edits.len(), 1);
    assert!(edits[0].new_text.contains("#[\\ReturnTypeWillChange]"));
}

#[test]
fn no_action_when_rtwc_already_present() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class MyCollection implements \Countable {
    #[\ReturnTypeWillChange]
    public function count(): int { return 0; }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Return type (int) of method MyCollection::count() should be covariant with return type (int) of method Countable::count()",
        "method.tentativeReturnType",
    );

    let actions = get_code_actions_at(&backend, uri, content, 3, 4);
    let rtwc_action = find_action_containing(&actions, "ReturnTypeWillChange");

    assert!(
        rtwc_action.is_none(),
        "should NOT offer action when #[\\ReturnTypeWillChange] already present"
    );
}

#[test]
fn no_action_for_other_identifiers() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(&backend, uri, 2, "Some other error.", "return.unusedType");

    let actions = get_code_actions_at(&backend, uri, content, 2, 4);
    let rtwc_action = find_action_containing(&actions, "ReturnTypeWillChange");

    assert!(
        rtwc_action.is_none(),
        "should NOT offer action for non-tentativeReturnType identifiers"
    );
}

#[test]
fn inserts_before_existing_attributes() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class MyCollection implements \Countable {
    #[SomeAttr]
    public function count(): int { return 0; }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Return type (int) of method MyCollection::count() should be covariant with return type (int) of method Countable::count()",
        "method.tentativeReturnType",
    );

    let actions = get_code_actions_at(&backend, uri, content, 3, 4);
    let action =
        find_action_containing(&actions, "ReturnTypeWillChange").expect("should offer action");

    // Phase 2: resolve to get the edit.
    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);

    // The insertion position should be before the `#[SomeAttr]`
    // line (line 2), not before the `public function` line.
    assert_eq!(
        edits[0].range.start.line, 2,
        "should insert before existing attributes"
    );
    assert!(edits[0].new_text.contains("#[\\ReturnTypeWillChange]"));
}
