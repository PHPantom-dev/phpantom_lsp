use crate::common::{collect_diagnostics_with, create_test_backend, messages_with_code};
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect(php: &str) -> Vec<Diagnostic> {
    collect_diagnostics_with(
        &create_test_backend(),
        php,
        Backend::collect_return_type_diagnostics,
    )
}

// ─── Basic: return wrong type from function ─────────────────────────────────

#[test]
fn flags_array_returned_from_string_function_basic() {
    let php = r#"<?php
function get_name(): string {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected return type error for array returned from string function, got: {diags:?}"
    );
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert!(
        msgs.iter().any(|m| m.contains("incompatible")),
        "Expected message about incompatible return, got: {msgs:?}"
    );
}

// ─── Basic: correct return type — no diagnostic ────────────────────────────

#[test]
fn no_diagnostic_for_correct_return_type() {
    let php = r#"<?php
function get_name(): string {
    return "hello";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct return type, got: {diags:?}"
    );
}

// ─── Return null from non-nullable ─────────────────────────────────────────

#[test]
fn flags_null_returned_from_non_nullable() {
    let php = r#"<?php
function get_count(): int {
    return null;
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected return type error for null returned from int function, got: {diags:?}"
    );
}

// ─── Return null from nullable — OK ────────────────────────────────────────

#[test]
fn no_diagnostic_for_null_from_nullable() {
    let php = r#"<?php
function maybe_name(): ?string {
    return null;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag null returned from ?string, got: {diags:?}"
    );
}

// ─── Void function returning a value — error ───────────────────────────────

#[test]
fn flags_value_returned_from_void_function() {
    let php = r#"<?php
function do_nothing(): void {
    return 42;
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for value returned from void function, got: {diags:?}"
    );
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert!(
        msgs.iter()
            .any(|m| m.contains("Void") || m.contains("void")),
        "Expected message about void function, got: {msgs:?}"
    );
}

// ─── Void function with bare return — OK ───────────────────────────────────

#[test]
fn no_diagnostic_for_bare_return_in_void() {
    let php = r#"<?php
function do_nothing(): void {
    return;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag bare return in void function, got: {diags:?}"
    );
}

// ─── Bare return in non-void function — error ──────────────────────────────

#[test]
fn flags_bare_return_in_typed_function() {
    let php = r#"<?php
function get_name(): string {
    return;
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for bare return in string function, got: {diags:?}"
    );
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert!(
        msgs.iter()
            .any(|m| m.contains("must not return without a value")),
        "Expected message about missing return value, got: {msgs:?}"
    );
}

// ─── Void method returning a value — error ─────────────────────────────────

#[test]
fn flags_value_returned_from_void_method() {
    let php = r#"<?php
class Foo {
    public function doStuff(): void {
        return "oops";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for value returned from void method, got: {diags:?}"
    );
}

// ─── Method return type mismatch ────────────────────────────────────────────

#[test]
fn flags_wrong_return_type_in_method() {
    let php = r#"<?php
class Calculator {
    public function add(int $a, int $b): int {
        return "not a number";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected return type error in method, got: {diags:?}"
    );
}

// ─── Method correct return — no diagnostic ─────────────────────────────────

#[test]
fn no_diagnostic_for_correct_method_return() {
    let php = r#"<?php
class Calculator {
    public function add(int $a, int $b): int {
        return $a + $b;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct method return, got: {diags:?}"
    );
}

// ─── Multiple returns — only wrong ones flagged ────────────────────────────

#[test]
fn only_flags_wrong_returns_in_branching() {
    let php = r#"<?php
function get_value(bool $flag): string {
    if ($flag) {
        return "hello";
    }
    return [];
}
"#;
    let diags = collect(php);
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one return error (for the array return), got: {msgs:?}"
    );
}

// ─── Return inside try/catch ────────────────────────────────────────────────

#[test]
fn flags_wrong_return_in_try_catch() {
    let php = r#"<?php
function fetch(): string {
    try {
        return [];
    } catch (\Exception $e) {
        return "fallback";
    }
}
"#;
    let diags = collect(php);
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert_eq!(
        msgs.len(),
        1,
        "Expected one return error (in try block), got: {msgs:?}"
    );
}

// ─── Union return type ─────────────────────────────────────────────────────

#[test]
fn no_diagnostic_for_union_return_type() {
    let php = r#"<?php
function get_value(): string|int {
    return 42;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag int returned from string|int, got: {diags:?}"
    );
}

// ─── No return type declared — no diagnostic ───────────────────────────────

#[test]
fn no_diagnostic_when_no_return_type() {
    let php = r#"<?php
function get_value() {
    return 42;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag when no return type declared, got: {diags:?}"
    );
}

// ─── Mixed return type — no diagnostic ─────────────────────────────────────

#[test]
fn no_diagnostic_for_mixed_return() {
    let php = r#"<?php
function get_anything(): mixed {
    return 42;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag mixed return type, got: {diags:?}"
    );
}

// ─── Return in nested closure is not checked against outer function ─────────

#[test]
fn closure_return_does_not_affect_outer() {
    let php = r#"<?php
function get_processor(): string {
    $fn = function(): int {
        return 42;
    };
    return "hello";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Closure's int return should not be checked against outer string type, got: {diags:?}"
    );
}

// ─── Return bool from int function ─────────────────────────────────────────

#[test]
fn flags_bool_returned_from_int_function() {
    let php = r#"<?php
function get_count(): int {
    return true;
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected return type error for bool returned from int function, got: {diags:?}"
    );
}

// ─── Return string from int function ───────────────────────────────────────

#[test]
fn flags_string_returned_from_int_function() {
    let php = r#"<?php
function get_count(): int {
    return "not a number";
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected return type error for string returned from int function, got: {diags:?}"
    );
}

// ─── Array returned from string function ───────────────────────────────────

#[test]
fn flags_array_returned_from_string_function() {
    let php = r#"<?php
function get_name(): string {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected return type error for array returned from string function, got: {diags:?}"
    );
}

// ─── Return from switch/case ───────────────────────────────────────────────

#[test]
fn flags_wrong_return_in_switch() {
    let php = r#"<?php
function label(int $code): string {
    switch ($code) {
        case 1:
            return "one";
        default:
            return [];
    }
}
"#;
    let diags = collect(php);
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert_eq!(
        msgs.len(),
        1,
        "Expected one return error (default case), got: {msgs:?}"
    );
}

// ─── Bare return in nullable function — still an error ─────────────────────

#[test]
fn flags_bare_return_in_nullable_function() {
    let php = r#"<?php
function maybe_name(): ?string {
    return;
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for bare return in ?string function (should use return null), got: {diags:?}"
    );
}

// ─── Void method with bare return — OK ─────────────────────────────────────

#[test]
fn no_diagnostic_for_bare_return_in_void_method() {
    let php = r#"<?php
class Foo {
    public function reset(): void {
        return;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag bare return in void method, got: {diags:?}"
    );
}

// ─── Return in foreach loop ────────────────────────────────────────────────

#[test]
fn flags_wrong_return_in_foreach() {
    let php = r#"<?php
function find_name(array $items): string {
    foreach ($items as $item) {
        return [];
    }
    return "default";
}
"#;
    let diags = collect(php);
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert_eq!(
        msgs.len(),
        1,
        "Expected one return error (inside foreach), got: {msgs:?}"
    );
}

// ─── Return string from void method — error ────────────────────────────────

#[test]
fn flags_string_returned_from_void_method() {
    let php = r#"<?php
class Service {
    public function process(): void {
        return "done";
    }
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for string returned from void method, got: {diags:?}"
    );
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert!(
        msgs.iter()
            .any(|m| m.contains("Void") || m.contains("void")),
        "Expected void-related message, got: {msgs:?}"
    );
}

// ─── Return in while loop ──────────────────────────────────────────────────

#[test]
fn flags_wrong_return_in_while() {
    let php = r#"<?php
function search(): string {
    while (true) {
        return false;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected return type error in while loop, got: {diags:?}"
    );
}

// ─── Multiple bare returns in void — all OK ────────────────────────────────

#[test]
fn no_diagnostic_for_multiple_bare_returns_in_void() {
    let php = r#"<?php
function process(bool $flag): void {
    if ($flag) {
        return;
    }
    return;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag multiple bare returns in void function, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: generators (yield) must be skipped entirely
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_generator_returning_int() {
    // Generator functions have different return semantics — the declared
    // return type describes the Generator wrapper, not the yielded values.
    let php = r#"<?php
function gen(): \Generator {
    yield 1;
    yield 2;
    return "done";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag return in generator function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_generator_method() {
    let php = r#"<?php
class Streamer {
    public function items(): \Generator {
        yield "a";
        yield "b";
        return 42;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag return in generator method, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_generator_with_yield_in_loop() {
    let php = r#"<?php
function range_gen(int $start, int $end): \Generator {
    for ($i = $start; $i <= $end; $i++) {
        yield $i;
    }
    return "finished";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag generator with yield in loop, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_generator_yield_in_if() {
    let php = r#"<?php
function conditional_gen(bool $flag): \Generator {
    if ($flag) {
        yield 1;
    }
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag generator with yield inside if, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_generator_yield_in_try() {
    let php = r#"<?php
function safe_gen(): \Generator {
    try {
        yield "data";
    } catch (\Exception $e) {
        yield "error";
    }
    return false;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag generator with yield inside try/catch, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_generator_yield_in_switch() {
    let php = r#"<?php
function switch_gen(int $mode): \Generator {
    switch ($mode) {
        case 1:
            yield "one";
            break;
        default:
            yield "other";
    }
    return 0;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag generator with yield in switch, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: closures and arrow functions inside functions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn closure_returning_wrong_type_does_not_affect_outer_method() {
    let php = r#"<?php
class Service {
    public function process(): string {
        $fn = function(): int {
            return 42;
        };
        return "result";
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Closure's int return must not leak to outer string method, got: {diags:?}"
    );
}

#[test]
fn arrow_function_does_not_affect_outer() {
    let php = r#"<?php
function get_mapper(): string {
    $fn = fn(int $x): int => $x * 2;
    return "done";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Arrow function return should not affect outer function, got: {diags:?}"
    );
}

#[test]
fn nested_closure_with_array_map_does_not_leak() {
    let php = r#"<?php
function transform(array $items): string {
    $mapped = array_map(function($item): array {
        return [$item];
    }, $items);
    return "ok";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Closure in array_map should not leak return type to outer, got: {diags:?}"
    );
}

#[test]
fn nested_function_declaration_does_not_leak() {
    let php = r#"<?php
function outer(): string {
    function inner(): int {
        return 42;
    }
    return "hello";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Nested function return should not leak to outer, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: nullable and union return types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_string_from_nullable_string() {
    let php = r#"<?php
function maybe(): ?string {
    return "hello";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag string returned from ?string, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_int_from_union_int_string() {
    let php = r#"<?php
function flexible(): int|string {
    return 42;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag int returned from int|string, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_string_from_union_int_string() {
    let php = r#"<?php
function flexible(): int|string {
    return "hello";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag string returned from int|string, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_null_from_union_string_null() {
    let php = r#"<?php
function maybe(): string|null {
    return null;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag null returned from string|null, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_nullable_union_multiple_branches() {
    let php = r#"<?php
function resolve(bool $a, bool $b): int|string|null {
    if ($a) {
        return 42;
    } elseif ($b) {
        return "text";
    }
    return null;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag any branch of int|string|null, got: {diags:?}"
    );
}

#[test]
fn flags_array_from_union_int_string() {
    // array is not in int|string union — should flag
    let php = r#"<?php
function flexible(): int|string {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for array returned from int|string, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: type juggling (non-strict mode)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_int_returned_from_string_function_non_strict() {
    // PHP coerces int to string in non-strict mode.
    let php = r#"<?php
function label(): string {
    return 42;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag int returned from string function (type juggling), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_float_returned_from_string_function_non_strict() {
    let php = r#"<?php
function label(): string {
    return 3.14;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag float returned from string function (type juggling), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_int_to_float_return() {
    // int is always widened to float in PHP.
    let php = r#"<?php
function precise(): float {
    return 42;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag int returned from float function (widening), got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: class hierarchy (subclass / interface)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_subclass_return() {
    let php = r#"<?php
class Animal {}
class Cat extends Animal {}

function get_animal(): Animal {
    return new Cat();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag subclass Cat returned from Animal function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_interface_implementor_return() {
    let php = r#"<?php
interface Printable {
    public function print(): void;
}
class Report implements Printable {
    public function print(): void {}
}

function get_printable(): Printable {
    return new Report();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag interface implementor returned from interface function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_deep_inheritance_return() {
    let php = r#"<?php
class Base {}
class Middle extends Base {}
class Leaf extends Middle {}

function get_base(): Base {
    return new Leaf();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag deep subclass returned from base function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_object_return_with_class_instance() {
    let php = r#"<?php
class Foo {}

function get_object(): object {
    return new Foo();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag class instance returned from object function, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: self / static / parent return types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_self_return_type() {
    let php = r#"<?php
class Builder {
    public function reset(): self {
        return new self();
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag new self() returned from self function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_static_return_type() {
    let php = r#"<?php
class Builder {
    public function clone(): static {
        return new static();
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag new static() returned from static function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_this_fluent_return() {
    let php = r#"<?php
class Builder {
    public function with(string $key): static {
        return $this;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag $this returned from static return type, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_self_array_return() {
    // `@return self[]` returning an array of the enclosing class. `self`
    // inside the array element must resolve to the concrete class so the
    // element types compare equal.
    let php = r#"<?php
namespace App\Models;
class Category {
    /** @return self[] */
    public function children(): array {
        return [new Category(), new Category()];
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag Category[] returned from self[] method, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

#[test]
fn no_diagnostic_for_static_array_return() {
    let php = r#"<?php
namespace App\Models;
class Category {
    /** @return static[] */
    public function children(): array {
        return [new Category()];
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag Category[] returned from static[] method, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

#[test]
fn no_diagnostic_for_intersection_return() {
    // An intersection value `A&B` satisfies each member, so returning it
    // where `A` (a member) is declared is compatible.
    let php = r#"<?php
interface HasCount {}
class Node implements HasCount {}

/** @return Node&HasCount */
function make_node(): Node&HasCount { return new Node(); }

function build(): Node {
    return make_node();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag Node&HasCount returned where Node is declared, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

#[test]
fn no_diagnostic_for_self_return_in_method_chain() {
    let php = r#"<?php
class Query {
    public function where(string $col): self {
        return $this;
    }

    public function orderBy(string $col): self {
        return $this;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag $this returned from self in chaining methods, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: Stringable objects returned as string
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_stringable_returned_as_string() {
    let php = r#"<?php
class HtmlString {
    public function __toString(): string { return ''; }
}

function render(): string {
    return new HtmlString();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag Stringable object returned from string function, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: iterable / callable / array return types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_array_returned_from_iterable() {
    let php = r#"<?php
function get_items(): iterable {
    return [1, 2, 3];
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag array returned from iterable function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_array_returned_from_array() {
    let php = r#"<?php
function get_items(): array {
    return [1, 2, 3];
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag array literal returned from array function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_closure_returned_from_callable() {
    let php = r#"<?php
function get_callback(): callable {
    return function() { return 1; };
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag closure returned from callable function, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: trait and enum methods
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_trait_method_return() {
    let php = r#"<?php
trait Describable {
    public function describe(): string {
        return "description";
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct return in trait method, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_return_in_trait_method() {
    let php = r#"<?php
trait Describable {
    public function describe(): string {
        return [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for array returned from string trait method, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_correct_enum_method_return() {
    let php = r#"<?php
enum Color {
    case Red;
    case Blue;

    public function label(): string {
        return "color";
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct return in enum method, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_return_in_enum_method() {
    let php = r#"<?php
enum Status {
    case Active;
    case Inactive;

    public function label(): string {
        return 42;
    }
}
"#;
    let diags = collect(php);
    // In non-strict mode int→string is juggled, so this actually should NOT be flagged.
    // This tests that we don't accidentally flag valid juggling in enum context.
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag int returned from string enum method (type juggling), got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: abstract methods (no body)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_abstract_method() {
    let php = r#"<?php
abstract class Shape {
    abstract public function area(): float;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag abstract method (no body), got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: interface methods (no body)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_interface_method() {
    let php = r#"<?php
interface Repository {
    public function find(int $id): object;
    public function save(object $entity): void;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag interface methods (no body), got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: no return type declared
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_untyped_method() {
    let php = r#"<?php
class Legacy {
    public function fetch() {
        return 42;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag method with no return type, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_constructor_implicit_void() {
    let php = r#"<?php
class Foo {
    public function __construct() {
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag constructor with no return statement, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: complex control flow
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_return_in_deeply_nested_if() {
    let php = r#"<?php
function nested(int $a, int $b, int $c): string {
    if ($a > 0) {
        if ($b > 0) {
            if ($c > 0) {
                return "deep";
            } else {
                return "c-neg";
            }
        } else {
            return "b-neg";
        }
    } else {
        return "a-neg";
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct returns in deeply nested if, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_correct_return_in_do_while() {
    let php = r#"<?php
function search(array $items): string {
    $i = 0;
    do {
        if (isset($items[$i])) {
            return "found";
        }
        $i++;
    } while ($i < 10);
    return "not found";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct returns in do-while, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_correct_return_in_for() {
    let php = r#"<?php
function find_index(array $items, string $target): int {
    for ($i = 0; $i < 100; $i++) {
        if (true) {
            return 0;
        }
    }
    return -1;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct returns in for loop, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_return_in_finally() {
    let php = r#"<?php
function finalize(): string {
    try {
        return "ok";
    } finally {
        return [];
    }
}
"#;
    let diags = collect(php);
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert_eq!(
        msgs.len(),
        1,
        "Expected one return error (in finally), got: {msgs:?}"
    );
}

#[test]
fn flags_only_wrong_branch_in_complex_if_else() {
    let php = r#"<?php
function classify(int $x): string {
    if ($x > 100) {
        return "big";
    } elseif ($x > 50) {
        return "medium";
    } elseif ($x > 0) {
        return [];
    } else {
        return "negative";
    }
}
"#;
    let diags = collect(php);
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one return error (the array branch), got: {msgs:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: declare(strict_types=1) interactions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn strict_types_flags_int_returned_from_string() {
    let php = r#"<?php
declare(strict_types=1);

function label(): string {
    return 42;
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for int returned from string function under strict_types=1, got: {diags:?}"
    );
}

#[test]
fn strict_types_does_not_affect_subclass_return() {
    let php = r#"<?php
declare(strict_types=1);

class Animal {}
class Cat extends Animal {}

function get_animal(): Animal {
    return new Cat();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "strict_types should not affect subclass return, got: {diags:?}"
    );
}

#[test]
fn strict_types_still_allows_null_for_nullable() {
    let php = r#"<?php
declare(strict_types=1);

function maybe(): ?string {
    return null;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "strict_types should not affect nullable null return, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: return from namespace-wrapped functions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_namespaced_function() {
    let php = r#"<?php
namespace App\Utils;

function format_name(): string {
    return "formatted";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct return in namespaced function, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_return_in_namespaced_function() {
    let php = r#"<?php
namespace App\Utils;

function format_name(): string {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for array returned from namespaced string function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_correct_namespaced_class_method() {
    let php = r#"<?php
namespace App\Services;

class UserService {
    public function getName(): string {
        return "name";
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct return in namespaced class method, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: multiple classes / methods in same file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn correct_returns_across_multiple_classes() {
    let php = r#"<?php
class Foo {
    public function name(): string {
        return "foo";
    }
}

class Bar {
    public function count(): int {
        return 42;
    }
}

class Baz {
    public function flag(): bool {
        return true;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag any correct returns across multiple classes, got: {diags:?}"
    );
}

#[test]
fn only_wrong_class_flagged_among_multiple() {
    let php = r#"<?php
class Good {
    public function name(): string {
        return "good";
    }
}

class Bad {
    public function count(): int {
        return "not a number";
    }
}

class AlsoGood {
    public function flag(): bool {
        return true;
    }
}
"#;
    let diags = collect(php);
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one return error (in Bad class), got: {msgs:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: bool / true / false return types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_true_returned_from_bool() {
    let php = r#"<?php
function is_valid(): bool {
    return true;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag true returned from bool function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_false_returned_from_bool() {
    let php = r#"<?php
function is_valid(): bool {
    return false;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag false returned from bool function, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: string concatenation / expressions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_string_concat_returned_from_string() {
    let php = r#"<?php
function greet(string $name): string {
    return "Hello, " . $name;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag string concatenation returned from string function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_arithmetic_returned_from_int() {
    let php = r#"<?php
function add(int $a, int $b): int {
    return $a + $b;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag arithmetic returned from int function, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: return in declare block
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_return_in_declare_block() {
    let php = r#"<?php
declare(strict_types=1) {
    function get_name(): string {
        return "hello";
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct return inside declare block, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: ternary and null coalescing in return
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_ternary_return_string() {
    let php = r#"<?php
function pick(bool $flag): string {
    return $flag ? "yes" : "no";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag ternary returning strings from string function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_null_coalescing_return() {
    let php = r#"<?php
function get_name(?string $name): string {
    return $name ?? "default";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag null coalescing returning string from string function, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: multiple methods with different return types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_multiple_methods_correct_returns() {
    let php = r#"<?php
class UserService {
    public function getName(): string {
        return "Alice";
    }

    public function getAge(): int {
        return 30;
    }

    public function isActive(): bool {
        return true;
    }

    public function getItems(): array {
        return [];
    }

    public function process(): void {
        return;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag any correct returns across multiple methods, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: constructor returning void
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_constructor_with_early_return() {
    let php = r#"<?php
class Initializer {
    public string $name;

    public function __construct(string $name) {
        if ($name === '') {
            $this->name = 'default';
            return;
        }
        $this->name = $name;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag bare return in constructor, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: returning typed parameters directly
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_returning_typed_parameter() {
    let php = r#"<?php
function identity(string $s): string {
    return $s;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag returning typed parameter matching return type, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_returning_nullable_param_from_nullable() {
    let php = r#"<?php
function passthrough(?int $val): ?int {
    return $val;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag returning ?int param from ?int function, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: mixed use of functions and classes in one file
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_mixed_functions_and_classes_correct() {
    let php = r#"<?php
function helper(): int {
    return 42;
}

class Widget {
    public function render(): string {
        return "html";
    }
}

function another_helper(): bool {
    return false;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag any correct returns in mixed file, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// False-positive tests: return from match-like switch
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_returns_in_switch_cases() {
    let php = r#"<?php
function status_label(int $code): string {
    switch ($code) {
        case 200:
            return "OK";
        case 404:
            return "Not Found";
        case 500:
            return "Server Error";
        default:
            return "Unknown";
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct returns in switch cases, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case: empty array literal type resolution
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_empty_array_returned_from_array() {
    let php = r#"<?php
function empty_list(): array {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag empty array returned from array function, got: {diags:?}"
    );
}

#[test]
fn flags_empty_array_returned_from_int() {
    let php = r#"<?php
function oops(): int {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for empty array returned from int function, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case: nullable return with nullable param (flow-through)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_nullable_param_returned_from_non_nullable() {
    // Developer may have null-checked before returning — MAYBE, suppress.
    let php = r#"<?php
function unwrap(?string $s): string {
    if ($s === null) {
        return "default";
    }
    return $s;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag guarded nullable param return, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case: returning literal null from nullable
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_null_literal_from_nullable_int() {
    let php = r#"<?php
function maybe_count(): ?int {
    return null;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag null literal from ?int, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_int_literal_from_nullable_int() {
    let php = r#"<?php
function maybe_count(): ?int {
    return 42;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag int literal from ?int, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case: mixed return type (should be skipped)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_array_from_mixed() {
    let php = r#"<?php
function anything(): mixed {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag array returned from mixed, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_null_from_mixed() {
    let php = r#"<?php
function anything(): mixed {
    return null;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag null returned from mixed, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case: multiple return statements where all are correct
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_many_correct_returns() {
    let php = r#"<?php
function categorize(int $n): string {
    if ($n > 1000) { return "huge"; }
    if ($n > 100) { return "large"; }
    if ($n > 10) { return "medium"; }
    if ($n > 0) { return "small"; }
    if ($n === 0) { return "zero"; }
    return "negative";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag any of the many correct string returns, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case: function with no return statement and non-void type
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_no_return_statement() {
    // Functions that throw or loop forever might have no return.
    // We only check explicit return statements, not missing returns.
    let php = r#"<?php
function will_throw(): string {
    throw new \RuntimeException("oops");
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag function with no return statement (throws), got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge case: returning from catch block
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_returns_in_catch() {
    let php = r#"<?php
function safe_parse(string $json): string {
    try {
        return "parsed";
    } catch (\InvalidArgumentException $e) {
        return "invalid";
    } catch (\RuntimeException $e) {
        return "runtime";
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct returns in multiple catch blocks, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Genuine errors: various real mismatches that SHOULD be flagged
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flags_object_returned_from_int() {
    let php = r#"<?php
class Foo {}

function count_items(): int {
    return new Foo();
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for object returned from int function, got: {diags:?}"
    );
}

#[test]
fn flags_null_from_non_nullable_int() {
    let php = r#"<?php
function get_count(): int {
    return null;
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for null returned from int, got: {diags:?}"
    );
}

#[test]
fn flags_bool_returned_from_string() {
    let php = r#"<?php
function get_label(): string {
    return false;
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for bool returned from string, got: {diags:?}"
    );
}

#[test]
fn flags_string_returned_from_bool() {
    let php = r#"<?php
function check(): bool {
    return "yes";
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for string returned from bool, got: {diags:?}"
    );
}

#[test]
fn flags_array_returned_from_int() {
    let php = r#"<?php
function compute(): int {
    return [1, 2, 3];
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for array returned from int, got: {diags:?}"
    );
}

#[test]
fn flags_array_returned_from_bool() {
    let php = r#"<?php
function validate(): bool {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for array returned from bool, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: intersection types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_intersection_return_type_with_implementing_class() {
    let php = r#"<?php
interface Countable {
    public function count(): int;
}
interface Serializable {
    public function serialize(): string;
}
class Collection implements Countable, Serializable {
    public function count(): int { return 0; }
    public function serialize(): string { return ''; }
}

function get_collection(): Countable&Serializable {
    return new Collection();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag class implementing both interfaces for intersection return, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: PHPDoc @return annotations vs native return types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_phpdoc_array_return_type() {
    let php = r#"<?php
class Repository {
    /** @return array<string, int> */
    public function getCounts(): array {
        return ['a' => 1, 'b' => 2];
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag array literal from method with @return array<string, int>, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_phpdoc_collection_return() {
    let php = r#"<?php
/**
 * @template T
 */
class Collection {
    /** @return T[] */
    public function all(): array { return []; }
}

class UserRepo {
    /** @return Collection<User> */
    public function getUsers(): Collection {
        return new Collection();
    }
}

class User {}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag Collection returned from Collection-typed method, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: generic / typed array return types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_array_literal_from_typed_array_return() {
    // The function returns array but the PHPDoc says int[].
    // An empty array literal should be fine.
    let php = r#"<?php
/** @return int[] */
function get_ids(): array {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag empty array from int[] return type, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_array_literal_from_list_return() {
    let php = r#"<?php
/** @return list<string> */
function get_names(): array {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag empty array from list<string> return type, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_array_from_generic_array_return() {
    let php = r#"<?php
/** @return array<string, mixed> */
function get_config(): array {
    return ['key' => 'value'];
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag array literal from array<string, mixed>, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: nullable union with class types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_class_returned_from_nullable_class() {
    let php = r#"<?php
class User {}

function find_user(): ?User {
    return new User();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag User returned from ?User, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_null_returned_from_nullable_class() {
    let php = r#"<?php
class User {}

function find_user(): ?User {
    return null;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag null returned from ?User, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_subclass_returned_from_nullable_parent() {
    let php = r#"<?php
class Vehicle {}
class Car extends Vehicle {}

function find_vehicle(): ?Vehicle {
    return new Car();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag subclass Car returned from ?Vehicle, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: complex union return types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_false_returned_from_string_or_false() {
    let php = r#"<?php
function maybe_find(): string|false {
    return false;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag false returned from string|false, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_string_returned_from_string_or_false() {
    let php = r#"<?php
function maybe_find(): string|false {
    return "found";
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag string returned from string|false, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_class_in_multi_class_union() {
    let php = r#"<?php
class Success {}
class Failure {}
class Pending {}

function get_result(): Success|Failure|Pending {
    return new Pending();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag Pending returned from Success|Failure|Pending, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: array shapes in return types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_array_returned_from_array_shape_return() {
    let php = r#"<?php
/** @return array{name: string, age: int} */
function get_user(): array {
    return ['name' => 'Alice', 'age' => 30];
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag matching array shape return, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: real-world patterns with generics and inheritance
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_builder_pattern_fluent_returns() {
    let php = r#"<?php
class QueryBuilder {
    public function select(string $col): self {
        return $this;
    }

    public function where(string $col, string $op, string $val): self {
        return $this;
    }

    public function orderBy(string $col, string $dir = 'asc'): self {
        return $this;
    }

    public function limit(int $n): self {
        return $this;
    }

    public function get(): array {
        return [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag any returns in builder pattern, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_factory_method_returning_subclass() {
    let php = r#"<?php
abstract class Shape {
    abstract public function area(): float;

    public static function circle(float $r): self {
        return new Circle($r);
    }

    public static function square(float $s): self {
        return new Square($s);
    }
}

class Circle extends Shape {
    public function __construct(private float $r) {}
    public function area(): float { return 3.14 * $this->r * $this->r; }
}

class Square extends Shape {
    public function __construct(private float $s) {}
    public function area(): float { return $this->s * $this->s; }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag factory methods returning subclasses of self, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_repository_pattern_nullable_return() {
    let php = r#"<?php
class User {}

class UserRepository {
    public function find(int $id): ?User {
        if ($id <= 0) {
            return null;
        }
        return new User();
    }

    public function findOrFail(int $id): User {
        return new User();
    }

    public function all(): array {
        return [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag any returns in repository pattern, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_enum_method_returning_string_from_match() {
    // Enum methods commonly use match() which returns different types
    // per arm. All arms here return strings.
    let php = r#"<?php
enum Status {
    case Active;
    case Inactive;
    case Pending;

    public function label(): string {
        return match($this) {
            self::Active => 'Active',
            self::Inactive => 'Inactive',
            self::Pending => 'Pending',
        };
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag match expression returning strings from string method, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: method with multiple nullable/union return paths
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_complex_conditional_nullable_returns() {
    let php = r#"<?php
class Parser {
    public function parse(string $input): ?int {
        if ($input === '') {
            return null;
        }
        if ($input === 'zero') {
            return 0;
        }
        if ($input === 'one') {
            return 1;
        }
        return null;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag any returns in complex nullable method, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: return with cast expressions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_cast_to_matching_return_type() {
    let php = r#"<?php
function to_int(string $s): int {
    return (int) $s;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag (int) cast returned from int function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_string_cast_return() {
    let php = r#"<?php
function stringify(mixed $v): string {
    return (string) $v;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag (string) cast returned from string function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_array_cast_return() {
    let php = r#"<?php
function to_array(object $o): array {
    return (array) $o;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag (array) cast returned from array function, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: complex real-world class hierarchies
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_exception_subclass_return() {
    let php = r#"<?php
class AppException extends \RuntimeException {}
class ValidationException extends AppException {}

function make_error(): \RuntimeException {
    return new ValidationException("bad input");
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag deep exception subclass returned as RuntimeException, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: returning from static methods
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_static_method_return() {
    let php = r#"<?php
class Config {
    public static function getDefault(): string {
        return "default_value";
    }

    public static function getCount(): int {
        return 42;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct static method returns, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_static_method_return() {
    let php = r#"<?php
class Config {
    public static function getDefault(): string {
        return [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for array returned from static string method, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: returning method call results
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_returning_method_call_matching_type() {
    let php = r#"<?php
class Helper {
    public function getName(): string {
        return "name";
    }
}

class Service {
    private Helper $helper;

    public function getLabel(): string {
        return $this->helper->getName();
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag method call return matching declared type, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: returning from private/protected methods
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_correct_private_method_return() {
    let php = r#"<?php
class Internal {
    private function secret(): string {
        return "hidden";
    }

    protected function guarded(): int {
        return 42;
    }

    public function exposed(): bool {
        return true;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct private/protected method returns, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_private_method_return() {
    let php = r#"<?php
class Internal {
    private function secret(): string {
        return [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected error for array returned from private string method, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: complex nested closures and generators combined
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_method_with_closures_and_correct_return() {
    let php = r#"<?php
class Transformer {
    public function transform(array $items): array {
        $mapper = function(string $item): string {
            return strtoupper($item);
        };
        $filter = fn(string $s): bool => $s !== '';
        return [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag method with internal closures returning correct type, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: PHP 8.1+ enum backed values
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_backed_enum_method_correct_return() {
    let php = r#"<?php
enum Suit: string {
    case Hearts = 'H';
    case Diamonds = 'D';
    case Clubs = 'C';
    case Spades = 'S';

    public function color(): string {
        return match($this) {
            self::Hearts, self::Diamonds => 'red',
            self::Clubs, self::Spades => 'black',
        };
    }

    public function isRed(): bool {
        return $this === self::Hearts || $this === self::Diamonds;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag correct returns in backed enum methods, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: never return type (should have no returns at all)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_never_function_that_throws() {
    let php = r#"<?php
function fail(): never {
    throw new \RuntimeException("fatal");
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag never function with no return, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Advanced: mixed nullable and union returns across complex method
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_complex_service_class() {
    let php = r#"<?php
class OrderService {
    public function findOrder(int $id): ?array {
        if ($id <= 0) {
            return null;
        }
        return ['id' => $id, 'total' => 100];
    }

    public function getStatus(int $id): string|int {
        if ($id <= 0) {
            return "unknown";
        }
        return $id;
    }

    public function process(int $id): bool {
        if ($id <= 0) {
            return false;
        }
        return true;
    }

    public function getTotal(int $id): float {
        return 99.99;
    }

    public function cancel(int $id): void {
        return;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Should not flag any returns in complex service class, got: {diags:?}"
    );
}

// ─── Conditional return type narrows over a broad native union ──────────────

#[test]
fn conditional_return_over_union_narrows_array_literal_no_error() {
    // A method whose PHPStan conditional `@return` narrows a literal-array
    // argument to `array<static>` should be evaluated at the call site, and
    // the narrowed branch must supersede the method's broad native union
    // return type (which would otherwise drop the `array` member).  Mirrors
    // Spatie LaravelData's `Data::collect()`, where the conditional lives on
    // an interface while the concrete method comes from a trait.
    let php = r#"<?php
/**
 * @template TKey of array-key
 */
interface BaseDataContract {
    /**
     * @return ($into is 'array' ? array<TKey, static> : ($items is array ? array<TKey, static> : DataCollection))
     */
    public static function collect(mixed $items, ?string $into = null): array|DataCollection|Enumerable|Collection;
}

trait BaseDataTrait {
    public static function collect(mixed $items, ?string $into = null): array|DataCollection|Enumerable|Collection {
        return [];
    }
}

class Data implements BaseDataContract {
    use BaseDataTrait;
}

class AccordionData extends Data {}

class DataCollection {}
class Enumerable {}
class Collection {}

class Factory {
    /** @return array<AccordionData> */
    public static function make(): array {
        return AccordionData::collect([1, 2, 3]);
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "collect([...]) narrowed via conditional should satisfy array<AccordionData>, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn conditional_return_over_union_on_instance_call_no_error() {
    // The same authoritative-conditional behaviour must apply to instance
    // method calls, not just static ones: `$factory->build([...])` whose
    // conditional narrows a literal array to `list<Widget>` should supersede
    // the method's broad native union return type.
    let php = r#"<?php
class Widget {}
class WidgetCollection {}
class WidgetBag {}

class Factory {
    /**
     * @return ($items is array ? list<Widget> : WidgetCollection)
     */
    public function build(mixed $items): array|WidgetCollection|WidgetBag {
        return [];
    }
}

class Consumer {
    /** @return list<Widget> */
    public function run(Factory $factory): array {
        return $factory->build([1, 2, 3]);
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "instance build([...]) narrowed via conditional should satisfy list<Widget>, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

// ─── Laravel now()/today() resolve to the concrete Carbon class ─────────────

/// Shared Carbon-family scaffolding: the concrete `Illuminate\Support\Carbon`
/// extends `Carbon\Carbon`, which extends the built-in `\DateTime`, plus the
/// `now()`/`today()` helpers declared to return the looser `CarbonInterface`.
const CARBON_SCAFFOLD: &str = r#"
namespace {
    interface DateTimeInterface {}
    class DateTime implements DateTimeInterface {}
}
namespace Carbon {
    interface CarbonInterface extends \DateTimeInterface {}
    class Carbon extends \DateTime implements CarbonInterface {
        public function addHours(int $value = 1): static { return $this; }
    }
}
namespace Illuminate\Support {
    class Carbon extends \Carbon\Carbon {}
}
namespace {
    function now($tz = null): \Carbon\CarbonInterface { return new \Illuminate\Support\Carbon(); }
    function today($tz = null): \Carbon\CarbonInterface { return new \Illuminate\Support\Carbon(); }
}
"#;

#[test]
fn now_and_today_satisfy_datetime_return() {
    let php = format!(
        "<?php\n{CARBON_SCAFFOLD}\nnamespace App {{\n    class Job {{\n        public function a(): \\DateTime {{ return now(); }}\n        public function b(): \\DateTime {{ return today(); }}\n    }}\n}}\n"
    );
    let diags = collect(&php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "now()/today() resolve to Illuminate\\Support\\Carbon (a \\DateTime), so returning them \
         from a :DateTime method must not flag, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn now_chain_narrows_to_concrete_carbon() {
    // now() resolves to the concrete class, so a fluent `$this`-returning
    // method chained off it stays concrete rather than degrading to the
    // interface — `now()->addHours(1)` is still a `\DateTime`.
    let php = format!(
        "<?php\n{CARBON_SCAFFOLD}\nnamespace App {{\n    class Job {{\n        public function retryUntil(): \\DateTime {{ return now()->addHours(1); }}\n    }}\n}}\n"
    );
    let diags = collect(&php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "now()->addHours(1) should resolve to the concrete Carbon (a \\DateTime), got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

// ─── Conditional return type intersecting a template param with a fixed
//     interface (Laravel's `TestCase::mock()`/`partialMock()`/`spy()`) ──────

#[test]
fn mock_conditional_return_preserves_interface_intersection() {
    // Mirrors Laravel's `InteractsWithContainer::mock()`:
    // `@return ($abstract is class-string<TInstance> ? TInstance&\Mockery\MockInterface : \Mockery\MockInterface)`.
    // The template param (`TInstance`) must be substituted with the
    // resolved class while keeping the `&MockInterface` intersection,
    // not collapsed down to the template alone.
    let php = r#"<?php
namespace Mockery {
    interface MockInterface {}
}

namespace App {
    class Client {}

    trait InteractsWithContainer
    {
        /**
         * @template TInstance of object
         *
         * @param  string|class-string<TInstance>  $abstract
         * @return ($abstract is class-string<TInstance> ? TInstance&\Mockery\MockInterface : \Mockery\MockInterface)
         */
        protected function mock($abstract) {}
    }

    class TestCase
    {
        use InteractsWithContainer;
    }

    class MyTest extends TestCase
    {
        private function mockClient(): Client&\Mockery\MockInterface
        {
            return $this->mock(Client::class);
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "$this->mock(Client::class) should resolve to Client&MockInterface, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn mock_conditional_return_preserves_intersection_through_a_trait_this() {
    // Same shape as `mock_conditional_return_preserves_interface_intersection`,
    // but `mock()` is called from *inside a trait* rather than directly on
    // the test class.  `$this` there stands for whatever uses the trait, so
    // it resolves to several classes at once (the trait itself, the using
    // class, and its ancestor chain, all tagged as one intersection) instead
    // of the single owner the other test exercises.  A conditional return
    // type carries no plain `return_type` at all (only `conditional_return`),
    // so the first of those several owners whose merged method is found
    // must not lock in an empty hint before a later owner gets a chance to
    // resolve the conditional.
    let php = r#"<?php
namespace Mockery {
    interface MockInterface {}
}

namespace App {
    class Client {}

    trait InteractsWithContainer
    {
        /**
         * @template TInstance of object
         *
         * @param  string|class-string<TInstance>  $abstract
         * @return ($abstract is class-string<TInstance> ? TInstance&\Mockery\MockInterface : \Mockery\MockInterface)
         */
        protected function mock($abstract) {}
    }

    class BaseTestCase
    {
        use InteractsWithContainer;
    }

    trait MocksClient
    {
        protected function mockClient(): Client&\Mockery\MockInterface
        {
            $clientMock = $this->mock(Client::class);

            return $clientMock;
        }
    }

    class MyTest extends BaseTestCase
    {
        use MocksClient;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "$this->mock(Client::class) inside a trait should resolve to Client&MockInterface, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn mock_helpers_survive_an_imprecise_method_tag_on_the_test_base() {
    // A test class that also carries `@method MockInterface mock()` in
    // its own docblock.  The tag is a lie: `mock()` exists for real on
    // the inherited trait, so PHP dispatches straight to it and never
    // reaches `__call()`.  Honouring the tag over the real method threw
    // away the framework's precise conditional return and reported the
    // mock as a bare `MockInterface`.
    let php = r#"<?php
namespace Mockery {
    interface LegacyMockInterface {}
    interface MockInterface extends LegacyMockInterface {}
}

namespace App {
    class Client {}

    trait InteractsWithContainer
    {
        /**
         * @template TInstance of object
         *
         * @param  string  $abstract
         * @param  TInstance  $instance
         * @return TInstance
         */
        protected function instance($abstract, $instance) {}

        /**
         * @template TInstance of object
         *
         * @param  string|class-string<TInstance>  $abstract
         * @return ($abstract is class-string<TInstance> ? TInstance&\Mockery\MockInterface : \Mockery\MockInterface)
         */
        protected function mock($abstract) {}

        /**
         * @template TInstance of object
         *
         * @param  string|class-string<TInstance>  $abstract
         * @return ($abstract is class-string<TInstance> ? TInstance&\Mockery\MockInterface : \Mockery\MockInterface)
         */
        protected function partialMock($abstract) {}

        /**
         * @template TInstance of object
         *
         * @param  string|class-string<TInstance>  $abstract
         * @return ($abstract is class-string<TInstance> ? TInstance&\Mockery\MockInterface : \Mockery\MockInterface)
         */
        protected function spy($abstract) {}
    }

    class TestCase
    {
        use InteractsWithContainer;
    }

    /**
     * @method \Mockery\MockInterface mock(string $abstract, callable():mixed $mockDefinition = null)
     */
    class MyTest extends TestCase
    {
        private function mockClient(): Client&\Mockery\MockInterface
        {
            $mock = $this->mock(Client::class);

            return $mock;
        }

        private function partialMockClient(): Client&\Mockery\MockInterface
        {
            $mock = $this->partialMock(Client::class);

            return $mock;
        }

        private function spyClient(): Client&\Mockery\MockInterface
        {
            $mock = $this->spy(Client::class);

            return $mock;
        }

        private function instanceClient(): Client
        {
            $instance = $this->instance(Client::class, new Client());

            return $instance;
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "a `@method` tag must not shadow the real inherited helper, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn mock_conditional_return_survives_a_local_variable() {
    // Same helper, but the call result is parked in a local first.  The
    // stored variable type has to keep the intersection, not decay to the
    // bare interface.
    let php = r#"<?php
namespace Mockery {
    interface MockInterface {}
}

namespace App {
    class Client {}

    trait InteractsWithContainer
    {
        /**
         * @template TInstance of object
         *
         * @param  string|class-string<TInstance>  $abstract
         * @return ($abstract is class-string<TInstance> ? TInstance&\Mockery\MockInterface : \Mockery\MockInterface)
         */
        protected function mock($abstract) {}

        /**
         * @template TInstance of object
         *
         * @param  string|class-string<TInstance>  $abstract
         * @return ($abstract is class-string<TInstance> ? TInstance&\Mockery\MockInterface : \Mockery\MockInterface)
         */
        protected function partialMock($abstract) {}

        /**
         * @template TInstance of object
         *
         * @param  string|class-string<TInstance>  $abstract
         * @return ($abstract is class-string<TInstance> ? TInstance&\Mockery\MockInterface : \Mockery\MockInterface)
         */
        protected function spy($abstract) {}

        /**
         * @template TInstance of object
         *
         * @param  string  $abstract
         * @param  TInstance  $instance
         * @return TInstance
         */
        protected function instance($abstract, $instance) {}
    }

    class TestCase
    {
        use InteractsWithContainer;
    }

    class MyTest extends TestCase
    {
        private function mockClient(): Client&\Mockery\MockInterface
        {
            $mock = $this->mock(Client::class);

            return $mock;
        }

        private function partialMockClient(): Client&\Mockery\MockInterface
        {
            $mock = $this->partialMock(Client::class);

            return $mock;
        }

        private function spyClient(): Client&\Mockery\MockInterface
        {
            $mock = $this->spy(Client::class);

            return $mock;
        }

        private function instanceClient(): Client
        {
            $instance = $this->instance(Client::class, new Client());

            return $instance;
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "a mock parked in a local should keep the intersection, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

// ─── Eloquent custom collections and the view() helper ──────────────────────

/// Eloquent scaffolding with a model that declares a custom collection and a
/// self-referential `hasMany`, mirroring what real Laravel models look like:
/// the collection class lives in the model's own namespace, so it is named
/// without an import.
const ELOQUENT_COLLECTION_SCAFFOLD: &str = r#"
namespace Illuminate\Database\Eloquent {
    /** @template TCollection */
    trait HasCollection {}
    abstract class Model {
        /** @return \Illuminate\Database\Eloquent\Builder<static> */
        public static function query() {}
        /** @return \Illuminate\Database\Eloquent\Relations\HasMany<static, $this> */
        protected function hasMany(string $related, string $foreign = '', string $local = '') {}
    }
    /** @template TModel of \Illuminate\Database\Eloquent\Model */
    class Builder {
        /** @return $this */
        public function where($column, $value = null) {}
        /** @return \Illuminate\Database\Eloquent\Collection<int, TModel> */
        public function get() {}
    }
    /**
     * @template TKey of array-key
     * @template TModel
     */
    class Collection {
        /** @return TModel|null */
        public function first(): mixed {}
    }
}
namespace Illuminate\Database\Eloquent\Relations {
    /**
     * @template TRelated of \Illuminate\Database\Eloquent\Model
     * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
     */
    class HasMany {
        /** @return \Illuminate\Database\Eloquent\Collection<int, TRelated> */
        public function get() {}
    }
}
namespace App\Models {
    use Illuminate\Database\Eloquent\Collection;
    use Illuminate\Database\Eloquent\HasCollection;
    use Illuminate\Database\Eloquent\Model;
    use Illuminate\Database\Eloquent\Relations\HasMany;

    /** @extends Collection<int, Order> */
    final class OrderCollection extends Collection {}

    class Order extends Model {
        /** @use HasCollection<OrderCollection> */
        use HasCollection;

        /** @return HasMany<self, $this> */
        public function children(): HasMany
        {
            return $this->hasMany(self::class, 'parent_id', 'id');
        }
    }
}
"#;

#[test]
fn chained_builder_get_returns_the_models_custom_collection() {
    // `Builder::get()` is annotated `Collection<int, TModel>`; substituting
    // TModel is not enough, because the model builds an `OrderCollection`.
    let php = format!(
        "<?php\n{ELOQUENT_COLLECTION_SCAFFOLD}\nnamespace App {{\n    use App\\Models\\Order;\n    use App\\Models\\OrderCollection;\n    class Repo {{\n        public function all(): OrderCollection {{ return Order::where('paid', true)->get(); }}\n    }}\n}}\n"
    );
    let diags = collect(&php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "chained get() should resolve to OrderCollection, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn self_referential_relation_property_uses_the_custom_collection() {
    // `HasMany<self, $this>` names the owning model, so the relation
    // property is an `OrderCollection`, not the base collection.
    let php = format!(
        "<?php\n{ELOQUENT_COLLECTION_SCAFFOLD}\nnamespace App {{\n    use App\\Models\\Order;\n    use App\\Models\\OrderCollection;\n    class Repo {{\n        public function kids(Order $order): OrderCollection {{ return $order->children; }}\n    }}\n}}\n"
    );
    let diags = collect(&php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "a self-referential relation should resolve to OrderCollection, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn relation_get_returns_the_related_models_custom_collection() {
    let php = format!(
        "<?php\n{ELOQUENT_COLLECTION_SCAFFOLD}\nnamespace App {{\n    use App\\Models\\Order;\n    use App\\Models\\OrderCollection;\n    class Repo {{\n        public function kids(Order $order): OrderCollection {{ return $order->children()->get(); }}\n    }}\n}}\n"
    );
    let diags = collect(&php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "relation get() should resolve to OrderCollection, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn base_collection_is_flagged_where_a_custom_collection_is_declared() {
    // The precision fix must not turn into a blanket pass: a plain
    // `Collection` really is incompatible with `OrderCollection`.
    let php = format!(
        "<?php\n{ELOQUENT_COLLECTION_SCAFFOLD}\nnamespace App {{\n    use Illuminate\\Database\\Eloquent\\Collection;\n    use App\\Models\\OrderCollection;\n    class Repo {{\n        public function all(Collection $rows): OrderCollection {{ return $rows; }}\n    }}\n}}\n"
    );
    let diags = collect(&php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "the base Collection is not an OrderCollection and should be flagged"
    );
}

/// Laravel's `view()` helper is declared to return the *contract*, but the
/// view factory always builds the concrete `Illuminate\View\View`.
const VIEW_SCAFFOLD: &str = r#"
namespace Illuminate\Contracts\View {
    interface View {}
    interface Factory {}
}
namespace Illuminate\View {
    class View implements \Illuminate\Contracts\View\View {}
    class Component {}
}
namespace {
    /** @return ($view is null ? \Illuminate\Contracts\View\Factory : \Illuminate\Contracts\View\View) */
    function view($view = null, $data = [], $mergeData = []) {}
}
"#;

#[test]
fn view_helper_satisfies_a_concrete_view_return() {
    // Every Blade component declares `render(): View` against the concrete
    // class, which is what `view('name')` actually hands back.
    let php = format!(
        "<?php\n{VIEW_SCAFFOLD}\nnamespace App {{\n    use Illuminate\\View\\Component;\n    use Illuminate\\View\\View;\n    class Card extends Component {{\n        public function render(): View {{ return view('components.card', ['a' => 1]); }}\n    }}\n}}\n"
    );
    let diags = collect(&php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "view('name') should resolve to the concrete Illuminate\\View\\View, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn argument_less_view_helper_still_resolves_to_the_factory() {
    let php = format!(
        "<?php\n{VIEW_SCAFFOLD}\nnamespace App {{\n    use Illuminate\\Contracts\\View\\Factory;\n    class Views {{\n        public function factory(): Factory {{ return view(); }}\n    }}\n}}\n"
    );
    let diags = collect(&php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "view() with no arguments is the factory, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn scalar_conditional_function_return_survives_template_binding() {
    // A templated wrapper binds its return from the shared text-based call
    // resolver.  The selected `string` branch must not be replaced by the
    // loadable class from the helper's broader native union.
    let php = r#"<?php
interface UrlGenerator {}

/**
 * @return ($path is null ? UrlGenerator : string)
 */
function url(?string $path = null, mixed $parameters = [], ?bool $secure = null): UrlGenerator|string {}

/**
 * @template T
 * @param T $value
 * @return T
 */
function identity(mixed $value): mixed {}

function loginUrl(): string {
    return identity(url('/login'));
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "url('/login') should keep the selected string branch through template binding, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

// ─── match ($x::class) arm narrowing ────────────────────────────────────────

#[test]
fn match_on_class_constant_narrows_the_subject_in_each_arm() {
    // `match ($v::class)` proves which class the subject is, so an arm
    // returning it satisfies a union of exactly those classes.
    let php = r#"<?php
class Base {}
class A extends Base {}
class B extends Base {}
class C extends Base {}
class Runner {
    public function run(Base $v): A|B {
        return match ($v::class) {
            A::class,
            B::class => $v,
            default => throw new \Exception('unexpected'),
        };
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "match ($$v::class) arms should narrow the subject, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn match_on_class_constant_still_flags_an_arm_outside_the_declared_union() {
    let php = r#"<?php
class Base {}
class A extends Base {}
class B extends Base {}
class C extends Base {}
class Runner {
    public function run(Base $v): A|B {
        return match ($v::class) {
            C::class => $v,
            default => throw new \Exception('unexpected'),
        };
    }
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "an arm narrowed to C cannot satisfy A|B"
    );
}

// ─── Lazy initialisation inside a guarded `if` ──────────────────────────────
//
// Property paths resolve through the forward walker's scope snapshots,
// which only the full slow-diagnostic pass builds — so these go through
// `collect_slow_diagnostics` rather than the bare collector `collect`
// calls.

fn collect_via_slow_pass(php: &str) -> Vec<Diagnostic> {
    collect_diagnostics_with(
        &create_test_backend(),
        php,
        Backend::collect_slow_diagnostics,
    )
}

#[test]
fn lazy_initialisation_in_guarded_if_narrows_the_property_after_the_block() {
    let php = r#"<?php
abstract class AbstractType {}
class ConcreteType extends AbstractType {}
class Context {
    public function makeConcrete(): ConcreteType { return new ConcreteType(); }
}
class Holder {
    protected ?AbstractType $instance = null;
    public function __construct(private Context $context) {}

    public function getType(): ConcreteType
    {
        if (!$this->instance instanceof ConcreteType) {
            $this->instance = $this->context->makeConcrete();
        }

        return $this->instance;
    }
}
"#;
    let diags = collect_via_slow_pass(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "both paths out of the guard give ConcreteType, got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

#[test]
fn a_property_narrowed_only_inside_a_branch_does_not_leak_past_the_block() {
    let php = r#"<?php
abstract class AbstractType {}
class ConcreteType extends AbstractType {}
class Holder {
    protected ?AbstractType $instance = null;

    public function getType(): ConcreteType
    {
        if ($this->instance instanceof ConcreteType) {
            echo 'yes';
        }

        return $this->instance;
    }
}
"#;
    let diags = collect_via_slow_pass(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "the branch proves nothing about the implicit-else path"
    );
}

#[test]
fn an_interface_name_returned_where_an_interface_string_is_declared() {
    let php = r#"<?php
namespace App;

interface SomeInterface {}

final class SomeClass implements SomeInterface {}

/** @return interface-string */
function returnsInterface() { return SomeInterface::class; }

/** @return interface-string */
function returnsClass() { return SomeClass::class; }
"#;
    let messages = messages_with_code(&collect(php), "type_mismatch_return");
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("interface-string"), "{messages:?}");
}

#[test]
fn accumulating_a_refined_int_return_type_stays_int() {
    let php = r#"<?php
/** @return int<0,max> */
function count_things(): int { return 0; }

function total(): int {
    $length = 0;
    $length += count_things();

    return $length;
}
"#;
    let messages = messages_with_code(&collect(php), "type_mismatch_return");
    assert!(messages.is_empty(), "got {messages:?}");
}

#[test]
fn accumulating_a_positive_int_return_type_stays_int() {
    let php = r#"<?php
/** @return positive-int */
function count_things(): int { return 1; }

function total(): int {
    $length = 0;
    $length += count_things();

    return $length;
}
"#;
    let messages = messages_with_code(&collect(php), "type_mismatch_return");
    assert!(messages.is_empty(), "got {messages:?}");
}

// ── key-of / value-of over a constant ───────────────────────────────────────

/// `@return key-of<CONSTANT>` holds the body to the keys the table has,
/// with no `@template` anywhere in the signature.
#[test]
fn key_of_over_a_constant_holds_the_body_to_the_tables_keys() {
    let php = r#"<?php
namespace App;

const ID_TABLE = ['immutable' => 1, 'mutable' => 'two'];

/** @return key-of<ID_TABLE> */
function goodKey() { return 'mutable'; }

/** @return key-of<ID_TABLE> */
function badKey() { return 'nope'; }
"#;
    let messages = messages_with_code(&collect(php), "type_mismatch_return");
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'nope'"), "{messages:?}");
    assert!(
        messages[0].contains("'immutable'|'mutable'"),
        "{messages:?}"
    );
}

/// And `@return value-of<CONSTANT>` to its values, whichever of them the
/// body picks.
#[test]
fn value_of_over_a_constant_holds_the_body_to_the_tables_values() {
    let php = r#"<?php
namespace App;

const ID_TABLE = ['immutable' => 1, 'mutable' => 'two'];

/** @return value-of<ID_TABLE> */
function goodInt() { return 1; }

/** @return value-of<ID_TABLE> */
function goodString() { return 'two'; }

/** @return value-of<ID_TABLE> */
function badValue() { return 3.5; }
"#;
    let messages = messages_with_code(&collect(php), "type_mismatch_return");
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("3.5"), "{messages:?}");
}

/// The same on a method, whose declared return names its own class constant.
#[test]
fn key_of_over_a_class_constant_holds_a_methods_body() {
    let php = r#"<?php
namespace App;

class Ids {
    const TABLE = ['immutable' => 1, 'mutable' => 'two'];

    /** @return key-of<Ids::TABLE> */
    public function bad() { return 'nope'; }

    /** @return key-of<self::TABLE> */
    public function good() { return 'immutable'; }
}
"#;
    let messages = messages_with_code(&collect(php), "type_mismatch_return");
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'nope'"), "{messages:?}");
}

/// A constant nobody can read leaves the operator standing, and an
/// operator that names no set of values cannot reject anything.
#[test]
fn key_of_over_an_unreadable_constant_rejects_nothing() {
    let php = r#"<?php
namespace App;

/** @return key-of<\Vendor\Config::MAP> */
function unknownKey() { return 'anything'; }
"#;
    let messages = messages_with_code(&collect(php), "type_mismatch_return");
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A conditional return type keyed on an argument's value is decided by the
/// literal passed, and by the parameter's declared default when the argument
/// is omitted, rather than reading back the union of every branch.
#[test]
fn a_conditional_return_is_decided_by_the_argument_value_and_its_default() {
    let php = r#"<?php
namespace App;

/** @return ($format is 0 ? int : list<string>) */
function words(string $text, int $format = 0) { return $format === 0 ? 1 : ['a']; }

function counted(string $text): int { return words($text); }

function listed(string $text): array { return words($text, 1); }

function countedByName(string $text): int { return words($text, format: 0); }

function badCount(string $text): int { return words($text, 1); }

function badList(string $text): array { return words($text); }
"#;
    let messages = messages_with_code(&collect(php), "type_mismatch_return");
    assert_eq!(messages.len(), 2, "got {messages:?}");
    assert!(
        messages[0].contains("list<string>") && messages[0].contains("int"),
        "{messages:?}"
    );
    assert!(
        messages[1].contains("int") && messages[1].contains("array"),
        "{messages:?}"
    );
}

/// Eloquent relation scaffolding for the `Relation::__call` forwarding
/// tests below: a `Relation` that uses `ForwardsCalls` and inherits
/// `@mixin Builder<TRelatedModel>`, plus an `Author` model carrying every
/// shape of Builder-typed member that gets injected onto a relation
/// (a trait `@method` tag, a scope, a scope returning a custom builder
/// subclass, and a `where{Column}` column).
const RELATION_FORWARDING_SCAFFOLD: &str = r#"
namespace Illuminate\Support\Traits {
    trait ForwardsCalls {}
}
namespace Illuminate\Database\Eloquent {
    abstract class Model {
        /**
         * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
         * @param class-string<TRelatedModel> $related
         * @return \Illuminate\Database\Eloquent\Relations\BelongsTo<TRelatedModel, $this>
         */
        protected function belongsTo(string $related, string $foreign = '', string $owner = '') {}
    }
    /** @template TModel of \Illuminate\Database\Eloquent\Model */
    class Builder {
        /** @return $this */
        public function where($column, $value = null) {}
        /** @return TModel|null */
        public function first() {}
    }
    /**
     * @method static \Illuminate\Database\Eloquent\Builder<static> withTrashed()
     */
    trait SoftDeletes {}
}
namespace Illuminate\Database\Eloquent\Relations {
    use Illuminate\Support\Traits\ForwardsCalls;
    /**
     * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
     * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
     * @mixin \Illuminate\Database\Eloquent\Builder<TRelatedModel>
     */
    abstract class Relation {
        use ForwardsCalls;

        /** @return mixed */
        public function __call($method, $parameters) {}
    }
    /**
     * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
     * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
     * @extends Relation<TRelatedModel, TDeclaringModel>
     */
    class BelongsTo extends Relation {}
    /**
     * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
     * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
     * @extends Relation<TRelatedModel, TDeclaringModel>
     */
    class HasMany extends Relation {}
}
namespace App\Models {
    use Illuminate\Database\Eloquent\Builder;
    use Illuminate\Database\Eloquent\Model;
    use Illuminate\Database\Eloquent\SoftDeletes;

    /**
     * @template TModel of \Illuminate\Database\Eloquent\Model
     * @extends Builder<TModel>
     */
    class AuthorBuilder extends Builder {}

    /**
     * @property string $name
     * @method static Builder<static>|null maybeTrashed()
     */
    class Author extends Model {
        use SoftDeletes;

        /**
         * @param Builder<static> $query
         * @return Builder<static>
         */
        public function scopeActive($query) { return $query; }

        /**
         * @param AuthorBuilder<static> $query
         * @return AuthorBuilder<static>
         */
        public function scopeFeatured($query) { return $query; }
    }
}
"#;

/// Wrap a `Post` method body in the relation scaffolding above.
fn relation_forwarding_php(declared_return: &str, body: &str) -> String {
    format!(
        "<?php\n{RELATION_FORWARDING_SCAFFOLD}\nnamespace App {{\n\
         \x20   use App\\Models\\Author;\n\
         \x20   use Illuminate\\Database\\Eloquent\\Model;\n\
         \x20   use Illuminate\\Database\\Eloquent\\Relations\\BelongsTo;\n\
         \x20   use Illuminate\\Database\\Eloquent\\Relations\\HasMany;\n\n\
         \x20   class Post extends Model {{\n\
         \x20       public function author(): {declared_return}\n\
         \x20       {{\n\
         \x20           return {body};\n\
         \x20       }}\n\
         \x20   }}\n}}\n"
    )
}

/// Chaining a builder method onto an Eloquent relation (e.g.
/// `$this->belongsTo(Author::class)->withTrashed()`) should not produce a
/// `type_mismatch_return` diagnostic.  At runtime, `Relation::__call`
/// uses `ForwardsCalls::forwardDecoratedCallTo`, which returns `$this`
/// (the Relation) when the forwarded method returns the builder.
///
/// Closes #354
#[test]
fn relation_chained_builder_method_returns_relation_not_builder() {
    for body in [
        // A `@method` tag inherited from a trait (SoftDeletes).
        "$this->belongsTo(Author::class)->withTrashed()",
        // A scope method.
        "$this->belongsTo(Author::class)->active()",
        // A scope method returning a custom builder subclass, which
        // forwards the same way as the base Builder.
        "$this->belongsTo(Author::class)->featured()",
        // A synthesized `where{Column}` method.
        "$this->belongsTo(Author::class)->whereName('a')",
        // Two forwarded calls in a row.
        "$this->belongsTo(Author::class)->withTrashed()->active()",
    ] {
        let diags = collect(&relation_forwarding_php("BelongsTo", body));
        assert!(
            messages_with_code(&diags, "type_mismatch_return").is_empty(),
            "`{body}` should return BelongsTo, not a Builder; got: {}",
            messages_with_code(&diags, "type_mismatch_return").join("; ")
        );
    }
}

/// The relation type the chain lands on is still checked: a forwarded
/// call keeps the relation it was made on, so declaring a *different*
/// relation is still a mismatch.
#[test]
fn relation_chained_builder_method_still_reports_a_wrong_relation() {
    let diags = collect(&relation_forwarding_php(
        "HasMany",
        "$this->belongsTo(Author::class)->withTrashed()",
    ));
    let messages = messages_with_code(&diags, "type_mismatch_return").join("; ");
    assert!(
        messages.contains("BelongsTo") && messages.contains("HasMany"),
        "belongsTo()->withTrashed() declared as HasMany should still be reported; got: {messages}"
    );
}

/// The chain continues past a forwarded call: `->first()` on the relation
/// resolves through the inherited `@mixin Builder<Author>` to `?Author`,
/// so declaring the model satisfies it and declaring the relation does
/// not.  (That the chain resolves at all is guarded in
/// `diagnostics_unknown_members`.)
#[test]
fn relation_chain_continues_past_a_forwarded_builder_method() {
    let body = "$this->belongsTo(Author::class)->withTrashed()->first()";

    let diags = collect(&relation_forwarding_php("?Author", body));
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "belongsTo()->withTrashed()->first() should resolve to ?Author; got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );

    let diags = collect(&relation_forwarding_php("BelongsTo", body));
    let messages = messages_with_code(&diags, "type_mismatch_return").join("; ");
    assert!(
        messages.contains("App\\Models\\Author"),
        "belongsTo()->withTrashed()->first() should be reported as Author, not a relation; \
         got: {messages}"
    );
}

/// A nullable Builder-typed return keeps its nullability when it is
/// rewritten to the relation: `Builder<static>|null` forwards as
/// `BelongsTo<Author, Post>|null`, which is not a `BelongsTo`.
#[test]
fn relation_forwarded_builder_return_keeps_its_nullability() {
    let body = "$this->belongsTo(Author::class)->maybeTrashed()";

    let diags = collect(&relation_forwarding_php("BelongsTo", body));
    let messages = messages_with_code(&diags, "type_mismatch_return").join("; ");
    assert!(
        messages.contains("BelongsTo") && messages.contains("null"),
        "a nullable forwarded return should stay nullable; got: {messages}"
    );

    let diags = collect(&relation_forwarding_php("?BelongsTo", body));
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "a nullable forwarded return satisfies `?BelongsTo`; got: {}",
        messages_with_code(&diags, "type_mismatch_return").join("; ")
    );
}

/// A `$format` that isn't a literal decides nothing, so the call keeps every
/// branch it could return instead of committing to one.
#[test]
fn a_conditional_return_on_an_unknown_argument_keeps_every_branch() {
    let php = r#"<?php
namespace App;

/** @return ($format is 0 ? int : list<string>) */
function words(string $text, int $format = 0) { return $format === 0 ? 1 : ['a']; }

function counted(string $text, int $format): int { return words($text, $format); }
"#;
    let messages = messages_with_code(&collect(php), "type_mismatch_return");
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("list<string>"), "{messages:?}");
}

/// A namespaced `const` list narrows a needle through every spelling a
/// reference can use: bare, through an imported namespace, and fully
/// qualified.  The gate proves `$grade` is one of the table's literals, so
/// the `?string` the parameter allowed is gone by the `return`.
#[test]
fn a_namespaced_constant_list_narrows_through_every_spelling() {
    for haystack in [
        "GRADES",
        "Config\\GRADES",
        "\\App\\Config\\GRADES",
        "namespace\\Config\\GRADES",
    ] {
        let php = format!(
            r#"<?php
namespace App\Config;

const GRADES = ['a', 'b', 'c'];

namespace App;

use App\Config;
use const App\Config\GRADES;

function gate(?string $grade): string {{
    if (!in_array($grade, {haystack}, true)) {{
        throw new \RuntimeException('unknown grade');
    }}
    return $grade;
}}
"#
        );
        assert!(
            messages_with_code(&collect(&php), "type_mismatch_return").is_empty(),
            "`{haystack}` should narrow `$grade` to its literals; got: {}",
            messages_with_code(&collect(&php), "type_mismatch_return").join("; ")
        );
    }
}

/// The global fallback still applies: an unqualified name that the current
/// namespace does not declare means the global constant of that name.
#[test]
fn an_unqualified_constant_falls_back_to_the_global_one() {
    let php = r#"<?php
const GRADES = ['a', 'b', 'c'];

namespace App;

function gate(?string $grade): string {
    if (!in_array($grade, GRADES, true)) {
        throw new \RuntimeException('unknown grade');
    }
    return $grade;
}
"#;
    assert!(
        messages_with_code(&collect(php), "type_mismatch_return").is_empty(),
        "a bare name should fall back to the global constant; got: {}",
        messages_with_code(&collect(php), "type_mismatch_return").join("; ")
    );
}

// ── A function's `@return` docblock refines its native `array` hint ──────────

#[test]
fn flags_shape_value_mismatch_against_docblock_map_on_plain_function() {
    let php = r#"<?php
/** @return array<string, int> */
function bad(): array {
    return ['a' => 'x'];
}
"#;
    let diags = collect(php);
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert!(
        msgs.iter().any(|m| m.contains("array<string, int>")),
        "Expected the docblock map type to be checked, not the bare `array` hint, got: {msgs:?}"
    );
}

#[test]
fn flags_shape_value_mismatch_against_docblock_list_on_plain_function() {
    let php = r#"<?php
/** @return list<int> */
function bad(): array {
    return ['x', 'y'];
}
"#;
    let diags = collect(php);
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert!(
        msgs.iter().any(|m| m.contains("list<int>")),
        "Expected the docblock list type to be checked, got: {msgs:?}"
    );
}

#[test]
fn no_diagnostic_when_shape_satisfies_docblock_map_on_plain_function() {
    let php = r#"<?php
/** @return array<string, int> */
function good(): array {
    return ['a' => 1];
}

/** @return list<int> */
function goodList(): array {
    return [1, 2];
}

/** @return array<string, int> */
function goodEmpty(): array {
    return [];
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Expected no return type error, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

#[test]
fn docblock_return_type_of_another_files_function_is_not_borrowed() {
    let backend = create_test_backend();
    let other = "file:///other.php";
    backend.update_ast(
        other,
        r#"<?php
/** @return array<string, int> */
function dup(): array {
    return [];
}
"#,
    );

    let uri = "file:///test.php";
    let php = r#"<?php
function dup(): array {
    return ['a' => 'x'];
}
"#;
    backend.update_ast(uri, php);
    let mut diags = Vec::new();
    backend.collect_return_type_diagnostics(uri, php, &mut diags);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "This file declares `dup(): array` with no docblock, so the other file's \
         `@return array<string, int>` must not be checked against this body, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

// ─── B78: standalone `@var` cast above `return` ────────────────────────────

#[test]
fn a_nameless_var_docblock_above_return_casts_the_returned_expression() {
    let php = r#"<?php
function giveString(): string {
    return 'x';
}

function cast(): int
{
    /** @var int */
    return giveString();
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "The standalone `@var int` cast above `return` should make the return \
         type check as `int`, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

#[test]
fn a_nameless_var_docblock_above_return_still_flags_a_real_mismatch() {
    let php = r#"<?php
function giveString(): string {
    return 'x';
}

function cast(): array
{
    /** @var int */
    return giveString();
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "The `@var int` cast is incompatible with the declared `array` return \
         type, so this must still be flagged, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

#[test]
fn a_named_var_docblock_above_return_is_unaffected_by_the_nameless_cast_fix() {
    let php = r#"<?php
function cast(): int
{
    /** @var int $x */
    $x = giveString();
    return $x;
}

function giveString(): string {
    return 'x';
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "The named `@var int $x` form already worked before this fix and must \
         keep working, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

// ─── Magic constants as arithmetic and string operands ─────────────────────

#[test]
fn a_line_magic_constant_added_to_an_int_stays_an_int() {
    let php = r#"<?php
function getEndLineOfThisFile(): int
{
    return __LINE__ + 3;
}

function offsetFromLine(int $offset): int
{
    return __LINE__ - $offset;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "`__LINE__` is an int, so arithmetic on it stays an int, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

#[test]
fn the_string_magic_constants_satisfy_a_string_return_type() {
    let php = r#"<?php
class Widget
{
    public function file(): string { return __FILE__; }
    public function dir(): string { return __DIR__; }
    public function method(): string { return __METHOD__; }
    public function function_(): string { return __FUNCTION__; }
    public function namespace_(): string { return __NAMESPACE__; }
    public function class_(): string { return __CLASS__; }
    public function trait_(): string { return __TRAIT__; }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Every magic constant but `__LINE__` is a string, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

#[test]
fn a_line_magic_constant_still_fails_an_array_return_type() {
    let php = r#"<?php
function lines(): array
{
    return __LINE__ + 1;
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "`__LINE__ + 1` is an int, which an `array` return type rejects, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

// ─── int / int (int|float) returned from an int position ───────────────────

#[test]
fn no_strict_types_allows_int_division_returned_as_int() {
    // `int / int` resolves to `int|float`, but outside strict_types PHP
    // coerces (truncates) the float half on the way in instead of raising
    // a TypeError, so there is nothing to flag.
    let php = r#"<?php
function to_whole_days(int $length): int
{
    return $length / 86400;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "Without strict_types, int/int returned as int should be allowed, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

#[test]
fn strict_types_allows_int_division_returned_as_int() {
    // Under strict_types the float half of `int / int` is no longer
    // coerced away, but whether it appears at all depends on the operand
    // *values* rather than on their types, so the union `int / int`
    // produces is benevolent: one branch fitting the return type is
    // enough.
    let php = r#"<?php
declare(strict_types=1);

function to_whole_days(int $length): int
{
    return $length / 86400;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "int/int returned as int should be allowed under strict_types, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

/// The benevolence above belongs to the division operator, not to unions
/// at large: a declared `int|float` still has to fit the return type whole.
#[test]
fn strict_types_flags_declared_int_float_union_returned_as_int() {
    let php = r#"<?php
declare(strict_types=1);

/** @param int|float $length */
function to_whole_days($length): int
{
    return $length;
}
"#;
    let diags = collect(php);
    assert!(
        !messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "A declared int|float returned as int should be flagged, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

/// `int ** int` gets the same benevolent treatment as `int / int`: PHP
/// promotes the result to `float` on overflow or a negative exponent, a
/// property of the values rather than the operands' types.
#[test]
fn strict_types_allows_int_exponentiation_returned_as_int() {
    let php = r#"<?php
declare(strict_types=1);

function pow2(int $exp): int
{
    return 2 ** $exp;
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "int**int returned as int should be allowed under strict_types, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

/// `new self(...)` must resolve through the enclosing class's *fully
/// qualified* name.  A namespaced class whose short name collides with a
/// global one (`App\Error` vs the built-in `\Error`) otherwise has every
/// `new self(...)` misresolve to the global class.
#[test]
fn new_self_resolves_namespaced_class_over_same_named_global() {
    let php = r#"<?php
namespace {
    class Error
    {
        public function __construct(string $message = '', int $code = 0) {}
    }
}

namespace App {
    class Error
    {
        public function __construct(private string $message, private ?int $line = null) {}

        public function changeLine(?int $line): self
        {
            return new self($this->message, $line);
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "new self() inside App\\Error should resolve to App\\Error, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

/// `return $this;` inside a trait method whose declared return type is an
/// interface the trait does not itself implement.  `$this` is the class the
/// trait is mixed into, so the interface is satisfied there even though the
/// trait declaration alone cannot prove it.
#[test]
fn trait_this_return_satisfies_using_class_interface() {
    let php = r#"<?php
interface Shape
{
    public function area(): float;
}

trait ShapeTrait
{
    public function self_(): Shape
    {
        return $this;
    }
}

class Square implements Shape
{
    use ShapeTrait;

    public function area(): float
    {
        return 1.0;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "return $this in a trait should satisfy the using class's interface, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

/// `@phpstan-require-implements` states the same guarantee outright, and
/// has to reach the forward walker's `$this` and not just the chain
/// resolver's.
#[test]
fn trait_this_return_satisfies_require_implements_bound() {
    let php = r#"<?php
interface Shape
{
    public function area(): float;
}

/** @phpstan-require-implements Shape */
trait ShapeTrait
{
    public function self_(): Shape
    {
        return $this;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "@phpstan-require-implements should let `return $this` satisfy the interface, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

/// A trait no class uses says nothing about what `$this` is, and a trait
/// whose users do not all satisfy the declared return type says the
/// opposite of what the declaration claims.  Both stay flagged.
#[test]
fn trait_this_return_still_flagged_without_a_shared_bound() {
    let unused = r#"<?php
interface Shape
{
    public function area(): float;
}

trait ShapeTrait
{
    public function self_(): Shape
    {
        return $this;
    }
}
"#;
    assert!(
        !messages_with_code(&collect(unused), "type_mismatch_return").is_empty(),
        "an unused trait offers no proof that $this is a Shape"
    );

    let mixed = unused.to_string()
        + r#"
class Square implements Shape
{
    use ShapeTrait;

    public function area(): float
    {
        return 1.0;
    }
}

class Loose
{
    use ShapeTrait;
}
"#;
    assert!(
        !messages_with_code(&collect(&mixed), "type_mismatch_return").is_empty(),
        "a user that is not a Shape means $this is not guaranteed to be one, got: {:?}",
        messages_with_code(&collect(&mixed), "type_mismatch_return")
    );
}

/// A trait's own members stay reachable on `$this` once the using
/// classes' bounds are intersected in: the intersection adds to the
/// trait, it does not replace it.
#[test]
fn trait_this_keeps_trait_members_alongside_the_bound() {
    let php = r#"<?php
interface Shape
{
    public function area(): float;
}

trait ShapeTrait
{
    public function scaled(): float
    {
        return $this->area() * $this->factor();
    }

    public function factor(): float
    {
        return 2.0;
    }
}

class Square implements Shape
{
    use ShapeTrait;

    public function area(): float
    {
        return 1.0;
    }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "trait and interface members should both resolve on $this, got: {:?}",
        messages_with_code(&diags, "type_mismatch_return")
    );
}

// ─── A template parameter named like a class is still the template ─────────

#[test]
fn class_template_named_like_a_class_is_not_that_class() {
    let php = r#"<?php
namespace App;

class T1 {}

/**
 * @template T1
 */
trait Holds {
    /** @return T1 */
    public function first() { return null; }
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "An unbounded template accepts any value, even beside a class of the same name, got: {diags:?}"
    );
}

#[test]
fn class_template_is_not_a_same_named_class_in_another_namespace_block() {
    let php = r#"<?php
namespace A {
    /**
     * @template T1
     */
    trait Holds {
        /** @return T1 */
        public function first() { return null; }
    }
}
namespace B {
    trait T1 {}
}
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "The template must not resolve to another block's class, got: {diags:?}"
    );
}

#[test]
fn function_template_named_like_a_class_is_not_that_class() {
    let php = r#"<?php
namespace App;

class T2 {}

/**
 * @template T2
 * @param T2 $x
 * @return T2
 */
function pass($x) { return null; }
"#;
    let diags = collect(php);
    assert!(
        messages_with_code(&diags, "type_mismatch_return").is_empty(),
        "An unbounded function template accepts any value, got: {diags:?}"
    );
}

#[test]
fn bounded_template_return_is_checked_against_its_bound() {
    let php = r#"<?php
interface Shape {}
class Circle implements Shape {}
class Plain {}

/**
 * @template T of Shape
 */
class Holder {
    /** @return T */
    public function wrong() { return new Plain(); }

    /** @return T */
    public function withinBound() { return new Circle(); }
}
"#;
    let diags = collect(php);
    let msgs = messages_with_code(&diags, "type_mismatch_return");
    assert_eq!(
        msgs.len(),
        1,
        "Only the value outside the bound is flagged, got: {msgs:?}"
    );
    assert!(msgs[0].contains("Plain"), "got: {msgs:?}");
}

/// A Laravel model operator is a name for a class, so it has to be
/// resolved before the returned and declared types are compared — and
/// before the message is written. Leaving it until the comparison
/// reported the operator's own spelling back at the reader, who wrote a
/// type that does name something.
///
/// `static` here is bound to the trait that wrote the annotation, which
/// is no model at all, so the operator stands for the framework's own
/// collection.
#[test]
fn a_model_operator_is_resolved_before_the_return_type_is_reported() {
    let php = r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Collection;

/** @phpstan-require-extends \Illuminate\Database\Eloquent\Model */
trait HasFactory
{
    /** @return collection-of<static> */
    public static function many(): Collection
    {
        return new Collection();
    }

    /** @return static|Collection */
    public static function caller(): mixed
    {
        return static::many();
    }
}
"#;
    let messages = messages_with_code(&collect(php), "type_mismatch_return");
    assert!(
        !messages.iter().any(|m| m.contains("collection-of")),
        "the operator should be resolved, not reported as itself: {messages:?}"
    );
}
