use super::*;

// ── is_fix_return_type_stale ───────────────────────────────────

#[test]
fn stale_when_return_has_no_expression() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(is_fix_return_type_stale(content, 2, "return.void"));
}

#[test]
fn not_stale_when_return_has_expression() {
    let content = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    assert!(!is_fix_return_type_stale(content, 2, "return.void"));
}

#[test]
fn stale_return_empty_when_type_is_void() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(is_fix_return_type_stale(content, 2, "return.empty"));
}

#[test]
fn not_stale_return_empty_when_type_is_not_void() {
    let content = "<?php\nfunction foo(): int {\n    return;\n}\n";
    assert!(!is_fix_return_type_stale(content, 2, "return.empty"));
}

#[test]
fn stale_when_line_gone() {
    let content = "<?php\n";
    assert!(is_fix_return_type_stale(content, 5, "return.void"));
    assert!(is_fix_return_type_stale(content, 5, "return.empty"));
}

#[test]
fn not_stale_for_unknown_identifier() {
    let content = "<?php\nfunction foo(): void {\n    return;\n}\n";
    assert!(!is_fix_return_type_stale(content, 2, "other.id"));
}

// ── stale detection for new identifiers ─────────────────────────

#[test]
fn return_type_never_stale_via_heuristic() {
    // return.type is only cleared by codeAction/resolve, not by
    // content heuristics, because the right fix might be to change
    // the code rather than the type.
    let content = "<?php\nfunction foo(): int {\n    $x = 1;\n}\n";
    assert!(!is_fix_return_type_stale(content, 2, "return.type"));

    let content2 = "<?php\nfunction foo(): int {\n    return 'hello';\n}\n";
    assert!(!is_fix_return_type_stale(content2, 2, "return.type"));
}

#[test]
fn stale_missing_type_when_type_added() {
    let content = "<?php\nfunction foo(): int {\n    return 1;\n}\n";
    // missingType.return is reported on the function declaration line
    assert!(is_fix_return_type_stale(content, 1, "missingType.return"));
}

#[test]
fn stale_missing_type_multiline_signature() {
    let content = "<?php\nfunction foo(\n    int $x\n): int {\n    return $x;\n}\n";
    // The diagnostic is on the `function` line (line 1), but the
    // `)` and `: int` are on line 3.  PHPStan reports on the
    // function keyword line.  Our simple check looks at the diag
    // line for `)...:`  which won't find it on line 1.  That's
    // acceptable — the diagnostic will be cleared by the next
    // PHPStan run instead of eagerly.
    assert!(!is_fix_return_type_stale(content, 1, "missingType.return"));
}

#[test]
fn not_stale_missing_type_when_no_type() {
    let content = "<?php\nfunction foo() {\n    return 1;\n}\n";
    assert!(!is_fix_return_type_stale(content, 1, "missingType.return"));
}

// ── Stale detection after strip fix ─────────────────────────────

#[test]
fn stale_after_strip_fix() {
    // Before fix: not stale.
    let before = "<?php\nfunction foo(): void {\n    return 42;\n}\n";
    assert!(!is_fix_return_type_stale(before, 2, "return.void"));

    // After fix (expression kept, no redundant return;): stale
    // because the line no longer has `return ` (it now has `42;`).
    let after = "<?php\nfunction foo(): void {\n    42;\n}\n";
    assert!(is_fix_return_type_stale(after, 2, "return.void"));
}
