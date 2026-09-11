//! Integration tests for the "Update docblock to match signature" code action.
//!
//! These tests exercise the full pipeline: parsing PHP source, finding the
//! function/method under the cursor, detecting docblock/signature mismatches,
//! and generating the `WorkspaceEdit` that patches the docblock.

use crate::common::{create_test_backend, extract_edit_text, get_code_actions_at};
use tower_lsp::lsp_types::*;

/// Check whether the docblock text contains a `@param` tag with the given
/// type and name, allowing arbitrary whitespace between them (for column
/// alignment).  An optional description can also be checked.
fn contains_param(text: &str, type_str: &str, name: &str) -> bool {
    for line in text.lines() {
        let trimmed = line.trim().trim_start_matches('*').trim();
        if let Some(rest) = trimmed.strip_prefix("@param") {
            let rest = rest.trim_start();
            if let Some(after_type) = rest.strip_prefix(type_str) {
                let after_type = after_type.trim_start();
                if after_type == name
                    || after_type.starts_with(&format!("{} ", name))
                    || after_type.starts_with(&format!("{}\n", name))
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Find the "Update docblock" code action from a list of actions.
fn find_update_docblock_action(actions: &[CodeActionOrCommand]) -> Option<&CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title.contains("Update docblock") => Some(ca),
        _ => None,
    })
}

// ── Missing parameter ───────────────────────────────────────────────────────

#[test]
fn adds_missing_param_tag() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Does something.
     *
     * @param string $a The first param
     */
    public function bar(string $a, int $b): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("$a The first param"),
        "should preserve existing param description: {}",
        new_text
    );
    assert!(
        contains_param(&new_text, "int", "$b"),
        "should add missing param: {}",
        new_text
    );
}

// ── Extra parameter ─────────────────────────────────────────────────────────

#[test]
fn removes_extra_param_tag() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param string $a
     * @param int $b
     */
    public function bar(string $a): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("@param string $a"),
        "should keep existing param: {}",
        new_text
    );
    assert!(
        !new_text.contains("$b"),
        "should remove extra param: {}",
        new_text
    );
}

// ── Reordered parameters ────────────────────────────────────────────────────

#[test]
fn reorders_param_tags_to_match_signature() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param int $b Second
     * @param string $a First
     */
    public function bar(string $a, int $b): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    let a_pos = new_text.find("$a").expect("should contain $a");
    let b_pos = new_text.find("$b").expect("should contain $b");
    assert!(
        a_pos < b_pos,
        "$a should come before $b in the updated docblock: {}",
        new_text
    );
    // Descriptions should be preserved.
    assert!(
        new_text.contains("First"),
        "should preserve $a description: {}",
        new_text
    );
    assert!(
        new_text.contains("Second"),
        "should preserve $b description: {}",
        new_text
    );
}

// ── No update needed ────────────────────────────────────────────────────────

#[test]
fn no_action_when_params_already_match() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param string $a
     * @param int $b
     */
    public function bar(string $a, int $b): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action = find_update_docblock_action(&actions);
    assert!(
        action.is_none(),
        "should not offer action when params match"
    );
}

// ── Type contradiction in @param ────────────────────────────────────────────

#[test]
fn updates_contradicted_param_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param string $a The name
     */
    public function bar(int $a): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("@param int $a"),
        "should update type to match signature: {}",
        new_text
    );
    assert!(
        new_text.contains("The name"),
        "should preserve description: {}",
        new_text
    );
}

// ── Refinement types are preserved ──────────────────────────────────────────

#[test]
fn preserves_refinement_type_in_param() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param non-empty-string $a
     */
    public function bar(string $a): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 4, 20);
    let action = find_update_docblock_action(&actions);
    assert!(
        action.is_none(),
        "should not offer action when docblock type is a refinement"
    );
}

#[test]
fn preserves_generic_refinement_in_param() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param array<int, string> $items
     */
    public function bar(array $items): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 4, 20);
    let action = find_update_docblock_action(&actions);
    assert!(
        action.is_none(),
        "should not offer action when docblock type is a generic refinement"
    );
}

// ── @return tag handling ────────────────────────────────────────────────────

#[test]
fn removes_redundant_void_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Does something.
     *
     * @return void
     */
    public function bar(): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        !new_text.contains("@return"),
        "should remove @return void: {}",
        new_text
    );
    assert!(
        new_text.contains("Does something"),
        "should preserve summary: {}",
        new_text
    );
}

#[test]
fn updates_contradicted_return_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Gets the value.
     *
     * @return string The value
     */
    public function getValue(): int {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("@return int"),
        "should update return type: {}",
        new_text
    );
    assert!(
        new_text.contains("The value"),
        "should preserve return description: {}",
        new_text
    );
}

// ── No docblock ─────────────────────────────────────────────────────────────

#[test]
fn no_action_without_docblock() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    public function bar(string $a): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 20);
    let action = find_update_docblock_action(&actions);
    assert!(
        action.is_none(),
        "should not offer action without an existing docblock"
    );
}

// ── Standalone function ─────────────────────────────────────────────────────

#[test]
fn works_with_standalone_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * @param string $a
 * @param int $b
 */
function bar(string $a, int $b, bool $c): void {}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 2, 5);
    let action = find_update_docblock_action(&actions)
        .expect("should offer Update docblock action for standalone function");

    let new_text = extract_edit_text(action);
    assert!(
        contains_param(&new_text, "bool", "$c"),
        "should add missing param: {}",
        new_text
    );
}

// ── Preserves other tags ────────────────────────────────────────────────────

#[test]
fn preserves_template_and_throws_tags() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Summary.
     *
     * @template T
     * @param string $a
     * @throws \RuntimeException
     */
    public function bar(string $a, int $b): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 6, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("@template T"),
        "should preserve @template tag: {}",
        new_text
    );
    assert!(
        new_text.contains("@throws"),
        "should preserve @throws tag: {}",
        new_text
    );
    assert!(
        contains_param(&new_text, "int", "$b"),
        "should add missing param: {}",
        new_text
    );
    assert!(
        new_text.contains("Summary"),
        "should preserve summary: {}",
        new_text
    );
}

#[test]
fn preserves_deprecated_tag() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @deprecated Use newBar() instead.
     * @param string $a
     */
    public function bar(int $a): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 4, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("@deprecated"),
        "should preserve @deprecated tag: {}",
        new_text
    );
    assert!(
        new_text.contains("@param int $a"),
        "should update param type: {}",
        new_text
    );
}

// ── Variadic parameter ──────────────────────────────────────────────────────

#[test]
fn handles_variadic_param_match() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param string ...$args
     */
    public function bar(string ...$args): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action = find_update_docblock_action(&actions);
    assert!(
        action.is_none(),
        "should not offer action when variadic params match"
    );
}

#[test]
fn adds_variadic_prefix_for_new_param() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Summary.
     *
     * @param string $a The first param
     */
    public function bar(string $a, int ...$rest): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        contains_param(&new_text, "string", "$a"),
        "should keep $a param: {}",
        new_text
    );
    assert!(
        contains_param(&new_text, "int", "...$rest"),
        "should add variadic $rest param: {}",
        new_text
    );
}

// ── Namespace handling ──────────────────────────────────────────────────────

#[test]
fn works_inside_namespace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Services;

class UserService {
    /**
     * @param string $name
     */
    public function create(string $name, string $email): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 5);
    let action = find_update_docblock_action(&actions)
        .expect("should offer Update docblock action inside namespace");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("@param string $email"),
        "should add missing $email param: {}",
        new_text
    );
}

// ── Combined param and return update ────────────────────────────────────────

#[test]
fn updates_both_params_and_return() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Transforms data.
     *
     * @param string $input
     * @return string
     */
    public function transform(int $input, bool $strict): array {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 5, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        contains_param(&new_text, "int", "$input"),
        "should update param type: {}",
        new_text
    );
    assert!(
        contains_param(&new_text, "bool", "$strict"),
        "should add new param: {}",
        new_text
    );
    assert!(
        new_text.contains("@return array"),
        "should update return type: {}",
        new_text
    );
}

// ── Summary preservation ────────────────────────────────────────────────────

#[test]
fn preserves_multiline_summary() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * This is a summary that describes
     * what the method does.
     *
     * @param string $old
     */
    public function bar(int $new): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 6, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("This is a summary"),
        "should preserve first summary line: {}",
        new_text
    );
    assert!(
        new_text.contains("what the method does"),
        "should preserve second summary line: {}",
        new_text
    );
    assert!(
        !new_text.contains("$old"),
        "should remove old param: {}",
        new_text
    );
    assert!(
        new_text.contains("@param int $new"),
        "should add new param: {}",
        new_text
    );
}

// ── Untyped parameters ──────────────────────────────────────────────────────

#[test]
fn uses_mixed_for_untyped_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Summary.
     */
    public function bar($a, $b): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("@param mixed $a"),
        "should use mixed for untyped $a: {}",
        new_text
    );
    assert!(
        new_text.contains("@param mixed $b"),
        "should use mixed for untyped $b: {}",
        new_text
    );
}

// ── Interface method ────────────────────────────────────────────────────────

#[test]
fn works_on_interface_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
interface Transformer {
    /**
     * @param string $input
     */
    public function transform(string $input, array $options): mixed;
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action = find_update_docblock_action(&actions)
        .expect("should offer Update docblock action on interface method");

    let new_text = extract_edit_text(action);
    assert!(
        contains_param(&new_text, "array<mixed>", "$options"),
        "should add missing param: {}",
        new_text
    );
}

// ── Trait method ────────────────────────────────────────────────────────────

#[test]
fn works_on_trait_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
trait HasName {
    /**
     * @param int $name
     */
    public function setName(string $name): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action = find_update_docblock_action(&actions)
        .expect("should offer Update docblock action on trait method");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("@param string $name"),
        "should update type from int to string: {}",
        new_text
    );
}

// ── Nullable types ──────────────────────────────────────────────────────────

#[test]
fn no_action_when_nullable_syntax_differs_but_semantically_equal() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param string|null $a
     */
    public function bar(?string $a): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action = find_update_docblock_action(&actions);
    // string|null and ?string are semantically equivalent — no update needed.
    assert!(
        action.is_none(),
        "should not offer action when nullable syntax differs but is semantically equivalent"
    );
}

// ── Empty docblock with just summary ────────────────────────────────────────

#[test]
fn no_action_for_summary_only_docblock_with_typed_params() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Process the input.
     */
    public function process(string $input): string {}
}
"#;
    backend.update_ast(uri, content);

    // The docblock has zero @param tags and the native type is sufficient,
    // so no update should be offered (matches generate-docblock behaviour).
    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action = find_update_docblock_action(&actions);
    assert!(
        action.is_none(),
        "should NOT offer Update docblock for summary-only docblock with fully typed params"
    );
}

#[test]
fn adds_params_to_summary_only_docblock_with_untyped_param() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Process the input.
     */
    public function process($input): string {}
}
"#;
    backend.update_ast(uri, content);

    // The param has no native type, so enrichment produces `mixed` and
    // the update should be offered.
    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock action");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("Process the input"),
        "should preserve summary: {}",
        new_text
    );
    assert!(
        new_text.contains("@param mixed $input"),
        "should add param with mixed type: {}",
        new_text
    );
}

// ── Cursor inside docblock ──────────────────────────────────────────────────

#[test]
fn action_offered_when_cursor_inside_docblock() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {}
}
"#;
    backend.update_ast(uri, content);

    // Cursor on the @param line inside the docblock (line 3).
    let actions = get_code_actions_at(&backend, uri, content, 3, 10);
    let action = find_update_docblock_action(&actions)
        .expect("should offer Update docblock action when cursor is inside docblock");

    let new_text = extract_edit_text(action);
    assert!(
        contains_param(&new_text, "int", "$b"),
        "should add missing param: {}",
        new_text
    );
}

#[test]
fn action_offered_when_cursor_on_opening_comment() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {}
}
"#;
    backend.update_ast(uri, content);

    // Cursor on the /** line (line 2).
    let actions = get_code_actions_at(&backend, uri, content, 2, 6);
    let action = find_update_docblock_action(&actions)
        .expect("should offer Update docblock action when cursor is on /**");

    let new_text = extract_edit_text(action);
    assert!(
        contains_param(&new_text, "int", "$b"),
        "should add missing param: {}",
        new_text
    );
}

// ── @param with no type ─────────────────────────────────────────────────────

#[test]
fn no_duplicate_when_param_has_no_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param $name The user name
     */
    public function bar($name): void {}
}
"#;
    backend.update_ast(uri, content);

    // The existing @param $name (no type) should be recognised as covering
    // the $name parameter.  The action will be offered to add `mixed` as
    // the explicit type, but the result must not contain a duplicate $name.
    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action = find_update_docblock_action(&actions);

    if let Some(action) = action {
        let new_text = extract_edit_text(action);
        let param_name_count = new_text.matches("$name").count();
        assert_eq!(
            param_name_count, 1,
            "should not duplicate $name param, got:\n{}",
            new_text
        );
        assert!(
            new_text.contains("The user name"),
            "should preserve description, got:\n{}",
            new_text
        );
    }
}

#[test]
fn no_duplicate_when_param_has_no_type_with_native_hint() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param $name The user name
     * @param int $age The age
     */
    public function bar(string $name, int $age): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action = find_update_docblock_action(&actions);

    // The action may or may not be offered (depends on whether the missing
    // type is considered a contradiction), but if it IS offered, the result
    // must not contain a duplicate $name param.
    if let Some(action) = action {
        let new_text = extract_edit_text(action);
        let param_name_count = new_text.matches("$name").count();
        assert_eq!(
            param_name_count, 1,
            "should not duplicate $name param, got:\n{}",
            new_text
        );
    }
}

#[test]
fn preserves_description_for_param_with_no_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param $a First param
     */
    public function bar(string $a, int $b): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action =
        find_update_docblock_action(&actions).expect("should offer Update docblock for missing $b");

    let new_text = extract_edit_text(action);
    assert!(
        new_text.contains("First param"),
        "should preserve description from @param $a: {}",
        new_text
    );
    assert!(
        contains_param(&new_text, "int", "$b"),
        "should add missing $b param: {}",
        new_text
    );
}

#[test]
fn no_duplicate_for_variadic_param_with_no_type() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * @param ...$args The arguments
     */
    public function bar(...$args): void {}
}
"#;
    backend.update_ast(uri, content);

    let actions = get_code_actions_at(&backend, uri, content, 3, 5);
    let action = find_update_docblock_action(&actions);

    // The action may be offered to add `mixed` as the explicit type, but
    // the result must not contain a duplicate ...$args.
    if let Some(action) = action {
        let new_text = extract_edit_text(action);
        let param_args_count = new_text.matches("$args").count();
        assert_eq!(
            param_args_count, 1,
            "should not duplicate $args param, got:\n{}",
            new_text
        );
        assert!(
            new_text.contains("The arguments"),
            "should preserve description, got:\n{}",
            new_text
        );
    }
}
