//! Integration tests for the "Generate getter/setter" code actions.
//!
//! These tests exercise the full pipeline: a cursor on a property
//! declaration produces up to three `refactor` actions ("Generate
//! getter", "Generate setter", "Generate getter and setter"), each
//! carrying the `TextEdit` that inserts the accessor methods.
//!
//! They cover which actions are offered (readonly properties get no
//! setter, an existing `getX`/`setX` suppresses the matching action,
//! case-insensitively) and the PHP the edits actually insert (`is`
//! prefix for `bool`, `static` methods for static properties).

use crate::common::{
    create_test_backend, extract_edits, find_action_titled, find_actions,
    get_code_actions_in_range, position_of,
};
use tower_lsp::lsp_types::*;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// The titles of the getter/setter actions among `actions`, for
/// assertion messages.
fn generate_titles(actions: &[CodeActionOrCommand]) -> Vec<String> {
    find_actions(actions, "Generate getter")
        .into_iter()
        .chain(find_actions(actions, "Generate setter"))
        .map(|a| a.title.clone())
        .collect()
}

/// The code actions offered with the cursor at the start of `needle`.
fn actions_at(
    backend: &phpantom_lsp::Backend,
    content: &str,
    needle: &str,
) -> Vec<CodeActionOrCommand> {
    let pos = position_of(content, needle);
    get_code_actions_in_range(backend, "file:///test.php", content, Range::new(pos, pos))
}

// ── Which actions are offered ───────────────────────────────────────────────

#[test]
fn offers_all_three_actions_for_regular_property() {
    let backend = create_test_backend();
    let content = "<?php\nclass Foo {\n    private string $name;\n}";
    let actions = actions_at(&backend, content, "private string $name");
    let titles = generate_titles(&actions);

    assert!(
        titles.iter().any(|t| t == "Generate getter"),
        "should offer getter: {titles:?}"
    );
    assert!(
        titles.iter().any(|t| t == "Generate setter"),
        "should offer setter: {titles:?}"
    );
    assert!(
        titles.iter().any(|t| t == "Generate getter and setter"),
        "should offer both: {titles:?}"
    );
}

#[test]
fn readonly_property_only_offers_getter() {
    let backend = create_test_backend();
    let content = "<?php\nclass Foo {\n    public readonly int $id;\n}";
    let actions = actions_at(&backend, content, "public readonly");
    let titles = generate_titles(&actions);

    assert!(
        titles.iter().any(|t| t == "Generate getter"),
        "should offer getter for readonly: {titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t == "Generate setter"),
        "should NOT offer setter for readonly: {titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t == "Generate getter and setter"),
        "should NOT offer both for readonly: {titles:?}"
    );
}

#[test]
fn skips_when_getter_already_exists() {
    let backend = create_test_backend();
    let content = "<?php\nclass Foo {\n    private string $name;\n    public function getName(): string { return $this->name; }\n}";
    let actions = actions_at(&backend, content, "private string $name");
    let titles = generate_titles(&actions);

    assert!(
        !titles.iter().any(|t| t == "Generate getter"),
        "should NOT offer getter when it exists: {titles:?}"
    );
    assert!(
        titles.iter().any(|t| t == "Generate setter"),
        "should still offer setter: {titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t == "Generate getter and setter"),
        "should NOT offer both when getter exists: {titles:?}"
    );
}

#[test]
fn skips_when_setter_already_exists() {
    let backend = create_test_backend();
    let content = "<?php\nclass Foo {\n    private string $name;\n    public function setName(string $name): self { $this->name = $name; return $this; }\n}";
    let actions = actions_at(&backend, content, "private string $name");
    let titles = generate_titles(&actions);

    assert!(
        titles.iter().any(|t| t == "Generate getter"),
        "should still offer getter: {titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t == "Generate setter"),
        "should NOT offer setter when it exists: {titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t == "Generate getter and setter"),
        "should NOT offer both when setter exists: {titles:?}"
    );
}

#[test]
fn no_actions_when_both_exist() {
    let backend = create_test_backend();
    let content = "<?php\nclass Foo {\n    private string $name;\n    public function getName(): string { return $this->name; }\n    public function setName(string $name): self { $this->name = $name; return $this; }\n}";
    let actions = actions_at(&backend, content, "private string $name");
    let getter_setter_titles = generate_titles(&actions);

    assert!(
        getter_setter_titles.is_empty(),
        "should not offer any getter/setter actions: {getter_setter_titles:?}"
    );
}

#[test]
fn case_insensitive_method_check() {
    let backend = create_test_backend();
    let content = "<?php\nclass Foo {\n    private string $name;\n    public function GETNAME(): string { return $this->name; }\n}";
    let actions = actions_at(&backend, content, "private string $name");
    let titles = generate_titles(&actions);

    assert!(
        !titles.iter().any(|t| t == "Generate getter"),
        "GETNAME should count as existing getter (case insensitive): {titles:?}"
    );
}

// ── The PHP the edits insert ────────────────────────────────────────────────

#[test]
fn getter_edit_contains_correct_php() {
    let backend = create_test_backend();
    let content = "<?php\nclass Foo {\n    private string $name;\n}\n";
    let actions = actions_at(&backend, content, "private string $name");

    let ca = find_action_titled(&actions, "Generate getter").expect("should have getter action");
    let edits = extract_edits(ca);
    let new_text = &edits[0].new_text;

    assert!(
        new_text.contains("public function getName(): string"),
        "correct getter signature: {new_text}"
    );
    assert!(
        new_text.contains("return $this->name;"),
        "correct getter body: {new_text}"
    );
}

#[test]
fn setter_edit_contains_correct_php() {
    let backend = create_test_backend();
    let content = "<?php\nclass Foo {\n    private string $name;\n}\n";
    let actions = actions_at(&backend, content, "private string $name");

    let ca = find_action_titled(&actions, "Generate setter").expect("should have setter action");
    let edits = extract_edits(ca);
    let new_text = &edits[0].new_text;

    assert!(
        new_text.contains("public function setName(string $name): self"),
        "correct setter signature: {new_text}"
    );
    assert!(
        new_text.contains("$this->name = $name;"),
        "correct setter assignment: {new_text}"
    );
    assert!(
        new_text.contains("return $this;"),
        "correct setter return: {new_text}"
    );
}

#[test]
fn bool_property_uses_is_prefix_in_action() {
    let backend = create_test_backend();
    let content = "<?php\nclass Foo {\n    private bool $active;\n}\n";
    let actions = actions_at(&backend, content, "private bool $active");

    let ca = find_action_titled(&actions, "Generate getter").expect("should have getter action");
    let edits = extract_edits(ca);
    let new_text = &edits[0].new_text;

    assert!(
        new_text.contains("public function isActive(): bool"),
        "bool getter uses is prefix: {new_text}"
    );
}

#[test]
fn static_property_generates_static_methods() {
    let backend = create_test_backend();
    let content = "<?php\nclass Foo {\n    private static int $count;\n}\n";
    let actions = actions_at(&backend, content, "private static int $count");

    // Check getter.
    let ca = find_action_titled(&actions, "Generate getter").expect("should have getter action");
    let edits = extract_edits(ca);
    assert!(
        edits[0]
            .new_text
            .contains("public static function getCount(): int"),
        "static getter: {}",
        edits[0].new_text
    );
    assert!(
        edits[0].new_text.contains("return self::$count;"),
        "static getter body: {}",
        edits[0].new_text
    );

    // Check setter.
    let ca = find_action_titled(&actions, "Generate setter").expect("should have setter action");
    let edits = extract_edits(ca);
    assert!(
        edits[0]
            .new_text
            .contains("public static function setCount(int $count): void"),
        "static setter: {}",
        edits[0].new_text
    );
    assert!(
        edits[0].new_text.contains("self::$count = $count;"),
        "static setter body: {}",
        edits[0].new_text
    );
}
