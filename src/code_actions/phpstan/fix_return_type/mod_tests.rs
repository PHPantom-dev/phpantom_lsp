use super::*;

const VOID_MSG: &str =
    "Function foo() with return type void returns int but should not return anything.";
const EMPTY_MSG: &str = "Function foo() should return int but empty return statement found.";
const TYPE_MSG: &str = "Function foo() should return int but returns string.";
const MISSING_MSG: &str = "Function foo() has no return type specified.";

// ── is_fix_return_type_stale ───────────────────────────────────

#[test]
fn stale_when_return_has_no_expression() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(is_fix_return_type_stale(
        content,
        2,
        "return.void",
        VOID_MSG
    ));
}

#[test]
fn not_stale_when_return_has_expression() {
    let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    assert!(!is_fix_return_type_stale(
        content,
        2,
        "return.void",
        VOID_MSG
    ));
}

#[test]
fn stale_return_empty_when_type_is_void() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(is_fix_return_type_stale(
        content,
        2,
        "return.empty",
        EMPTY_MSG
    ));
}

#[test]
fn not_stale_return_empty_when_type_is_not_void() {
    let content = "<?php\nfunction foo(): int {\n    return;\n}\n";
    assert!(!is_fix_return_type_stale(
        content,
        2,
        "return.empty",
        EMPTY_MSG
    ));
}

#[test]
fn stale_when_line_gone() {
    let content = "<?php\n";
    assert!(is_fix_return_type_stale(
        content,
        5,
        "return.void",
        VOID_MSG
    ));
    assert!(is_fix_return_type_stale(
        content,
        5,
        "return.empty",
        EMPTY_MSG
    ));
}

#[test]
fn not_stale_for_unknown_identifier() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(!is_fix_return_type_stale(content, 2, "other.id", TYPE_MSG));
}

// ── stale detection for new identifiers ─────────────────────────

#[test]
fn return_type_never_stale_via_heuristic() {
    // return.type is only cleared by codeAction/resolve, not by
    // content heuristics, because the right fix might be to change
    // the code rather than the type.
    let content = "<?php\nfunction foo(): int {\n    $x = 1;\n}\n";
    assert!(!is_fix_return_type_stale(
        content,
        2,
        "return.type",
        TYPE_MSG
    ));

    let content2 = "<?php\nfunction foo(): int {\n    return 'hello';\n}\n";
    assert!(!is_fix_return_type_stale(
        content2,
        2,
        "return.type",
        TYPE_MSG
    ));
}

#[test]
fn stale_missing_type_when_type_added() {
    let content = "<?php\nfunction foo(): int {\n    return 1;\n}\n";
    // missingType.return is reported on the function declaration line
    assert!(is_fix_return_type_stale(
        content,
        1,
        "missingType.return",
        MISSING_MSG
    ));
}

#[test]
fn stale_missing_type_multiline_signature() {
    // The diagnostic is on the `function` line (line 1), but the
    // `)` and `: int` are on line 3.
    let content = "<?php\nfunction foo(\n    int $x\n): int {\n    return $x;\n}\n";
    assert!(is_fix_return_type_stale(
        content,
        1,
        "missingType.return",
        MISSING_MSG
    ));
}

#[test]
fn not_stale_missing_type_when_no_type() {
    let content = "<?php\nfunction foo() {\n    return 1;\n}\n";
    assert!(!is_fix_return_type_stale(
        content,
        1,
        "missingType.return",
        MISSING_MSG
    ));
}

// ── Stale detection after strip fix ─────────────────────────────

#[test]
fn stale_after_strip_fix() {
    // Before fix: not stale.
    let before = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    assert!(!is_fix_return_type_stale(
        before,
        2,
        "return.void",
        VOID_MSG
    ));

    // After fix (expression kept, no redundant return;): stale
    // because the line no longer has `return ` (it now has `42;`).
    let after = "<?php\nfunction foo(): void {\n    42;\n}\n";
    assert!(is_fix_return_type_stale(after, 2, "return.void", VOID_MSG));
}

// ── Closures and arrow functions ────────────────────────────────

const ANONYMOUS_VOID_MSG: &str =
    "Anonymous function with return type void returns int but should not return anything.";
const ANONYMOUS_EMPTY_MSG: &str =
    "Anonymous function should return int but empty return statement found.";

#[test]
fn arrow_function_returning_from_a_void_one_is_not_stale() {
    // An arrow function has no `return` keyword on its line.
    let content = "<?php\nfunction run(): void {\n    $f = fn (): void => 42;\n}\n";
    assert!(!is_fix_return_type_stale(
        content,
        2,
        "return.void",
        ANONYMOUS_VOID_MSG
    ));
}

#[test]
fn return_empty_reads_the_closure_type_not_the_method_type() {
    let closure_typed_void = "<?php\nfunction run(): int {\n    $f = function (): void {\n        return;\n    };\n    return 1;\n}\n";
    assert!(is_fix_return_type_stale(
        closure_typed_void,
        3,
        "return.empty",
        ANONYMOUS_EMPTY_MSG
    ));

    let method_typed_void =
        "<?php\nfunction run(): void {\n    $f = function (): int {\n        return;\n    };\n}\n";
    assert!(!is_fix_return_type_stale(
        method_typed_void,
        3,
        "return.empty",
        ANONYMOUS_EMPTY_MSG
    ));
}
