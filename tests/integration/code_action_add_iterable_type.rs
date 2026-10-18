//! Integration tests for the "Add @return type" code action on PHPStan's
//! `missingType.iterableValue` diagnostic.
//!
//! Each test injects the diagnostic, resolves the action, applies the
//! edit, and checks where the `@return` tag ends up for each shape of
//! docblock the method can already have.

use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action, get_code_actions_on_line,
    inject_phpstan_diag, resolve_action,
};

const MESSAGE: &str =
    "Method Foo::ids() return type has no value type specified in iterable type array.";

/// Resolve the action for a method whose signature is on `line` and
/// return the edited source.
fn add_iterable_type(content: &str, line: u32) -> String {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    inject_phpstan_diag(&backend, uri, line, MESSAGE, "missingType.iterableValue");

    let actions = get_code_actions_on_line(&backend, uri, content, line);
    let action = find_action(&actions, "Add @return type").expect("should offer the action");
    let resolved = resolve_action(&backend, uri, content, action);
    apply_edits(content, &extract_edits(&resolved))
}

#[test]
fn creates_a_docblock_when_there_is_none() {
    let content = "<?php
class Foo {
    public function ids(): array {
        return [1, 2];
    }
}
";
    assert_eq!(
        add_iterable_type(content, 2),
        "<?php
class Foo {
    /**
     * @return array<1|2>
     */
    public function ids(): array {
        return [1, 2];
    }
}
"
    );
}

#[test]
fn adds_the_tag_to_an_existing_docblock() {
    let content = "<?php
class Foo {
    /**
     * The ids.
     *
     * @param int $limit
     */
    public function ids(int $limit): array {
        return [1, 2];
    }
}
";
    assert_eq!(
        add_iterable_type(content, 7),
        "<?php
class Foo {
    /**
     * The ids.
     *
     * @param int $limit
     *
     * @return array<1|2>
     */
    public function ids(int $limit): array {
        return [1, 2];
    }
}
"
    );
}

#[test]
fn expands_a_single_line_docblock() {
    let content = "<?php
class Foo {
    /** The ids. */
    public function ids(): array {
        return [1, 2];
    }
}
";
    assert_eq!(
        add_iterable_type(content, 3),
        "<?php
class Foo {
    /**
     * The ids.
     *
     * @return array<1|2>
     */
    public function ids(): array {
        return [1, 2];
    }
}
"
    );
}

#[test]
fn rewrites_the_type_of_an_existing_tag_and_keeps_its_description() {
    let content = "<?php
class Foo {
    /**
     * @return array The ids.
     */
    public function ids(): array {
        return [1, 2];
    }
}
";
    assert_eq!(
        add_iterable_type(content, 5),
        "<?php
class Foo {
    /**
     * @return array<1|2> The ids.
     */
    public function ids(): array {
        return [1, 2];
    }
}
"
    );
}

#[test]
fn a_keyed_array_literal_names_its_key_and_value_types() {
    let content = "<?php
class Foo {
    public function ids(): array {
        return ['a' => 1, 'b' => 2];
    }
}
";
    let result = add_iterable_type(content, 2);
    assert!(
        result.contains("@return array<string, 1|2>"),
        "should read the key and value types off the literal, got:\n{result}"
    );
}
