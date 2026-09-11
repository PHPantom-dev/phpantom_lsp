//! Integration tests for the "Promote constructor parameter" code action.
//!
//! These tests exercise the full pipeline: parsing PHP source, finding
//! a promotable constructor parameter under the cursor, and generating
//! the `WorkspaceEdit` that removes the property declaration, removes
//! the assignment, and adds a visibility modifier to the parameter.

use crate::common::{apply_workspace_edit, create_test_backend, get_code_actions_at};
use tower_lsp::lsp_types::*;

/// Find the "Promote to constructor property" code action from a list.
fn find_promote_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title == "Promote to constructor property" => {
            Some(ca)
        }
        _ => None,
    })
}

// ── Basic promotion ─────────────────────────────────────────────────────────

#[test]
fn promotes_private_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
    // Cursor on `$name` in the constructor parameter list (line 4, on "string $name").
    let actions = get_code_actions_at(&backend, uri, content, 4, 35);
    let action = find_promote_action(&actions).expect("should offer promote action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("private string $name)"),
        "parameter should have private visibility: {result}"
    );
    assert!(
        !result.contains("private string $name;"),
        "property declaration should be removed: {result}"
    );
    assert!(
        !result.contains("$this->name = $name;"),
        "assignment should be removed: {result}"
    );
}

#[test]
fn promotes_protected_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    protected int $age;

    public function __construct(int $age) {
        $this->age = $age;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 35);
    let action = find_promote_action(&actions).expect("should offer promote action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("protected int $age)"),
        "should use protected: {result}"
    );
}

#[test]
fn promotes_readonly_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private readonly string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 35);
    let action = find_promote_action(&actions).expect("should offer promote action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("private readonly string $name)"),
        "should include readonly: {result}"
    );
}

// ── Default value carry-over ────────────────────────────────────────────────

#[test]
fn carries_over_default_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private string $status = 'active';

    public function __construct(string $status) {
        $this->status = $status;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 35);
    let action = find_promote_action(&actions).expect("should offer promote action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("private string $status = 'active')"),
        "should carry default value: {result}"
    );
}

// ── Preceding docblock ──────────────────────────────────────────────────────

#[test]
fn removes_docblock_above_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    /** @var int */
    private int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 5, 32);
    let action = find_promote_action(&actions).expect("should offer promote action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("private int $bar)"),
        "parameter should have private visibility: {result}"
    );
    assert!(
        !result.contains("@var int"),
        "the property's docblock should be removed with it: {result}"
    );
}

#[test]
fn removes_multi_line_docblock_above_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    /**
     * The bar.
     *
     * @var int
     */
    private int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 9, 32);
    let action = find_promote_action(&actions).expect("should offer promote action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        !result.contains("The bar."),
        "the whole docblock should be removed: {result}"
    );
    assert!(
        !result.contains("/**"),
        "no docblock fragment should be left behind: {result}"
    );
}

#[test]
fn keeps_unrelated_comment_above_earlier_member() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    /** @var string */
    private string $keep = '';

    private int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 7, 32);
    let action = find_promote_action(&actions).expect("should offer promote action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("@var string"),
        "another property's docblock must be untouched: {result}"
    );
    assert!(
        result.contains("private string $keep = '';"),
        "the other property must be untouched: {result}"
    );
    assert!(
        !result.contains("private int $bar;"),
        "the promoted property declaration should be removed: {result}"
    );
}

// ── Attribute carry-over ────────────────────────────────────────────────────

#[test]
fn carries_attribute_onto_promoted_parameter() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    #[SomeAttr]
    private int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 5, 32);
    let action = find_promote_action(&actions).expect("should offer promote action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("#[SomeAttr] private int $bar)"),
        "the property's attribute should move onto the parameter: {result}"
    );
    assert_eq!(
        result.matches("#[SomeAttr]").count(),
        1,
        "the attribute should appear exactly once: {result}"
    );
}

#[test]
fn carries_several_attributes_onto_readonly_parameter() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    #[First]
    #[Second(name: 'bar')]
    private readonly int $bar;

    public function __construct(int $bar) {
        $this->bar = $bar;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 6, 32);
    let action = find_promote_action(&actions).expect("should offer promote action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("#[First] #[Second(name: 'bar')] private readonly int $bar)"),
        "both attributes should carry over ahead of the modifiers: {result}"
    );
    assert!(
        !result.contains("private readonly int $bar;"),
        "the property declaration should be removed: {result}"
    );
}

// ── Rejection cases ─────────────────────────────────────────────────────────

#[test]
fn no_action_for_non_constructor() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private string $name;

    public function setName(string $name): void {
        $this->name = $name;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 35);
    let action = find_promote_action(&actions);
    assert!(action.is_none(), "should not offer for non-constructor");
}

#[test]
fn no_action_for_already_promoted() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public function __construct(private string $name) {}
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 40);
    let action = find_promote_action(&actions);
    assert!(action.is_none(), "should not offer for already-promoted");
}

#[test]
fn no_action_when_no_matching_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public function __construct(string $name) {
        echo $name;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 35);
    let action = find_promote_action(&actions);
    assert!(
        action.is_none(),
        "should not offer when no matching property"
    );
}

#[test]
fn no_action_when_param_used_elsewhere() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private string $name;

    public function __construct(string $name) {
        $this->name = $name;
        echo $name;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 35);
    let action = find_promote_action(&actions);
    assert!(
        action.is_none(),
        "should not offer when param used elsewhere"
    );
}

#[test]
fn no_action_for_static_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private static string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 35);
    let action = find_promote_action(&actions);
    assert!(action.is_none(), "should not offer for static property");
}

// ── Multiple parameters ─────────────────────────────────────────────────────

#[test]
fn promotes_only_targeted_parameter() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private string $name;
    private int $age;

    public function __construct(string $name, int $age) {
        $this->name = $name;
        $this->age = $age;
    }
}
";
    // Cursor on `$age` parameter.
    let actions = get_code_actions_at(&backend, uri, content, 5, 50);
    let action = find_promote_action(&actions).expect("should offer promote for $age");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // $age should be promoted.
    assert!(
        result.contains("private int $age)"),
        "$age should be promoted: {result}"
    );
    // $name property and assignment should remain untouched.
    assert!(
        result.contains("private string $name;"),
        "$name property should remain: {result}"
    );
    assert!(
        result.contains("$this->name = $name;"),
        "$name assignment should remain: {result}"
    );
}

// ── Namespace ───────────────────────────────────────────────────────────────

#[test]
fn works_in_namespace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
namespace App\\Models;

class User {
    private string $email;

    public function __construct(string $email) {
        $this->email = $email;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 6, 35);
    let action = find_promote_action(&actions).expect("should work in namespace");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("private string $email)"),
        "should promote in namespace: {result}"
    );
}

// ── Union / nullable types ──────────────────────────────────────────────────

#[test]
fn promotes_with_union_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private int|string $id;

    public function __construct(int|string $id) {
        $this->id = $id;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 35);
    let action = find_promote_action(&actions).expect("should handle union types");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("private int|string $id)"),
        "should preserve union type: {result}"
    );
}

#[test]
fn promotes_with_nullable_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private ?string $name;

    public function __construct(?string $name) {
        $this->name = $name;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 35);
    let action = find_promote_action(&actions).expect("should handle nullable types");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("private ?string $name)"),
        "should preserve nullable type: {result}"
    );
}

// ── Code action kind ────────────────────────────────────────────────────────

#[test]
fn action_has_correct_kind() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    private string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 35);
    let action = find_promote_action(&actions).expect("should offer promote action");
    assert_eq!(
        action.kind,
        Some(CodeActionKind::new("refactor.rewrite")),
        "should be a refactor.rewrite action"
    );
}
