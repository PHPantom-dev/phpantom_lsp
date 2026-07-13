//! Type-guard narrowing on compound conditions and non-variable
//! subjects.
//!
//! `instanceof` / `assert*` narrowing must survive beyond the simplest
//! single-negated-variable guard: inline `&&` chains, `||` guards whose
//! De Morgan expansion narrows several distinct subjects, array-indexed
//! subjects, inline assignments in the condition, and `@phpstan-assert`
//! on property/array subjects.

use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

/// Run slow diagnostics (activates the forward-walker scope cache) and
/// keep only `unknown_member` diagnostics.
fn unknown_member_diagnostics(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    text: &str,
) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut out);
    out.retain(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
    });
    out
}

/// Shared scaffolding: a wide `Expr` interface, a `StringExpr` subtype
/// with a `value` property, an unrelated subtype, and holders.
const SCAFFOLD: &str = r#"<?php
namespace Repro;

interface Expr {}
class StringExpr implements Expr {
    public string $value = '';
}
class OtherExpr implements Expr {}
class Arg {
    public Expr $value;
}
class Holder {
    public function getReturnType(): ?Expr { return null; }
}
function takeString(string $s): void {}
/** @phpstan-assert StringExpr $value */
function assertStringExpr(Expr $value): void {}
"#;

/// `&&` chain: a later conjunct uses the narrowing from an earlier one.
#[test]
fn and_chain_uses_earlier_conjunct_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///and_chain.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Arg $arg): void {{
        if ($arg->value instanceof StringExpr && $arg->value->value === 'x') {{
            takeString($arg->value->value);
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "Narrowing from the first `&&` conjunct should apply to the \
         later conjunct and body, got: {diags:?}"
    );
}

/// `||` guard clause: De Morgan narrows both distinct subjects.
#[test]
fn or_guard_narrows_multiple_subjects() {
    let backend = create_test_backend();
    let uri = "file:///or_guard.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Arg $arg): void {{
        if (! $arg instanceof Arg || ! $arg->value instanceof StringExpr) {{
            return;
        }}
        takeString($arg->value->value);
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "After the `||` guard, both `$arg` and `$arg->value` should be \
         narrowed, got: {diags:?}"
    );
}

/// Integer-indexed subject in a guard clause.
#[test]
fn integer_index_guard_narrows_element() {
    let backend = create_test_backend();
    let uri = "file:///int_index.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    /** @param Expr[] $stmts */
    public function m(array $stmts): void {{
        if (! $stmts[0] instanceof StringExpr) {{
            return;
        }}
        takeString($stmts[0]->value);
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "The integer-indexed element `$stmts[0]` should narrow to \
         StringExpr after the guard, got: {diags:?}"
    );
}

/// Array index then property (`$args[0]->value`) in a guard clause.
#[test]
fn integer_index_then_property_guard() {
    let backend = create_test_backend();
    let uri = "file:///int_index_prop.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    /** @param Arg[] $args */
    public function m(array $args): void {{
        if (! $args[0]->value instanceof StringExpr) {{
            return;
        }}
        takeString($args[0]->value->value);
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "`$args[0]->value` should narrow to StringExpr after the guard, \
         got: {diags:?}"
    );
}

/// String-indexed subject in a guard clause.
#[test]
fn string_index_guard_narrows_element() {
    let backend = create_test_backend();
    let uri = "file:///str_index.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    /** @param array<string, Expr> $constants */
    public function m(array $constants): void {{
        if (! $constants['C'] instanceof StringExpr) {{
            return;
        }}
        takeString($constants['C']->value);
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "`$constants['C']` should narrow to StringExpr after the guard, \
         got: {diags:?}"
    );
}

/// Inline assignment in the condition: `if (($x = expr()) instanceof Foo)`.
#[test]
fn inline_assignment_in_condition_narrows() {
    let backend = create_test_backend();
    let uri = "file:///inline_assign.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Holder $h): void {{
        if (($node = $h->getReturnType()) instanceof StringExpr) {{
            takeString($node->value);
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "The inline-assigned `$node` should narrow to StringExpr inside \
         the branch, got: {diags:?}"
    );
}

/// `@phpstan-assert` on a property subject narrows subsequent accesses.
#[test]
fn phpstan_assert_on_property_subject() {
    let backend = create_test_backend();
    let uri = "file:///assert_prop.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Arg $arg): void {{
        assertStringExpr($arg->value);
        takeString($arg->value->value);
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "`@phpstan-assert StringExpr` on `$arg->value` should narrow the \
         property for later accesses, got: {diags:?}"
    );
}

/// Negative control: narrowing must not leak. Inside the `instanceof
/// OtherExpr` branch, `$arg->value` is `OtherExpr` (no `value` property),
/// so accessing `->value` must still be flagged.
#[test]
fn narrowing_does_not_over_apply() {
    let backend = create_test_backend();
    let uri = "file:///negative.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Arg $arg): void {{
        if ($arg->value instanceof OtherExpr) {{
            takeString($arg->value->value);
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("value") && d.message.contains("OtherExpr")),
        "Accessing `->value` on the narrowed `OtherExpr` should be \
         flagged, got: {diags:?}"
    );
}
