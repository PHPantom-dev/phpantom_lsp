//! Integration tests for the "Generate property hooks" code action (PHP 8.4+).
//!
//! These tests exercise the full pipeline: with the cursor on a property
//! declaration inside a class-like body, the server offers "Generate get
//! hook", "Generate set hook", and "Generate get and set hooks", and the
//! edit each one carries rewrites the property with the requested hooks.
//!
//! Which actions are offered depends on the property and its container:
//! static and readonly properties (and every property of a `readonly
//! class`) get none, an already-hooked property only gets the missing
//! hook, and an interface generates abstract hook signatures.
//!
//! The unit tests for the hook text builders themselves live next to those
//! private helpers in `src/code_actions/generate_property_hooks.rs`.

use crate::common::{
    create_test_backend, extract_edit_text, find_action_titled, find_actions_containing,
    get_code_actions_in_range, position_of,
};
use tower_lsp::lsp_types::*;

// ── Which actions are offered ───────────────────────────────────────────────

#[test]
fn offers_all_three_actions_for_plain_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n    public string $name;\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let hook_actions = find_actions_containing(&actions, "hook");

    assert_eq!(
        hook_actions.len(),
        3,
        "Expected 3 hook actions, got: {:?}",
        hook_actions
            .iter()
            .map(|ca| ca.title.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn readonly_property_offers_no_hooks() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n    public readonly string $name;\n}";

    let position = position_of(content, "public readonly");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let hook_actions = find_actions_containing(&actions, "hook");

    assert_eq!(
        hook_actions.len(),
        0,
        "readonly properties cannot have hooks in PHP 8.4"
    );
}

#[test]
fn static_property_offers_no_hooks() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n    public static string $name;\n}";

    let position = position_of(content, "public static");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let hook_actions = find_actions_containing(&actions, "hook");

    assert_eq!(
        hook_actions.len(),
        0,
        "static properties should not offer hook actions"
    );
}

#[test]
fn interface_property_generates_abstract_hooks() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\ninterface Foo {\n    public string $name { get; }\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let hook_actions = find_actions_containing(&actions, "hook");

    // The property already has `get;` so only `set` should be offered.
    assert_eq!(
        hook_actions.len(),
        1,
        "interface with existing get hook should only offer set, got: {:?}",
        hook_actions
            .iter()
            .map(|ca| ca.title.clone())
            .collect::<Vec<_>>()
    );

    let ca = hook_actions[0];
    assert_eq!(ca.title, "Generate set hook");
    // Verify the generated text uses abstract hook syntax.
    let new_text = extract_edit_text(ca);
    assert!(
        new_text.contains("set;"),
        "interface hook should be abstract, got: {new_text}"
    );
}

// ── The PHP each action generates ───────────────────────────────────────────

#[test]
fn get_hook_edit_contains_correct_php() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n    public string $name;\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let get_action = find_action_titled(&actions, "Generate get hook");

    assert!(
        get_action.is_some(),
        "should have a 'Generate get hook' action"
    );

    if let Some(ca) = get_action {
        let new_text = extract_edit_text(ca);

        assert!(
            new_text.contains("get => $this->name;"),
            "get hook should reference $this->name, got: {new_text}"
        );
        assert!(
            new_text.starts_with("public string $name {"),
            "should start with property declaration, got: {new_text}"
        );
    }
}

#[test]
fn set_hook_edit_contains_correct_php() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n    public string $name;\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let set_action = find_action_titled(&actions, "Generate set hook");

    assert!(
        set_action.is_some(),
        "should have a 'Generate set hook' action"
    );

    if let Some(ca) = set_action {
        let new_text = extract_edit_text(ca);

        assert!(
            new_text.contains("set => $this->name = $value;"),
            "set hook should assign $value, got: {new_text}"
        );
    }
}

#[test]
fn both_hooks_edit_contains_correct_php() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n    public string $name;\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let both_action = find_action_titled(&actions, "Generate get and set hooks");

    assert!(
        both_action.is_some(),
        "should have a 'Generate get and set hooks' action"
    );

    if let Some(ca) = both_action {
        let new_text = extract_edit_text(ca);

        assert!(
            new_text.contains("get => $this->name;"),
            "both hooks should include get, got: {new_text}"
        );
        assert!(
            new_text.contains("set => $this->name = $value;"),
            "both hooks should include set, got: {new_text}"
        );
    }
}

#[test]
fn property_with_default_preserves_default() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n    public string $name = 'default';\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let get_action = find_action_titled(&actions, "Generate get hook");

    if let Some(ca) = get_action {
        let new_text = extract_edit_text(ca);

        assert!(
            new_text.contains("= 'default'"),
            "default value should be preserved, got: {new_text}"
        );
    }
}

// ── Properties that already have hooks ──────────────────────────────────────

#[test]
fn existing_hooked_property_with_get_only_offers_set() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content =
        "<?php\nclass Foo {\n    public string $name {\n        get => $this->name;\n    }\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let hook_actions = find_actions_containing(&actions, "hook");

    assert_eq!(
        hook_actions.len(),
        1,
        "property with existing get hook should only offer set, got: {:?}",
        hook_actions
            .iter()
            .map(|ca| ca.title.clone())
            .collect::<Vec<_>>()
    );

    assert_eq!(hook_actions[0].title, "Generate set hook");
}

#[test]
fn existing_hooked_property_with_both_hooks_offers_nothing() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n    public string $name {\n        get => $this->name;\n        set => $this->name = $value;\n    }\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let hook_actions = find_actions_containing(&actions, "hook");

    assert_eq!(
        hook_actions.len(),
        0,
        "property with both hooks should not offer any hook actions"
    );
}

// ── Other class-like kinds and property shapes ──────────────────────────────

#[test]
fn no_hooks_for_enum_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    // Enums can't have regular properties, but let's make sure
    // the code action doesn't crash or offer hooks for enum members.
    let content = "<?php\nenum Foo: string {\n    case Bar = 'bar';\n}";

    let position = position_of(content, "case");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let hook_actions = find_actions_containing(&actions, "hook");

    assert_eq!(hook_actions.len(), 0, "enums should not offer hook actions");
}

#[test]
fn untyped_property_generates_hooks() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n    public $name;\n}";

    let position = position_of(content, "public $name");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let hook_actions = find_actions_containing(&actions, "hook");

    assert_eq!(
        hook_actions.len(),
        3,
        "untyped property should offer all three hook actions, got: {:?}",
        hook_actions
            .iter()
            .map(|ca| ca.title.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn multi_variable_property_offers_no_hooks() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n    public string $a, $b;\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let hook_actions = find_actions_containing(&actions, "hook");

    assert_eq!(
        hook_actions.len(),
        0,
        "multi-variable properties cannot have hooks"
    );
}

#[test]
fn trait_property_generates_concrete_hooks() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\ntrait Foo {\n    public string $name;\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let get_action = find_action_titled(&actions, "Generate get hook");

    assert!(get_action.is_some(), "trait should offer hook actions");

    if let Some(ca) = get_action {
        let new_text = extract_edit_text(ca);
        assert!(
            new_text.contains("get => $this->name;"),
            "trait hooks should be concrete, got: {new_text}"
        );
    }
}

#[test]
fn readonly_class_property_offers_no_hooks() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nfinal readonly class Foo {\n    public string $name;\n}";

    let position = position_of(content, "public string");
    let actions = get_code_actions_in_range(&backend, uri, content, Range::new(position, position));
    let hook_actions = find_actions_containing(&actions, "hook");

    assert_eq!(
        hook_actions.len(),
        0,
        "readonly class properties cannot have hooks in PHP 8.4"
    );
}
