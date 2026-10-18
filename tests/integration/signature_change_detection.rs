//! `update_ast` reports whether a re-parse changed a signature other files
//! depend on, which is what triggers cross-file diagnostic invalidation.
//! A first parse and a body-only edit change nothing; a function or class
//! signature edit, and a function added or removed, do.

use crate::common::create_test_backend;

/// Changing a function's parameter type should cause `update_ast` to
/// return `true` (signature changed), triggering cross-file
/// diagnostic invalidation.  This is the exact scenario from
/// GitHub issue #123.
#[test]
fn update_ast_detects_function_param_type_change() {
    let backend = create_test_backend();
    let uri = "file:///test2.php";

    let v1 = "<?php\nfunction bar(null $x) {\n    return $x;\n}\n";
    let changed = backend.update_ast(uri, v1);
    // First parse — no old functions to compare against.
    assert!(!changed, "First parse should not report a change");

    let v2 = "<?php\nfunction bar(string $x) {\n    return $x;\n}\n";
    let changed = backend.update_ast(uri, v2);
    assert!(
        changed,
        "Changing parameter type null→string must be detected"
    );
}

/// Changing a function's return type should be detected.
#[test]
fn update_ast_detects_function_return_type_change() {
    let backend = create_test_backend();
    let uri = "file:///helpers.php";

    let v1 = "<?php\nfunction helper(): int {\n    return 42;\n}\n";
    backend.update_ast(uri, v1);

    let v2 = "<?php\nfunction helper(): string {\n    return 'hello';\n}\n";
    let changed = backend.update_ast(uri, v2);
    assert!(changed, "Changing return type int→string must be detected");
}

/// Changing only the function body (not the signature) should NOT
/// trigger cross-file invalidation.
#[test]
fn update_ast_ignores_function_body_change() {
    let backend = create_test_backend();
    let uri = "file:///helpers.php";

    let v1 = "<?php\nfunction helper(int $x): int {\n    return $x + 1;\n}\n";
    backend.update_ast(uri, v1);

    let v2 = "<?php\nfunction helper(int $x): int {\n    return $x + 2;\n}\n";
    let changed = backend.update_ast(uri, v2);
    assert!(
        !changed,
        "Body-only change should not report a signature change"
    );
}

/// Adding a new function should be detected as a change.
#[test]
fn update_ast_detects_new_function() {
    let backend = create_test_backend();
    let uri = "file:///helpers.php";

    let v1 = "<?php\nfunction foo(): void {}\n";
    backend.update_ast(uri, v1);

    let v2 = "<?php\nfunction foo(): void {}\nfunction bar(): void {}\n";
    let changed = backend.update_ast(uri, v2);
    assert!(changed, "Adding a new function must be detected");
}

/// Removing a function should be detected as a change.
#[test]
fn update_ast_detects_removed_function() {
    let backend = create_test_backend();
    let uri = "file:///helpers.php";

    let v1 = "<?php\nfunction foo(): void {}\nfunction bar(): void {}\n";
    backend.update_ast(uri, v1);

    let v2 = "<?php\nfunction foo(): void {}\n";
    let changed = backend.update_ast(uri, v2);
    assert!(changed, "Removing a function must be detected");
}

/// Adding a parameter to a function should be detected.
#[test]
fn update_ast_detects_added_parameter() {
    let backend = create_test_backend();
    let uri = "file:///helpers.php";

    let v1 = "<?php\nfunction greet(string $name): string {\n    return $name;\n}\n";
    backend.update_ast(uri, v1);

    let v2 = "<?php\nfunction greet(string $name, string $greeting = 'Hello'): string {\n    return \"$greeting $name\";\n}\n";
    let changed = backend.update_ast(uri, v2);
    assert!(changed, "Adding a parameter must be detected");
}

/// Class signature changes should still be detected (regression guard).
#[test]
fn update_ast_still_detects_class_signature_change() {
    let backend = create_test_backend();
    let uri = "file:///MyClass.php";

    let v1 = "<?php\nclass MyClass {\n    public function foo(): int { return 1; }\n}\n";
    backend.update_ast(uri, v1);

    let v2 = "<?php\nclass MyClass {\n    public function foo(): string { return 'a'; }\n}\n";
    let changed = backend.update_ast(uri, v2);
    assert!(
        changed,
        "Class method return type change must still be detected"
    );
}
