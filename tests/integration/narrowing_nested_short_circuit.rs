//! Short-circuit narrowing when the `&&` / `||` chain is nested inside a
//! larger expression.
//!
//! A chain proves the same things about its own operands wherever it
//! sits.  Recording that proof only when the chain was the whole
//! condition of an `if`/`while`/`for` or the whole `return` value left
//! every other position (an assignment's right-hand side, a call
//! argument, an array element, a ternary condition, a nested operand)
//! reading the un-narrowed type.

use crate::common::{
    create_test_backend, create_test_backend_with_function_stubs, slow_diagnostic_messages,
};

/// Collect argument-type diagnostics through the slow pipeline, which is
/// what activates the forward walker's scope-snapshot cache.
fn type_errors_with(backend: phpantom_lsp::Backend, php: &str) -> Vec<String> {
    slow_diagnostic_messages(
        &backend,
        "file:///nested_short_circuit.php",
        php,
        "type_mismatch_argument",
    )
}

/// The positions matrix only needs user-declared functions, so it runs
/// without stubs — a missing builtin signature would otherwise report
/// nothing and let the test pass for the wrong reason.
fn type_errors(php: &str) -> Vec<String> {
    type_errors_with(create_test_backend(), php)
}

/// `array_key_exists` needs a real signature to check its argument
/// against.
fn type_errors_with_stubs(php: &str) -> Vec<String> {
    type_errors_with(create_test_backend_with_function_stubs(), php)
}

/// Every position a chain can appear in, checked in one pass so a
/// regression names the position it broke.
#[test]
fn a_chain_narrows_its_operands_in_every_expression_position() {
    let cases: [(&str, &str); 8] = [
        (
            "assignment right-hand side",
            "$ok = is_string($c) && check($c);",
        ),
        ("call argument", "takesBool(is_string($c) && check($c));"),
        ("array element", "$row = [is_string($c) && check($c)];"),
        (
            "nested operand of an outer chain",
            "$ok = (is_string($c) && check($c)) && true;",
        ),
        (
            "ternary condition",
            "$n = is_string($c) && check($c) ? 1 : 0;",
        ),
        (
            "`||` in an assignment right-hand side",
            "$ok = null === $c || check($c);",
        ),
        (
            "`||` in a call argument",
            "takesBool(null === $c || check($c));",
        ),
        (
            "`||` as the first operand of an outer chain",
            "$ok = (null === $c || check($c)) && true;",
        ),
    ];

    for (position, statement) in cases {
        let php = format!(
            "<?php
function takesBool(bool $b): void {{}}
function check(string $s): bool {{ return $s !== ''; }}
function probe(?string $c): void {{
    {statement}
}}
"
        );
        let errors = type_errors(&php);
        assert!(
            errors.is_empty(),
            "a chain in a {position} should narrow `$c` for the operand \
             that follows the guard, got: {errors:?}"
        );
    }
}

/// The chain's subject is a property rather than a plain variable, and
/// the chain sits in a ternary condition inside a `return`.
#[test]
fn a_property_guard_narrows_the_conjunct_that_follows_it_inside_a_ternary() {
    let php = r#"<?php
class Holder
{
    /** @var null|array<string, string> */
    public ?array $data = null;

    public function field(string $key): ?string
    {
        return is_array($this->data) && array_key_exists($key, $this->data)
            ? $this->data[$key]
            : null;
    }
}
"#;
    let errors = type_errors_with_stubs(php);
    assert!(
        errors.is_empty(),
        "`is_array($this->data)` should narrow the property for the \
         `array_key_exists` conjunct beside it, got: {errors:?}"
    );
}

/// The narrowing must stay inside the chain: an operand that proves
/// nothing must not silence a genuine mismatch after it.
#[test]
fn a_chain_in_a_nested_position_does_not_narrow_past_itself() {
    let php = r#"<?php
function takesString(string $s): void {}
function check(string $s): bool { return $s !== ''; }
function probe(?string $c): void {
    $ok = is_string($c) && check($c);
    takesString($c);
}
"#;
    let errors = type_errors(php);
    assert!(
        errors.iter().any(|m| m.contains("expects string")),
        "the chain proves nothing about `$c` after the statement it \
         sits in, so the later call must still be reported, got: {errors:?}"
    );
}
