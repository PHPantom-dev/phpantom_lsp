//! Integration tests for the "Generate constructor" code action.
//!
//! These tests exercise the full pipeline: parsing PHP source, detecting
//! qualifying properties, and generating a `WorkspaceEdit` that inserts
//! a constructor with parameters and assignments for each non-static
//! property (including readonly properties, which must be initialized
//! in the constructor).

use crate::common::{apply_workspace_edit, create_test_backend, get_code_actions_at};
use tower_lsp::lsp_types::*;

/// Find the "Generate constructor" code action from a list.
fn find_generate_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title == "Generate constructor" => Some(ca),
        _ => None,
    })
}

/// Find the "Generate promoted constructor" code action from a list.
fn find_promoted_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title == "Generate promoted constructor" => {
            Some(ca)
        }
        _ => None,
    })
}

// ── Basic generation ────────────────────────────────────────────────────────

#[test]
fn generates_constructor_for_single_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer generate constructor action");
    assert_eq!(
        action.kind,
        Some(CodeActionKind::REFACTOR_REWRITE),
        "should be refactor.rewrite"
    );
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("public function __construct(string $name)"),
        "should generate constructor with typed param: {result}"
    );
    assert!(
        result.contains("$this->name = $name;"),
        "should generate assignment: {result}"
    );
}

#[test]
fn generates_constructor_for_multiple_properties() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class User {
    public string $name;
    public int $age;
    public string $email;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("string $name, int $age, string $email"),
        "should include all params: {result}"
    );
    assert!(
        result.contains("$this->name = $name;"),
        "should assign name: {result}"
    );
    assert!(
        result.contains("$this->age = $age;"),
        "should assign age: {result}"
    );
    assert!(
        result.contains("$this->email = $email;"),
        "should assign email: {result}"
    );
}

// ── Default values ──────────────────────────────────────────────────────────

#[test]
fn includes_default_values_on_parameters() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Config {
    public string $status = 'active';
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("string $status = 'active'"),
        "should carry over default value: {result}"
    );
}

#[test]
fn required_params_before_optional_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Config {
    public string $status = 'active';
    public string $name;
    public int $retries = 3;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // $name (required) should come before $status and $retries (optional).
    let name_pos = result.find("$name").expect("should have $name");
    let status_pos = result.find("$status").expect("should have $status");
    let retries_pos = result.find("$retries").expect("should have $retries");

    // In the parameter list, required params come first.
    // But assignment order follows declaration order.
    // Find the parameter list portion.
    let construct_pos = result.find("__construct(").unwrap();
    let paren_close = result[construct_pos..].find(')').unwrap() + construct_pos;
    let param_list = &result[construct_pos..paren_close];

    let param_name_pos = param_list.find("$name").expect("$name in params");
    let param_status_pos = param_list.find("$status").expect("$status in params");
    let param_retries_pos = param_list.find("$retries").expect("$retries in params");

    assert!(
        param_name_pos < param_status_pos,
        "required $name before optional $status in params: {param_list}"
    );
    assert!(
        param_name_pos < param_retries_pos,
        "required $name before optional $retries in params: {param_list}"
    );

    // Assignments should still reference all properties.
    assert!(
        result.contains("$this->status = $status;"),
        "should assign status: {result}"
    );
    assert!(
        result.contains("$this->name = $name;"),
        "should assign name: {result}"
    );
    assert!(
        result.contains("$this->retries = $retries;"),
        "should assign retries: {result}"
    );

    // Verify the generated result mentions all three in the parameter list.
    let _ = (name_pos, status_pos, retries_pos); // suppress unused warnings
}

// ── Type preservation ───────────────────────────────────────────────────────

#[test]
fn preserves_nullable_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public ?string $label;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("?string $label"),
        "should preserve nullable type: {result}"
    );
}

#[test]
fn preserves_union_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public int|string $id;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("int|string $id"),
        "should preserve union type: {result}"
    );
}

#[test]
fn untyped_property_produces_untyped_param() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public $data;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("($data)"),
        "untyped property should produce untyped param: {result}"
    );
}

// ── Exclusion rules ─────────────────────────────────────────────────────────

#[test]
fn skips_static_properties() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
    public static int $count;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // Check the parameter list specifically, not the whole file
    // (the property declarations still contain "$count").
    let construct_pos = result.find("__construct(").unwrap();
    let paren_close = result[construct_pos..].find(')').unwrap() + construct_pos;
    let param_list = &result[construct_pos..paren_close];

    assert!(
        param_list.contains("string $name"),
        "should include non-static property: {param_list}"
    );
    assert!(
        !param_list.contains("$count"),
        "should exclude static property from params: {param_list}"
    );
}

#[test]
fn includes_readonly_properties() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
    public readonly int $id;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // Readonly properties must be initialized in the constructor,
    // so they should be included.
    let construct_pos = result.find("__construct(").unwrap();
    let paren_close = result[construct_pos..].find(')').unwrap() + construct_pos;
    let param_list = &result[construct_pos..paren_close];

    assert!(
        param_list.contains("string $name"),
        "should include non-readonly property: {param_list}"
    );
    assert!(
        param_list.contains("int $id"),
        "should include readonly property: {param_list}"
    );
    assert!(
        result.contains("$this->id = $id;"),
        "should assign readonly property: {result}"
    );
}

#[test]
fn action_offered_when_all_properties_readonly() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public readonly string $name;
    public readonly int $id;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions);

    assert!(
        action.is_some(),
        "should offer action when all properties are readonly (they must be initialized in the constructor)"
    );

    let result = apply_workspace_edit(content, action.unwrap().edit.as_ref().unwrap());
    assert!(
        result.contains("string $name, int $id"),
        "should include all readonly properties: {result}"
    );
}

#[test]
fn no_action_when_all_properties_static() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public static string $name;
    public static int $count;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions);

    assert!(
        action.is_none(),
        "should not offer action when all properties are static"
    );
}

// ── Constructor already exists ──────────────────────────────────────────────

#[test]
fn no_action_when_constructor_exists() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions);

    assert!(
        action.is_none(),
        "should not offer action when constructor already exists"
    );
}

#[test]
fn no_action_when_constructor_exists_case_insensitive() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;

    public function __CONSTRUCT() {}
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions);

    assert!(
        action.is_none(),
        "should detect constructor case-insensitively"
    );
}

// ── Cursor position ─────────────────────────────────────────────────────────

#[test]
fn action_offered_on_property_line() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
}
";
    // Cursor directly on the property declaration.
    let actions = get_code_actions_at(&backend, uri, content, 2, 4);
    assert!(
        find_generate_action(&actions).is_some(),
        "should offer action when cursor is on property"
    );
}

#[test]
fn no_action_on_static_property() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
    public static int $count;
}
";
    // Cursor on the static property (line 3).
    let actions = get_code_actions_at(&backend, uri, content, 3, 10);
    assert!(
        find_generate_action(&actions).is_none(),
        "should not offer action when cursor is on a static property"
    );
}

#[test]
fn no_action_on_class_brace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
}
";
    // Cursor on the class keyword line — not on a property.
    let actions = get_code_actions_at(&backend, uri, content, 1, 0);
    assert!(
        find_generate_action(&actions).is_none(),
        "should not offer action when cursor is on class declaration"
    );
}

#[test]
fn no_action_inside_method_body() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;

    public function greet(): string {
        return 'hello';
    }
}
";
    // Cursor inside the method body (line 5, on "return").
    let actions = get_code_actions_at(&backend, uri, content, 5, 8);
    assert!(
        find_generate_action(&actions).is_none(),
        "should not offer action when cursor is inside a method body"
    );
}

// ── Abstract classes ────────────────────────────────────────────────────────

#[test]
fn action_offered_for_abstract_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
abstract class Foo {
    public string $name;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions);

    assert!(action.is_some(), "should offer action for abstract class");

    let result = apply_workspace_edit(content, action.unwrap().edit.as_ref().unwrap());
    assert!(
        result.contains("public function __construct(string $name)"),
        "should generate constructor for abstract class: {result}"
    );
}

// ── No action outside class ─────────────────────────────────────────────────

#[test]
fn no_action_outside_class() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
$x = 1;
";
    let actions = get_code_actions_at(&backend, uri, content, 1, 0);
    let action = find_generate_action(&actions);

    assert!(action.is_none(), "should not offer action outside class");
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
    public string $name;
    public string $email;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 10);
    let action = find_generate_action(&actions).expect("should offer action in namespace");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("public function __construct(string $name, string $email)"),
        "should generate constructor in namespaced class: {result}"
    );
}

#[test]
fn works_in_braced_namespace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
namespace App\\Models {

class User {
    public string $name;
}

}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 10);
    let action = find_generate_action(&actions).expect("should offer action in braced namespace");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("public function __construct(string $name)"),
        "should generate constructor in braced namespace: {result}"
    );
}

// ── Docblock type fallback ──────────────────────────────────────────────────

#[test]
fn uses_docblock_type_when_no_native_hint() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    /** @var string */
    public $name;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 3, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("string $name"),
        "should use docblock type as param hint: {result}"
    );
}

#[test]
fn skips_compound_docblock_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    /** @var int|string */
    public $id;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 3, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // Compound docblock types are not usable as native type hints in
    // all PHP versions, so the parameter should be untyped.
    assert!(
        result.contains("($id)"),
        "compound docblock type should produce untyped param: {result}"
    );
}

// ── Mixed qualifying and non-qualifying properties ──────────────────────────

#[test]
fn mixed_properties_only_qualifying_included() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
    public static int $count;
    private float $score;
    public readonly string $id;
    protected array $tags;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // Should include $name, $score, $id (readonly), and $tags but not $count (static).
    let construct_pos = result.find("__construct(").unwrap();
    let paren_close = result[construct_pos..].find(')').unwrap() + construct_pos;
    let param_list = &result[construct_pos..paren_close];

    assert!(
        param_list.contains("$name"),
        "should include $name: {param_list}"
    );
    assert!(
        param_list.contains("$score"),
        "should include $score: {param_list}"
    );
    assert!(
        param_list.contains("$tags"),
        "should include $tags: {param_list}"
    );
    assert!(
        param_list.contains("$id"),
        "should include readonly $id: {param_list}"
    );
    assert!(
        !param_list.contains("$count"),
        "should not include static $count: {param_list}"
    );
}

// ── Insertion point ─────────────────────────────────────────────────────────

#[test]
fn constructor_inserted_after_properties() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
    public int $age;

    public function greet(): string {
        return 'hello';
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // The constructor should appear between the properties and greet().
    let constructor_pos = result.find("__construct").expect("should have constructor");
    let greet_pos = result
        .find("function greet")
        .expect("should still have greet");
    let age_prop_pos = result
        .find("public int $age")
        .expect("should still have age prop");

    assert!(
        constructor_pos > age_prop_pos,
        "constructor should be after properties"
    );
    assert!(
        constructor_pos < greet_pos,
        "constructor should be before existing methods"
    );
}

// ── Indentation ─────────────────────────────────────────────────────────────

#[test]
fn detects_tab_indentation() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "<?php\nclass Foo {\n\tpublic string $name;\n}\n";
    let actions = get_code_actions_at(&backend, uri, content, 2, 5);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("\tpublic function __construct("),
        "should use tab indentation: {result}"
    );
    assert!(
        result.contains("\t\t$this->name = $name;"),
        "body should use double tab: {result}"
    );
}

// ── Array default value ─────────────────────────────────────────────────────

#[test]
fn handles_array_default_value() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public array $items = [];
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_generate_action(&actions).expect("should offer action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("array $items = []"),
        "should carry over array default: {result}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Promoted constructor tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn promoted_action_offered_alongside_traditional() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    assert!(
        find_generate_action(&actions).is_some(),
        "traditional action should be offered"
    );
    assert!(
        find_promoted_action(&actions).is_some(),
        "promoted action should be offered"
    );
}

#[test]
fn promoted_removes_properties_and_creates_promoted_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
    private int $age;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_promoted_action(&actions).expect("should offer promoted action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // Property declarations should be removed.
    assert!(
        !result.contains("public string $name;"),
        "property declaration should be removed: {result}"
    );
    assert!(
        !result.contains("private int $age;"),
        "property declaration should be removed: {result}"
    );

    // Promoted parameters should be present.
    assert!(
        result.contains("public string $name"),
        "should have promoted public param: {result}"
    );
    assert!(
        result.contains("private int $age"),
        "should have promoted private param: {result}"
    );

    // No assignment body.
    assert!(
        !result.contains("$this->"),
        "promoted constructor should not have assignments: {result}"
    );

    // Empty body.
    assert!(result.contains(") {}"), "should have empty body: {result}");
}

#[test]
fn promoted_preserves_readonly() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public readonly string $id;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_promoted_action(&actions).expect("should offer promoted action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("public readonly string $id"),
        "should preserve readonly modifier: {result}"
    );
}

#[test]
fn promoted_carries_default_values() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
    protected string $status = 'active';
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_promoted_action(&actions).expect("should offer promoted action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("protected string $status = 'active'"),
        "should carry over default value: {result}"
    );

    // Required param before optional.
    let name_pos = result.find("$name").unwrap();
    let status_pos = result.find("$status").unwrap();
    assert!(
        name_pos < status_pos,
        "required $name before optional $status: {result}"
    );
}

#[test]
fn promoted_skips_static_properties() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
    public static int $count;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_promoted_action(&actions).expect("should offer promoted action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // Static property should remain (not deleted, not promoted).
    assert!(
        result.contains("public static int $count;"),
        "static property should remain: {result}"
    );
    assert!(
        !result.contains("public static int $count,"),
        "static property should not be in constructor: {result}"
    );
}

#[test]
fn promoted_preserves_visibility_variants() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
    protected int $age;
    private float $score;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_promoted_action(&actions).expect("should offer promoted action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("public string $name"),
        "should preserve public: {result}"
    );
    assert!(
        result.contains("protected int $age"),
        "should preserve protected: {result}"
    );
    assert!(
        result.contains("private float $score"),
        "should preserve private: {result}"
    );
}

#[test]
fn promoted_no_action_when_constructor_exists() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    assert!(
        find_promoted_action(&actions).is_none(),
        "should not offer promoted action when constructor exists"
    );
}

#[test]
fn promoted_trailing_comma_on_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_promoted_action(&actions).expect("should offer promoted action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // Trailing comma for clean diffs.
    assert!(
        result.contains("$name,\n"),
        "should have trailing comma: {result}"
    );
}

#[test]
fn promoted_preserves_nullable_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public ?string $label;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_promoted_action(&actions).expect("should offer promoted action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("public ?string $label"),
        "should preserve nullable type: {result}"
    );
}

#[test]
fn promoted_works_in_namespace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
namespace App\\Models;

class User {
    public string $name;
    private string $email;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 4, 10);
    let action = find_promoted_action(&actions).expect("should offer promoted action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    assert!(
        result.contains("public string $name"),
        "should promote name: {result}"
    );
    assert!(
        result.contains("private string $email"),
        "should promote email: {result}"
    );
    assert!(
        !result.contains("public string $name;"),
        "property declaration should be removed: {result}"
    );
}

#[test]
fn promoted_static_properties_stay_above_constructor() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = "\
<?php
class Foo {
    public string $name;
    public int $age;
    public static int $instanceCount;
    public readonly string $id;
}
";
    let actions = get_code_actions_at(&backend, uri, content, 2, 10);
    let action = find_promoted_action(&actions).expect("should offer promoted action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());

    // Static property should remain and appear before the constructor.
    let static_pos = result
        .find("public static int $instanceCount;")
        .expect("static property should remain: {result}");
    let constructor_pos = result
        .find("public function __construct(")
        .expect("constructor should exist: {result}");
    assert!(
        static_pos < constructor_pos,
        "static property should appear above the constructor:\n{result}"
    );
}
