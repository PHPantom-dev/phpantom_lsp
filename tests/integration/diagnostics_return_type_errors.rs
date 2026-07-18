use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_return_type_diagnostics(uri, php, &mut out);
    out
}

fn has_return_error(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_return"))
    })
}

fn return_error_messages(diags: &[Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(
                |c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_return"),
            )
        })
        .map(|d| d.message.clone())
        .collect()
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
        has_return_error(&diags),
        "Expected return type error for array returned from string function, got: {diags:?}"
    );
    let msgs = return_error_messages(&diags);
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
        "Expected error for value returned from void function, got: {diags:?}"
    );
    let msgs = return_error_messages(&diags);
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
        "Expected error for bare return in string function, got: {diags:?}"
    );
    let msgs = return_error_messages(&diags);
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
        has_return_error(&diags),
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
    let msgs = return_error_messages(&diags);
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
    let msgs = return_error_messages(&diags);
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
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
        has_return_error(&diags),
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
        has_return_error(&diags),
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
    let msgs = return_error_messages(&diags);
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
    let msgs = return_error_messages(&diags);
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
        has_return_error(&diags),
        "Expected error for string returned from void method, got: {diags:?}"
    );
    let msgs = return_error_messages(&diags);
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
        "Should not flag Category[] returned from self[] method, got: {:?}",
        return_error_messages(&diags)
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
        !has_return_error(&diags),
        "Should not flag Category[] returned from static[] method, got: {:?}",
        return_error_messages(&diags)
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
        !has_return_error(&diags),
        "Should not flag Node&HasCount returned where Node is declared, got: {:?}",
        return_error_messages(&diags)
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
    let msgs = return_error_messages(&diags);
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
    let msgs = return_error_messages(&diags);
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
    let msgs = return_error_messages(&diags);
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
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
        has_return_error(&diags),
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
        has_return_error(&diags),
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
        has_return_error(&diags),
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
        has_return_error(&diags),
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
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
        !has_return_error(&diags),
        "Should not flag any returns in complex service class, got: {diags:?}"
    );
}
