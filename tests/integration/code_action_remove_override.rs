//! Integration tests for the "Remove `#[Override]`" code action.
//!
//! These tests exercise the full two-phase pipeline: a PHPStan
//! diagnostic with identifier `method.override`, `property.override`, or
//! `property.overrideAttribute` triggers a quickfix that strips the
//! `#[Override]` attribute, either by deleting the whole attribute line
//! or by removing just the `Override` entry from a shared attribute
//! list.  Diagnostics that land on the same line collapse into a single
//! action.

use crate::common::{
    create_test_backend, extract_edits, find_action_containing, find_actions, get_code_actions_at,
    inject_phpstan_diag, resolve_action,
};
use tower_lsp::lsp_types::*;

#[test]
fn offers_remove_override_action_for_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    #[\Override]
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Method Foo::bar() has #[\\Override] attribute but does not override any method.",
        "method.override",
    );

    let actions = get_code_actions_at(&backend, uri, content, 3, 4);
    let remove_action = find_action_containing(&actions, "Remove #[Override]");

    assert!(
        remove_action.is_some(),
        "should offer Remove #[Override] action"
    );

    let action = remove_action.unwrap();
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));
    assert!(
        action.title.contains("bar"),
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
    // Should remove the entire `#[\Override]` line.
    assert_eq!(edits[0].new_text, "");
}

#[test]
fn offers_remove_override_action_for_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    #[\Override]
    public string $baz = '';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Property Foo::$baz has #[\\Override] attribute but does not override any property.",
        "property.override",
    );

    let actions = get_code_actions_at(&backend, uri, content, 3, 4);
    let remove_action = find_action_containing(&actions, "Remove #[Override]");

    assert!(
        remove_action.is_some(),
        "should offer Remove #[Override] action for property"
    );

    let action = remove_action.unwrap();
    assert!(
        action.title.contains("$baz"),
        "title should mention property name: {}",
        action.title
    );

    // Phase 2: resolve the action.
    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "");
}

#[test]
fn offers_remove_override_action_for_override_attribute_on_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    #[\Override]
    public string $baz = '';
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Attribute class Override can be used with properties only on PHP 8.5 and later.",
        "property.overrideAttribute",
    );

    let actions = get_code_actions_at(&backend, uri, content, 3, 4);
    let remove_actions = find_actions(&actions, "Remove #[Override]");

    assert_eq!(
        remove_actions.len(),
        1,
        "should offer exactly one Remove #[Override] action"
    );

    let action = remove_actions[0];
    // Even though the overrideAttribute message doesn't contain
    // the property name, the title should still be generic when
    // it's the only diagnostic.
    assert_eq!(action.title, "Remove #[Override]");
    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));

    // Phase 2: resolve the action.
    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "");
}

#[test]
fn deduplicates_property_override_and_override_attribute() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    #[\Override]
    public string $baz = '';
}
"#;
    backend.update_ast(uri, content);

    // PHPStan reports both identifiers on the same line.
    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Property Foo::$baz has #[\\Override] attribute but does not override any property.",
        "property.override",
    );
    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Attribute class Override can be used with properties only on PHP 8.5 and later.",
        "property.overrideAttribute",
    );

    let actions = get_code_actions_at(&backend, uri, content, 3, 4);
    let remove_actions = find_actions(&actions, "Remove #[Override]");

    // Should produce exactly ONE action, not two.
    assert_eq!(
        remove_actions.len(),
        1,
        "should deduplicate into a single action, got: {:?}",
        remove_actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );

    let action = remove_actions[0];
    // Title should include the property name extracted from the
    // property.override diagnostic.
    assert!(
        action.title.contains("$baz"),
        "title should mention property name: {}",
        action.title
    );

    // Both diagnostics should be attached.
    let attached = action.diagnostics.as_ref().unwrap();
    assert_eq!(
        attached.len(),
        2,
        "should attach both diagnostics to the action"
    );

    // Phase 2: resolve clears both.
    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "");
}

#[test]
fn no_action_when_override_already_removed() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        2,
        "Method Foo::bar() has #[\\Override] attribute but does not override any method.",
        "method.override",
    );

    let actions = get_code_actions_at(&backend, uri, content, 2, 4);
    let remove_action = find_action_containing(&actions, "Remove #[Override]");

    assert!(
        remove_action.is_none(),
        "should NOT offer action when #[Override] already removed"
    );
}

#[test]
fn no_action_for_other_identifiers() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    #[\Override]
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(&backend, uri, 3, "Some other error.", "return.unusedType");

    let actions = get_code_actions_at(&backend, uri, content, 3, 4);
    let remove_action = find_action_containing(&actions, "Remove #[Override]");

    assert!(
        remove_action.is_none(),
        "should NOT offer action for non-override identifiers"
    );
}

#[test]
fn removes_override_from_shared_attribute_line() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    #[Override, Deprecated]
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        3,
        "Method Foo::bar() has #[\\Override] attribute but does not override any method.",
        "method.override",
    );

    let actions = get_code_actions_at(&backend, uri, content, 3, 4);
    let action =
        find_action_containing(&actions, "Remove #[Override]").expect("should offer action");

    // Phase 2: resolve.
    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);

    assert_eq!(edits.len(), 1);
    // Should keep the other attribute but remove Override.
    assert_eq!(edits[0].new_text, "    #[Deprecated]");
}
