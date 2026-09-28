//! Integration tests for the "Add @throws" code action.
//!
//! These tests exercise the full pipeline: a PHPStan diagnostic with
//! identifier `missingType.checkedException` triggers a code action
//! that inserts a `@throws` tag into the method docblock and (when
//! needed) adds a `use` import for the exception class.

use crate::common::{
    apply_edits, create_test_backend, extract_edits, find_action, get_code_actions_at,
    inject_phpstan_diag, resolve_action,
};
use tower_lsp::lsp_types::*;

// ── Basic: adds @throws into existing multi-line docblock ───────────────────

#[test]
fn adds_throws_to_existing_docblock() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Controllers;

class FooController {
    /**
     * Do something.
     */
    public function bar(): void {
        throw new \App\Exceptions\BarException();
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        8, // the throw line
        "Method App\\Controllers\\FooController::bar() throws checked exception App\\Exceptions\\BarException but it's missing from the PHPDoc @throws tag.",
        "missingType.checkedException",
    );

    let actions = get_code_actions_at(&backend, uri, content, 8, 10);
    let action = find_action(&actions, "Add @throws").expect("should offer Add @throws action");

    assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    assert_eq!(action.is_preferred, Some(true));
    assert!(
        action.title.contains("BarException"),
        "title should mention exception: {}",
        action.title
    );

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("@throws BarException"),
        "should insert @throws tag:\n{}",
        result
    );
    assert!(
        result.contains("use App\\Exceptions\\BarException;"),
        "should add use import:\n{}",
        result
    );
}

// ── No import needed when exception is in same namespace ────────────────────

#[test]
fn no_import_when_same_namespace() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Exceptions;

class Thrower {
    /**
     * Do something.
     */
    public function go(): void {
        throw new BarException();
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        8,
        "Method App\\Exceptions\\Thrower::go() throws checked exception App\\Exceptions\\BarException but it's missing from the PHPDoc @throws tag.",
        "missingType.checkedException",
    );

    let actions = get_code_actions_at(&backend, uri, content, 8, 10);
    let action = find_action(&actions, "Add @throws").expect("should offer Add @throws action");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("@throws BarException"),
        "should insert @throws tag:\n{}",
        result
    );
    // Should NOT add a use import — same namespace.
    assert!(
        !result.contains("use App\\Exceptions\\BarException"),
        "should NOT add use import for same-namespace class:\n{}",
        result
    );
}

// ── No import when already imported ─────────────────────────────────────────

#[test]
fn no_import_when_already_imported() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Controllers;

use App\Exceptions\BarException;

class FooController {
    /**
     * Do something.
     */
    public function bar(): void {
        throw new BarException();
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        10,
        "Method App\\Controllers\\FooController::bar() throws checked exception App\\Exceptions\\BarException but it's missing from the PHPDoc @throws tag.",
        "missingType.checkedException",
    );

    let actions = get_code_actions_at(&backend, uri, content, 10, 10);
    let action = find_action(&actions, "Add @throws").expect("should offer action");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("@throws BarException"),
        "should insert @throws tag:\n{}",
        result
    );
    // Count occurrences of the use statement — should still be exactly 1.
    let use_count = result.matches("use App\\Exceptions\\BarException;").count();
    assert_eq!(
        use_count, 1,
        "should NOT duplicate existing use import:\n{}",
        result
    );
}

// ── Creates new docblock when none exists ───────────────────────────────────

#[test]
fn creates_docblock_when_none_exists() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Controllers;

class FooController {
    public function bar(): void {
        throw new \App\Exceptions\BarException();
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        5,
        "Method App\\Controllers\\FooController::bar() throws checked exception App\\Exceptions\\BarException but it's missing from the PHPDoc @throws tag.",
        "missingType.checkedException",
    );

    let actions = get_code_actions_at(&backend, uri, content, 5, 10);
    let action = find_action(&actions, "Add @throws").expect("should offer action");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("/**"),
        "should create a docblock:\n{}",
        result
    );
    assert!(
        result.contains("@throws BarException"),
        "should insert @throws tag:\n{}",
        result
    );
    assert!(
        result.contains("use App\\Exceptions\\BarException;"),
        "should add use import:\n{}",
        result
    );
    // The generated docblock must be aligned with the method signature.
    // Each docblock line should start with exactly the same indentation
    // as `public function bar`.
    let expected_fragment =
        "    /**\n     * @throws BarException\n     */\n    public function bar(): void {";
    assert!(
        result.contains(expected_fragment),
        "docblock should be aligned with the method signature:\n{}",
        result
    );
}

// ── Standalone function ─────────────────────────────────────────────────────

#[test]
fn works_with_standalone_function() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
/**
 * Do things.
 */
function doThings(): void {
    throw new \App\Exceptions\ThingException();
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        5,
        "Function doThings() throws checked exception App\\Exceptions\\ThingException but it's missing from the PHPDoc @throws tag.",
        "missingType.checkedException",
    );

    let actions = get_code_actions_at(&backend, uri, content, 5, 10);
    let action = find_action(&actions, "Add @throws").expect("should offer action");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("@throws ThingException"),
        "should insert @throws tag:\n{}",
        result
    );
}

// ── Does not duplicate existing @throws ─────────────────────────────────────

#[test]
fn no_action_when_already_documented() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Controllers;

use App\Exceptions\BarException;

class FooController {
    /**
     * @throws BarException
     */
    public function bar(): void {
        throw new BarException();
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        10,
        "Method App\\Controllers\\FooController::bar() throws checked exception App\\Exceptions\\BarException but it's missing from the PHPDoc @throws tag.",
        "missingType.checkedException",
    );

    let actions = get_code_actions_at(&backend, uri, content, 10, 10);
    let action = find_action(&actions, "Add @throws");
    assert!(
        action.is_none(),
        "should NOT offer action when @throws already documented"
    );
}

// ── Ignores non-matching diagnostics ────────────────────────────────────────

#[test]
fn ignores_other_phpstan_identifiers() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
class Foo {
    /**
     * Summary.
     */
    public function bar(): void {
        $x = 1;
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        6,
        "Some other PHPStan error.",
        "return.unusedType",
    );

    let actions = get_code_actions_at(&backend, uri, content, 6, 10);
    let action = find_action(&actions, "Add @throws");
    assert!(
        action.is_none(),
        "should NOT offer action for non-checkedException identifiers"
    );
}

// ── Single-line docblock ────────────────────────────────────────────────────

#[test]
fn expands_single_line_docblock() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Controllers;

use App\Exceptions\BarException;

class FooController {
    /** Do something. */
    public function bar(): void {
        throw new BarException();
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        8,
        "Method App\\Controllers\\FooController::bar() throws checked exception App\\Exceptions\\BarException but it's missing from the PHPDoc @throws tag.",
        "missingType.checkedException",
    );

    let actions = get_code_actions_at(&backend, uri, content, 8, 10);
    let action = find_action(&actions, "Add @throws").expect("should offer action");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("@throws BarException"),
        "should insert @throws tag:\n{}",
        result
    );
    assert!(
        result.contains("Do something."),
        "should preserve summary:\n{}",
        result
    );
}

// ── Docblock with existing @throws for different exception ──────────────────

#[test]
fn appends_second_throws_tag() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Controllers;

use App\Exceptions\FooException;
use App\Exceptions\BarException;

class FooController {
    /**
     * Do something.
     *
     * @throws FooException
     */
    public function bar(): void {
        throw new FooException();
        throw new BarException();
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        14,
        "Method App\\Controllers\\FooController::bar() throws checked exception App\\Exceptions\\BarException but it's missing from the PHPDoc @throws tag.",
        "missingType.checkedException",
    );

    let actions = get_code_actions_at(&backend, uri, content, 14, 10);
    let action = find_action(&actions, "Add @throws").expect("should offer action");

    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("@throws FooException"),
        "should keep existing @throws:\n{}",
        result
    );
    assert!(
        result.contains("@throws BarException"),
        "should add new @throws:\n{}",
        result
    );
}

// ── Sibling diagnostic clearing ─────────────────────────────────────────────

/// When a method throws the same exception on multiple lines, PHPStan
/// reports a separate `missingType.checkedException` for each `throw`.
/// Adding `@throws` once fixes all of them, so resolving the action on
/// any one diagnostic must clear every sibling diagnostic for the same
/// exception within that method body.
#[test]
fn clears_sibling_checked_exception_diags_in_same_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Helpers;

use RuntimeException;

class BadgeHelper {
    /**
     * Get badge for stock status.
     */
    public function getBadgeForStockStatus(): string {
        if (true) {
            throw new RuntimeException('first');
        }
        throw new RuntimeException('second');
    }
}
"#;
    backend.update_ast(uri, content);

    // Inject two diagnostics for the same exception, different lines.
    let msg = "Method App\\Helpers\\BadgeHelper::getBadgeForStockStatus() throws checked exception RuntimeException but it's missing from the PHPDoc @throws tag.";
    inject_phpstan_diag(&backend, uri, 11, msg, "missingType.checkedException");
    inject_phpstan_diag(&backend, uri, 13, msg, "missingType.checkedException");

    // Trigger the action on the first diagnostic (line 11).
    let actions = get_code_actions_at(&backend, uri, content, 11, 10);
    let action = find_action(&actions, "Add @throws").expect("should offer Add @throws action");

    // Resolve — this should clear BOTH diagnostics from the cache.
    let resolved = resolve_action(&backend, uri, content, action);
    let edits = extract_edits(&resolved);
    let result = apply_edits(content, &edits);

    assert!(
        result.contains("@throws RuntimeException"),
        "should insert @throws tag:\n{}",
        result
    );

    // Both diagnostics must have been removed from the PHPStan cache.
    let remaining: Vec<_> = {
        let cache = backend.phpstan_last_diags().lock();
        cache
            .get(uri)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|d| {
                d.code
                    == Some(NumberOrString::String(
                        "missingType.checkedException".into(),
                    ))
            })
            .collect()
    };
    assert!(
        remaining.is_empty(),
        "both sibling diagnostics should be cleared, but {} remain: {:?}",
        remaining.len(),
        remaining
            .iter()
            .map(|d| d.range.start.line)
            .collect::<Vec<_>>()
    );
}

/// Sibling clearing must NOT clear diagnostics for a *different*
/// exception class, even if they are in the same method.
#[test]
fn does_not_clear_sibling_diags_for_different_exception() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Helpers;

use RuntimeException;
use InvalidArgumentException;

class BadgeHelper {
    /**
     * Get badge.
     */
    public function getBadge(): string {
        if (true) {
            throw new RuntimeException('boom');
        }
        throw new InvalidArgumentException('bad');
    }
}
"#;
    backend.update_ast(uri, content);

    inject_phpstan_diag(
        &backend,
        uri,
        12,
        "Method App\\Helpers\\BadgeHelper::getBadge() throws checked exception RuntimeException but it's missing from the PHPDoc @throws tag.",
        "missingType.checkedException",
    );
    inject_phpstan_diag(
        &backend,
        uri,
        14,
        "Method App\\Helpers\\BadgeHelper::getBadge() throws checked exception InvalidArgumentException but it's missing from the PHPDoc @throws tag.",
        "missingType.checkedException",
    );

    // Resolve only the RuntimeException action.
    let actions = get_code_actions_at(&backend, uri, content, 12, 10);
    let action = find_action(&actions, "Add @throws").expect("should offer Add @throws action");
    let _resolved = resolve_action(&backend, uri, content, action);

    // The InvalidArgumentException diagnostic must still be in the cache.
    let remaining: Vec<_> = {
        let cache = backend.phpstan_last_diags().lock();
        cache
            .get(uri)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|d| {
                d.code
                    == Some(NumberOrString::String(
                        "missingType.checkedException".into(),
                    ))
            })
            .collect()
    };
    assert_eq!(
        remaining.len(),
        1,
        "only the InvalidArgumentException diagnostic should remain"
    );
    assert!(
        remaining[0].message.contains("InvalidArgumentException"),
        "remaining diagnostic should be for InvalidArgumentException"
    );
}

/// Sibling clearing must NOT cross method boundaries: a diagnostic in
/// a different method for the same exception must not be cleared.
#[test]
fn does_not_clear_diags_in_different_method() {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    let content = r#"<?php
namespace App\Helpers;

use RuntimeException;

class BadgeHelper {
    /**
     * First method.
     */
    public function first(): void {
        throw new RuntimeException('a');
    }

    /**
     * Second method.
     */
    public function second(): void {
        throw new RuntimeException('b');
    }
}
"#;
    backend.update_ast(uri, content);

    let msg_first = "Method App\\Helpers\\BadgeHelper::first() throws checked exception RuntimeException but it's missing from the PHPDoc @throws tag.";
    let msg_second = "Method App\\Helpers\\BadgeHelper::second() throws checked exception RuntimeException but it's missing from the PHPDoc @throws tag.";
    inject_phpstan_diag(&backend, uri, 10, msg_first, "missingType.checkedException");
    inject_phpstan_diag(
        &backend,
        uri,
        17,
        msg_second,
        "missingType.checkedException",
    );

    // Resolve the action for first() only.
    let actions = get_code_actions_at(&backend, uri, content, 10, 10);
    let action = find_action(&actions, "Add @throws").expect("should offer Add @throws action");
    let _resolved = resolve_action(&backend, uri, content, action);

    // The diagnostic in second() must still be in the cache.
    let remaining: Vec<_> = {
        let cache = backend.phpstan_last_diags().lock();
        cache
            .get(uri)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|d| {
                d.code
                    == Some(NumberOrString::String(
                        "missingType.checkedException".into(),
                    ))
            })
            .collect()
    };
    assert_eq!(
        remaining.len(),
        1,
        "the diagnostic in second() should remain"
    );
    assert_eq!(
        remaining[0].range.start.line, 17,
        "remaining diagnostic should be on line 17 (second method)"
    );
}
