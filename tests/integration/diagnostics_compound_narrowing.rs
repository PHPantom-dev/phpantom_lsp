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
class OtherExpr implements Expr {
    /** @var list<mixed> */
    public array $items = [];
}
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

/// A guard on a no-argument method call narrows the repeated call.
#[test]
fn repeated_method_call_is_narrowed() {
    let backend = create_test_backend();
    let uri = "file:///repeated_call.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Holder $h): void {{
        if ($h->getReturnType() instanceof StringExpr) {{
            takeString($h->getReturnType()->value);
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "The repeated `$h->getReturnType()` call should carry the \
         `StringExpr` narrowing from the guard, got: {diags:?}"
    );
}

/// A guard against a supertype must not throw away what the return type
/// already knows: `getStringExpr(): StringExpr` checked against `Expr`
/// stays `StringExpr`, so `->value` is still there.
#[test]
fn call_guard_against_supertype_keeps_declared_type() {
    let backend = create_test_backend();
    let uri = "file:///call_supertype.php";
    let text = format!(
        "{SCAFFOLD}
class Wide {{
    public function getStringExpr(): StringExpr {{ return new StringExpr(); }}
}}
class C {{
    public function m(Wide $w): void {{
        if ($w->getStringExpr() instanceof Expr) {{
            takeString($w->getStringExpr()->value);
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "Checking a `StringExpr` return against the wider `Expr` should \
         leave the declared `StringExpr` in place, got: {diags:?}"
    );
}

/// Negative control: the guard proves nothing about a *different* call
/// on the same object.
#[test]
fn call_narrowing_does_not_leak_to_other_calls() {
    let backend = create_test_backend();
    let uri = "file:///call_other.php";
    let text = format!(
        "{SCAFFOLD}
class Two {{
    public function first(): ?Expr {{ return null; }}
    public function second(): ?Expr {{ return null; }}
}}
class C {{
    public function m(Two $t): void {{
        if ($t->first() instanceof StringExpr) {{
            takeString($t->second()->value);
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        !diags.is_empty(),
        "`first()` being a `StringExpr` says nothing about `second()`, \
         so `->value` must still be flagged, got: {diags:?}"
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

/// `if`/`elseif` chain where each branch narrows the same property-path
/// subject to a different type via a compound `&&` condition.  The
/// `elseif` branch's own `instanceof` must win: the preceding `if`
/// branch's narrowing must not leak into it.  Regression test: the chain
/// resolution cache keyed `$args[0]->value` by subject text plus the base
/// variable's type (identical at both branches), so the first branch's
/// `StringExpr` narrowing was reused inside the `elseif`, flagging
/// `->items` as missing on `StringExpr`.
#[test]
fn elseif_property_path_uses_own_branch_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///elseif_prop_path.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    /** @param Arg[] $args */
    public function m(array $args): void {{
        if (count($args) === 2 && $args[0]->value instanceof StringExpr) {{
            takeString($args[0]->value->value);
        }} elseif (count($args) === 1 && $args[0]->value instanceof OtherExpr) {{
            foreach ($args[0]->value->items as $item) {{
                unset($item);
            }}
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "Inside the `elseif`, `$args[0]->value` should narrow to \
         `OtherExpr` (which has `items`), not carry the `if` branch's \
         `StringExpr` narrowing, got: {diags:?}"
    );
}

/// A check on a call subject holds inside its own branch only. The
/// narrowing must not be carried over to a later, unrelated check on
/// the same call: the chain resolution cache keyed a call by its text
/// alone, so the first branch's answer was handed back inside the
/// second one.
#[test]
fn call_narrowing_does_not_leak_into_a_later_check() {
    let backend = create_test_backend();
    let uri = "file:///call_later_check.php";
    let text = format!(
        "{SCAFFOLD}
interface JobData {{}}
class Callback implements JobData {{
    public function getAgreementID(): string {{ return ''; }}
    public function getPaymentID(): string {{ return ''; }}
}}
class Agreement implements JobData {{
    public function getAgreementID(): string {{ return ''; }}
}}
class OrderJob implements JobData {{
    public function getOrder(): string {{ return ''; }}
}}
class Failed {{
    public function data(): ?JobData {{ return null; }}
}}
class C {{
    public function m(Failed $f): void {{
        if ($f->data() instanceof Callback || $f->data() instanceof Agreement) {{
            takeString($f->data()->getAgreementID());
            if ($f->data() instanceof Callback) {{
                takeString($f->data()->getPaymentID());
            }}
        }}
        if ($f->data() instanceof OrderJob) {{
            takeString($f->data()->getOrder());
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "Each branch's own check decides what `$f->data()` is inside it, \
         got: {diags:?}"
    );
}

/// A truthy check narrows a nullable call the same way it narrows a
/// nullable property.
#[test]
fn truthy_check_strips_null_from_a_call() {
    let backend = create_test_backend();
    let uri = "file:///call_truthy.php";
    let text = format!(
        "{SCAFFOLD}
class Sub {{
    public function ending(): string {{ return ''; }}
}}
class Ord {{
    public function getSub(): ?Sub {{ return null; }}
    public function m(): void {{
        if ($this->getSub()) {{
            takeString($this->getSub()->ending());
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "`if ($this->getSub())` should leave the repeated call non-null, \
         got: {diags:?}"
    );
}

/// Reassigning the base variable between the check and the use makes the
/// narrowing stale: `$arg->value` after `$arg = $other` is a different
/// value, so the member that only the narrowed type has must be flagged.
#[test]
fn reassigning_the_base_variable_drops_property_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///base_reassigned.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Arg $arg, Arg $other): void {{
        if ($arg->value instanceof StringExpr) {{
            $arg = $other;
            takeString($arg->value->value);
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert_eq!(
        diags.len(),
        1,
        "Reassigning `$arg` must drop the narrowing on `$arg->value`, \
         got: {diags:?}"
    );
}

/// The same for an argument-less call subject: once the receiver is
/// replaced, a check on `$holder->getReturnType()` says nothing about
/// what the call hands back now.
#[test]
fn reassigning_the_base_variable_drops_call_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///base_reassigned_call.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Holder $holder, Holder $other): void {{
        if ($holder->getReturnType() instanceof StringExpr) {{
            $holder = $other;
            takeString($holder->getReturnType()->value);
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert_eq!(
        diags.len(),
        1,
        "Reassigning `$holder` must drop the narrowing on its call, \
         got: {diags:?}"
    );
}

/// A reassignment *before* the check is irrelevant: the check runs on the
/// new value, so the narrowing stands.
#[test]
fn reassignment_before_the_check_keeps_property_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///base_reassigned_early.php";
    let text = format!(
        "{SCAFFOLD}
class C {{
    public function m(Arg $arg, Arg $other): void {{
        $arg = $other;
        if ($arg->value instanceof StringExpr) {{
            takeString($arg->value->value);
        }}
    }}
}}
"
    );
    let diags = unknown_member_diagnostics(&backend, uri, &text);
    assert!(
        diags.is_empty(),
        "A reassignment before the check must not disturb narrowing, \
         got: {diags:?}"
    );
}

// ─── Scalar checks on a property subject ────────────────────────────────────
//
// A check that rules out a scalar member of a union (`!== false`,
// `!== null`, `!$x`, `empty($x)`) has no class to swap, so it narrows the
// property's key in the forward walker's scope rather than a `ClassInfo`
// list.  Every guard-clause shape that narrows a local this way must
// narrow a property path the same way.

/// Run slow diagnostics and keep only argument type mismatches.
fn type_error_messages(backend: &phpantom_lsp::Backend, uri: &str, text: &str) -> Vec<String> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut out);
    out.iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(
                |c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_argument"),
            )
        })
        .map(|d| d.message.clone())
        .collect()
}

/// A `Holder` whose property is a `T|false` union — the shape a `T|false`
/// return gets cached into, and the one with no permissive
/// "the caller may have guarded" escape hatch at the call site.
const HANDLE_SCAFFOLD: &str = r#"<?php
namespace Handle;

function useString(string $value): void {}

class Holder {
    public string|false $value = false;
    public string|null $maybe = null;
}
"#;

/// `if ($h->value === false) { return; }` proves the property is not
/// `false` for the rest of the function, the same way it does for a local.
#[test]
fn a_false_equality_guard_clause_narrows_a_property() {
    let backend = create_test_backend();
    let uri = "file:///handle_guard_return.php";
    let text = format!(
        "{HANDLE_SCAFFOLD}
function run(Holder $h): void {{
    if ($h->value === false) {{
        return;
    }}
    useString($h->value);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The guard does not have to return: throwing ends the path just as well.
#[test]
fn a_false_equality_guard_that_throws_narrows_a_property() {
    let backend = create_test_backend();
    let uri = "file:///handle_guard_throw.php";
    let text = format!(
        "{HANDLE_SCAFFOLD}
class C {{
    public function m(Holder $h): void {{
        if ($h->value === false) {{
            throw new \\RuntimeException();
        }}
        useString($h->value);
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// `continue` ends the iteration, so the rest of the loop body runs only
/// on the paths the guard let through.
#[test]
fn a_false_equality_guard_that_continues_narrows_a_property() {
    let backend = create_test_backend();
    let uri = "file:///handle_guard_continue.php";
    let text = format!(
        "{HANDLE_SCAFFOLD}
function run(Holder $h): void {{
    for ($i = 0; $i < 3; $i++) {{
        if ($h->value === false) {{
            continue;
        }}
        useString($h->value);
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// `!$h->value` and `empty($h->value)` name a property subject the same
/// way they name a local, and both rule out every falsy member.
#[test]
fn a_falsy_guard_clause_narrows_a_property() {
    let backend = create_test_backend();
    let uri = "file:///handle_guard_falsy.php";
    let text = format!(
        "{HANDLE_SCAFFOLD}
function bang(Holder $h): void {{
    if (!$h->value) {{
        return;
    }}
    useString($h->value);
}}

function blank(Holder $h): void {{
    if (empty($h->value)) {{
        return;
    }}
    useString($h->value);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The property path can be deeper than one hop, and `$this` is a subject
/// like any other object expression.
#[test]
fn a_guard_clause_narrows_a_chained_property_on_this() {
    let backend = create_test_backend();
    let uri = "file:///handle_guard_chain.php";
    let text = format!(
        "{HANDLE_SCAFFOLD}
class C {{
    public Holder $holder;

    public function __construct(Holder $holder) {{
        $this->holder = $holder;
    }}

    public function m(): void {{
        if ($this->holder->value === false) {{
            return;
        }}
        useString($this->holder->value);
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Only the member the guard names is ruled out: a `null` guard leaves a
/// `string|null` property narrowed to `string`, and a `false` guard on a
/// `string|false` property says nothing about a different property.
#[test]
fn a_guard_clause_narrows_only_the_property_it_names() {
    let backend = create_test_backend();
    let uri = "file:///handle_guard_scoped.php";
    let text = format!(
        "{HANDLE_SCAFFOLD}
function run(Holder $h): void {{
    if ($h->maybe === null) {{
        return;
    }}
    useString($h->maybe);
    useString($h->value);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("string|false"),
        "the unguarded property keeps its declared type, got {messages:?}"
    );
}

/// A write after the guard replaces what the guard proved: the property
/// holds whatever was assigned, not the narrowed type.
#[test]
fn a_write_after_the_guard_replaces_the_property_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///handle_guard_rewritten.php";
    let text = format!(
        "{HANDLE_SCAFFOLD}
/** @return string|false */
function readIt() {{
    return false;
}}

function run(Holder $h): void {{
    if ($h->value === false) {{
        return;
    }}
    $h->value = readIt();
    useString($h->value);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

/// A `Holder` whose argument-less method returns `T|false` — the call-key
/// equivalent of `HANDLE_SCAFFOLD`'s property, seeded the same way once
/// the guard names it.
const HANDLE_CALL_SCAFFOLD: &str = r#"<?php
namespace HandleCall;

function useString(string $value): void {}

class Holder {
    public function value(): string|false {
        return false;
    }
}
"#;

/// `if ($h->value() === false) { return; }` narrows the argument-less call
/// the same way an equivalent property check narrows a property.
#[test]
fn a_false_equality_guard_clause_narrows_an_argument_less_call() {
    let backend = create_test_backend();
    let uri = "file:///handle_call_guard_return.php";
    let text = format!(
        "{HANDLE_CALL_SCAFFOLD}
function run(Holder $h): void {{
    if ($h->value() === false) {{
        return;
    }}
    useString($h->value());
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The `!== false` guard shape from B137's original repro, narrowing a
/// call on `$this` rather than an injected argument.
#[test]
fn a_not_equal_false_guard_narrows_an_argument_less_call_on_this() {
    let backend = create_test_backend();
    let uri = "file:///handle_call_guard_this.php";
    let text = format!(
        "{HANDLE_CALL_SCAFFOLD}
class C {{
    private Holder $holder;

    public function __construct(Holder $holder) {{
        $this->holder = $holder;
    }}

    public function run(): void {{
        if ($this->holder->value() !== false) {{
            useString($this->holder->value());
        }}
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

// ─── `false` narrowing in the inverse direction (B136) ──────────────────────
//
// The guard-clause form (`if ($value === false) { return; }`) already
// narrowed correctly; what was missing was the inverse direction: an
// explicit `else`, `!empty()`, and the implicit else an `||` guard's De
// Morgan expansion produces.

const READ_IT_SCAFFOLD: &str = r#"<?php
namespace ReadIt;

function useString(string $value): void {}

/** @return string|false */
function readIt() {
    return false;
}
"#;

/// The `else` branch of `$value === false` proves the opposite of the
/// then-branch: `$value` is not `false` there.
#[test]
fn a_false_equality_check_narrows_the_else_branch() {
    let backend = create_test_backend();
    let uri = "file:///read_it_else.php";
    let text = format!(
        "{READ_IT_SCAFFOLD}
function run(): void {{
    $value = readIt();
    if ($value === false) {{
        // ...
    }} else {{
        useString($value);
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// `!empty($value)` rules out every falsy value, `false` included, not
/// just `null`.
#[test]
fn a_not_empty_check_narrows_out_false() {
    let backend = create_test_backend();
    let uri = "file:///read_it_not_empty.php";
    let text = format!(
        "{READ_IT_SCAFFOLD}
function run(): void {{
    $value = readIt();
    if (!empty($value)) {{
        useString($value);
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The implicit else an `||` guard clause's De Morgan expansion produces:
/// falling through past the guard means every operand was false, so
/// `$value === false` being false proves `$value` is not `false`.
#[test]
fn an_or_guard_clause_narrows_out_false() {
    let backend = create_test_backend();
    let uri = "file:///read_it_or_guard.php";
    let text = format!(
        "{READ_IT_SCAFFOLD}
function run(): void {{
    $value = readIt();
    if ($value === false || rand(0, 1)) {{
        return;
    }}
    useString($value);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Scaffolding for the `if (!guard1 || !guard2) { exit; }` idiom: a wide
/// union parameter and consumers that only accept one of its members.
const EXIT_GUARD_SCAFFOLD: &str = r#"<?php
namespace ExitGuard;

function useString(string $value): void {}
function useArray(array $value): void {}
function useResource($handle): void {}
"#;

/// A negated compound guard that leaves the scope by `return` narrows the
/// fall-through by every conjunct, not just the last one.
#[test]
fn a_negated_or_guard_with_a_return_narrows_each_conjunct() {
    let backend = create_test_backend();
    let uri = "file:///exit_guard_return.php";
    let text = format!(
        "{EXIT_GUARD_SCAFFOLD}
function run(string|array|null $payload): void {{
    if (! is_string($payload) || $payload === '') {{
        return;
    }}
    useString($payload);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// `throw` and `continue` end the branch the same way `return` does.
#[test]
fn a_negated_or_guard_narrows_for_every_exit_form() {
    let backend = create_test_backend();
    let uri = "file:///exit_guard_forms.php";
    let text = format!(
        "{EXIT_GUARD_SCAFFOLD}
function thrown(string|array|null $payload): void {{
    if (! is_string($payload) || $payload === '') {{
        throw new \\Exception('bad');
    }}
    useString($payload);
}}

/** @param iterable<string|array|null> $items */
function skipped(iterable $items): void {{
    foreach ($items as $payload) {{
        if (! is_string($payload) || $payload === '') {{
            continue;
        }}
        useString($payload);
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Three conjuncts, so the fall-through depends on more than a pair.
#[test]
fn a_three_way_negated_or_guard_narrows_every_conjunct() {
    let backend = create_test_backend();
    let uri = "file:///exit_guard_three.php";
    let text = format!(
        "{EXIT_GUARD_SCAFFOLD}
function run(string|array|null|int $value): void {{
    if (! is_string($value) || $value === '' || \\strlen($value) > 5) {{
        return;
    }}
    useString($value);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// `is_array` and `is_resource` narrow in a negated compound guard too.
#[test]
fn is_array_and_is_resource_narrow_in_a_negated_guard() {
    let backend = create_test_backend();
    let uri = "file:///exit_guard_array_resource.php";
    let text = format!(
        "{EXIT_GUARD_SCAFFOLD}
function arrays(string|array|null $value): void {{
    if (! is_array($value) || $value === []) {{
        return;
    }}
    useArray($value);
}}

function resources(mixed $handle): void {{
    if (! is_resource($handle)) {{
        return;
    }}
    useResource($handle);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The `else` of the guard sees the same narrowing the fall-through does.
#[test]
fn the_else_of_a_negated_or_guard_narrows_each_conjunct() {
    let backend = create_test_backend();
    let uri = "file:///exit_guard_else.php";
    let text = format!(
        "{EXIT_GUARD_SCAFFOLD}
function run(string|array|null $payload): void {{
    if (! is_string($payload) || $payload === '') {{
        return;
    }} else {{
        useString($payload);
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Scaffolding for ternary-arm narrowing: consumers that accept exactly one
/// member of a wide union.
const TERNARY_SCAFFOLD: &str = r#"<?php
namespace Ternary;

class Icon {}

function useString(string $value): void {}
function useInt(int $value): void {}
function useIcon(Icon $value): void {}
function passThrough(string $value): string { return $value; }
"#;

/// A type-guard condition narrows the then arm, in assignment position.
#[test]
fn a_type_guard_ternary_narrows_its_then_arm() {
    let backend = create_test_backend();
    let uri = "file:///ternary_then.php";
    let text = format!(
        "{TERNARY_SCAFFOLD}
function run(string|array|null $req): void {{
    $period = is_string($req) ? $req : 'today';
    useString($period);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The same ternary in argument position, where the sweep found the bulk of
/// the false positives.
#[test]
fn a_type_guard_ternary_narrows_in_argument_position() {
    let backend = create_test_backend();
    let uri = "file:///ternary_arg.php";
    let text = format!(
        "{TERNARY_SCAFFOLD}
function run(string|array|null $req): void {{
    useString(is_string($req) ? $req : 'today');
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A negated condition puts the narrowing on the else arm instead.
#[test]
fn a_negated_ternary_narrows_its_else_arm() {
    let backend = create_test_backend();
    let uri = "file:///ternary_negated.php";
    let text = format!(
        "{TERNARY_SCAFFOLD}
function run(string|array|null $req): void {{
    $period = ! is_string($req) ? 'today' : $req;
    useString($period);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A null check and a bare truthy check narrow their then arm too.
#[test]
fn a_null_check_ternary_narrows_its_then_arm() {
    let backend = create_test_backend();
    let uri = "file:///ternary_null.php";
    let text = format!(
        "{TERNARY_SCAFFOLD}
function explicit(?string $s): void {{
    useString($s !== null ? $s : 'x');
}}

function truthy(?string $s): void {{
    useString($s ? passThrough($s) : 'x');
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A nested ternary's else arm carries the outer condition's inverse
/// narrowing as well as its own.
#[test]
fn a_nested_ternary_carries_the_outer_inverse_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///ternary_nested.php";
    let text = format!(
        "{TERNARY_SCAFFOLD}
function run(string|int|null $v): void {{
    $out = is_string($v) ? passThrough($v) : (is_int($v) ? $v : 0);
    useInt(is_string($v) ? 0 : (is_int($v) ? $v : 0));
    useString(is_string($v) ? $v : 'x');
    echo $out;
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// `instanceof` in a ternary condition narrows the arm to the class.
#[test]
fn an_instanceof_ternary_narrows_its_then_arm() {
    let backend = create_test_backend();
    let uri = "file:///ternary_instanceof.php";
    let text = format!(
        "{TERNARY_SCAFFOLD}
function run(Icon|string $v): void {{
    useIcon($v instanceof Icon ? $v : new Icon());
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A `throw` is an expression like any other, so a ternary inside the
/// value it throws narrows exactly as it would inside a `return`.
#[test]
fn a_ternary_inside_a_throw_narrows_its_arms() {
    let backend = create_test_backend();
    let uri = "file:///ternary_throw.php";
    let text = format!(
        "{TERNARY_SCAFFOLD}
function bare(?string $s): void {{
    throw new \\RuntimeException($s ? passThrough($s) : 'x');
}}

function concatenated(?string $s): void {{
    throw new \\RuntimeException('got: ' . ($s ? passThrough($s) : 'x'));
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Scaffolding for short-circuit narrowing: a wide union source and
/// consumers that accept only part of it.
const SHORT_CIRCUIT_SCAFFOLD: &str = r#"<?php
namespace ShortCircuit;

function useStringBool(string $value): bool { return true; }
function useArrayBool(array $value): bool { return true; }
function useStringOrArray(string|array $value): void {}

class Source {
    /** @return string|array|null */
    public function read() { return null; }
}
"#;

/// The right operand of `&&` sees the left operand's positive narrowing,
/// and the right operand of `||` sees its negative narrowing.
#[test]
fn short_circuit_operands_see_the_left_operands_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///short_circuit.php";
    let text = format!(
        "{SHORT_CIRCUIT_SCAFFOLD}
function andRight(string|array|null $v): bool {{
    return is_string($v) && useStringBool($v);
}}

function andRightArray(string|array|null $v): bool {{
    return is_array($v) && useArrayBool($v);
}}

function orRight(string|array|null $v): bool {{
    return ! is_string($v) || useStringBool($v);
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A property subject narrows across the operator too.
#[test]
fn short_circuit_operands_narrow_a_property_subject() {
    let backend = create_test_backend();
    let uri = "file:///short_circuit_property.php";
    let text = format!(
        "{SHORT_CIRCUIT_SCAFFOLD}
class Holder {{
    /** @var array<string, string>|string|null */
    private $address;

    public function andProperty(): bool {{
        return is_array($this->address) && useArrayBool($this->address);
    }}

    public function orProperty(): bool {{
        return ! is_array($this->address) || useArrayBool($this->address);
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// An assignment inside an `elseif` condition truthy-narrows the variable
/// it wrote, the same way the leading `if` form already did.
#[test]
fn an_assignment_in_an_elseif_condition_narrows_its_variable() {
    let backend = create_test_backend();
    let uri = "file:///elseif_assignment.php";
    let text = format!(
        "{SHORT_CIRCUIT_SCAFFOLD}
function leading(Source $source): void {{
    if ($value = $source->read()) {{
        useStringOrArray($value);
    }}
}}

function trailing(Source $source, bool $flag): void {{
    if ($flag) {{
        return;
    }} elseif ($value = $source->read()) {{
        useStringOrArray($value);
    }}
}}

function alternativeSyntax(Source $source, bool $flag): void {{
    if ($flag):
        return;
    elseif ($value = $source->read()):
        useStringOrArray($value);
    endif;
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

// ─── Call-expression subjects ───────────────────────────────────────────

/// Scaffolding for narrowing keyed on a *call* rather than a variable:
/// a free function and a method that both return a union, plus consumers
/// that reject the un-narrowed half.
const CALL_SUBJECT_SCAFFOLD: &str = r#"<?php
namespace CallSubject;

function findPos(string $haystack, string $needle): int|false { return 0; }
function useInt(int $value): void {}
function useString(string $value): void {}

class Options {
    public function position(string $key): int|false { return false; }
    public static function lookup(string $key): int|false { return false; }
}
"#;

/// A guarded call re-written verbatim inside the branch keeps the
/// narrowing the guard proved.
#[test]
fn a_repeated_function_call_keeps_the_guards_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///call_subject_function.php";
    let text = format!(
        "{CALL_SUBJECT_SCAFFOLD}
function guarded(string $slug, string $marker): void {{
    if (findPos($slug, $marker) !== false) {{
        useInt(findPos($slug, $marker));
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The same for a method call that takes an argument, in both the
/// `if`-body and ternary-arm forms.
#[test]
fn a_repeated_method_call_keeps_the_guards_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///call_subject_method.php";
    let text = format!(
        "{CALL_SUBJECT_SCAFFOLD}
function guarded(Options $opts): void {{
    if ($opts->position('from') !== false) {{
        useInt($opts->position('from'));
    }}
}}

function ternary(Options $opts): void {{
    $from = $opts->position('from') !== false ? useInt($opts->position('from')) : null;
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A static call is keyed on its class and arguments the same way.
#[test]
fn a_repeated_static_call_keeps_the_guards_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///call_subject_static.php";
    let text = format!(
        "{CALL_SUBJECT_SCAFFOLD}
function guarded(): void {{
    if (Options::lookup('from') !== false) {{
        useInt(Options::lookup('from'));
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Writing to a variable the key names invalidates the narrowing: the
/// call is a different call once its argument changed.  A variable whose
/// name merely *starts* the same is not one of its inputs.
#[test]
fn reassigning_an_argument_drops_the_calls_narrowing() {
    let backend = create_test_backend();
    let uri = "file:///call_subject_invalidation.php";
    let text = format!(
        "{CALL_SUBJECT_SCAFFOLD}
function reassign(string $slug, string $marker, string $other): void {{
    if (findPos($slug, $marker) !== false) {{
        $marker = $other;
        useInt(findPos($slug, $marker));
    }}
}}

function rewriteSubject(string $slug, string $marker, string $other): void {{
    if (findPos($slug, $marker) !== false) {{
        $slug = $other;
        useInt(findPos($slug, $marker));
    }}
}}

function nearMiss(string $slug, string $slugger, string $marker, string $other): void {{
    if (findPos($slug, $marker) !== false) {{
        $slugger = $other;
        useString($slugger);
        useInt(findPos($slug, $marker));
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert_eq!(messages.len(), 2, "got {messages:?}");
}

/// A call whose result differs from one invocation to the next is not
/// keyed at all: the check on the first says nothing about the second.
#[test]
fn a_state_advancing_call_is_not_keyed() {
    let backend = create_test_backend();
    let uri = "file:///call_subject_nondeterministic.php";
    let text = format!(
        "{CALL_SUBJECT_SCAFFOLD}
function fgets($handle): string|false {{ return false; }}

function readTwice($handle): void {{
    if (fgets($handle) !== false) {{
        useString(fgets($handle));
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

/// The proof does not survive the branch it was made in, nor an
/// iteration of the loop that contains it.
#[test]
fn a_calls_narrowing_ends_with_its_branch() {
    let backend = create_test_backend();
    let uri = "file:///call_subject_scope.php";
    let text = format!(
        "{CALL_SUBJECT_SCAFFOLD}
function afterBranch(string $slug, string $marker): void {{
    if (findPos($slug, $marker) !== false) {{
        useInt(findPos($slug, $marker));
    }}
    useInt(findPos($slug, $marker));
}}

function acrossIterations(string $slug, string $marker): void {{
    while (true) {{
        useInt(findPos($slug, $marker));
        if (findPos($slug, $marker) !== false) {{
            $slug .= 'x';
        }}
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert_eq!(messages.len(), 2, "got {messages:?}");
}

/// Scaffolding for an `instanceof` check on a subject whose declared type
/// names no class: the shape a route parameter or a container lookup
/// arrives as.
const BROAD_SUBJECT_SCAFFOLD: &str = r#"<?php
namespace BroadSubject;

class Server {}

class Auth {
    public function canManageServer(Server $server): bool { return true; }
}

function auth_user(): Auth { return new Auth(); }
function abort(int $code, string $message): void {}
"#;

/// `instanceof` on an `object|string` subject proves the subject *is* the
/// class: the `object` alternative is subsumed by it and the `string` one
/// is ruled out by the check succeeding, so neither may survive to be
/// judged against a parameter type.
#[test]
fn an_instanceof_check_replaces_a_union_that_names_no_class() {
    let backend = create_test_backend();
    let uri = "file:///broad_subject.php";
    let text = format!(
        "{BROAD_SUBJECT_SCAFFOLD}
class Controller {{
    /** @param object|string $server */
    public function orChain($server): void {{
        if (! $server instanceof Server || ! auth_user()->canManageServer($server)) {{
            abort(403, 'nope');
        }}
    }}

    /** @param object|string $server */
    public function andChain($server): bool {{
        return $server instanceof Server && auth_user()->canManageServer($server);
    }}

    /** @param object|string $server */
    public function truthyBranch($server): void {{
        if ($server instanceof Server) {{
            auth_user()->canManageServer($server);
        }}
    }}

    /** @param object|string $server */
    public function guardClause($server): void {{
        if (! $server instanceof Server) {{
            abort(403, 'nope');
            return;
        }}
        auth_user()->canManageServer($server);
    }}
}}
"
    );
    let messages = type_error_messages(&backend, uri, &text);
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Regression: property narrowing next to a chained method call must
/// complete in bounded time.  Before the fix, each `if ($o->a)` triggered
/// a full-body re-walk for property narrowing, and each of those re-walks
/// resolved the chained call `$h->getWork()` which re-entered
/// `apply_property_narrowing` for the call key, creating exponential
/// fan-out.  With 8 pairs the analysis took over 50 seconds.
#[test]
fn property_narrowing_next_to_chained_calls_completes_in_bounded_time() {
    let backend = create_test_backend();
    let lib_uri = "file:///repro_lib.php";
    let lib_text = r#"<?php
class Work   { public function changed(string $k): bool { return true; } public function run(): void {} }
class Holder { public function getWork(): Work { return new Work(); } }
class Ops    { public bool $a = false; }
"#;
    backend.update_ast(lib_uri, lib_text);

    let probe_uri = "file:///repro_probe.php";
    let probe_text = r#"<?php
function probe(Holder $h): void {
  $o = new Ops();
  if ($h->getWork()->changed("k")) { $o->a = true; }
  if ($o->a) { $h->getWork()->run(); }
  if ($h->getWork()->changed("k")) { $o->a = true; }
  if ($o->a) { $h->getWork()->run(); }
  if ($h->getWork()->changed("k")) { $o->a = true; }
  if ($o->a) { $h->getWork()->run(); }
  if ($h->getWork()->changed("k")) { $o->a = true; }
  if ($o->a) { $h->getWork()->run(); }
  if ($h->getWork()->changed("k")) { $o->a = true; }
  if ($o->a) { $h->getWork()->run(); }
  if ($h->getWork()->changed("k")) { $o->a = true; }
  if ($o->a) { $h->getWork()->run(); }
  if ($h->getWork()->changed("k")) { $o->a = true; }
  if ($o->a) { $h->getWork()->run(); }
  if ($h->getWork()->changed("k")) { $o->a = true; }
  if ($o->a) { $h->getWork()->run(); }
}
"#;
    let mut diags = Vec::new();
    backend.update_ast(probe_uri, probe_text);
    backend.collect_slow_diagnostics(probe_uri, probe_text, &mut diags);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}
