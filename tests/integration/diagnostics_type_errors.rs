use crate::common::{create_test_backend, create_test_backend_with_function_stubs};
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_type_error_diagnostics(uri, php, &mut out);
    out
}

fn collect_with_stubs(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend_with_function_stubs();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_type_error_diagnostics(uri, php, &mut out);
    out
}

fn has_type_error(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| {
        d.code.as_ref().is_some_and(
            |c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_argument"),
        )
    })
}

fn type_error_messages(diags: &[Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(
                |c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_argument"),
            )
        })
        .map(|d| d.message.clone())
        .collect()
}

// ─── Basic: string passed to int parameter ──────────────────────────────────

#[test]
fn flags_string_passed_to_int_param() {
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    $s = "hello";
    takes_int($s);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for string passed to int, got: {diags:?}"
    );
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.iter()
            .any(|m| m.contains("int") && m.contains("string")),
        "Expected message mentioning int and string, got: {msgs:?}"
    );
}

// ─── PHP juggling: int passed to string is accepted ─────────────────────────

#[test]
fn no_diagnostic_for_int_to_string_juggling() {
    let php = r#"<?php
function takes_string(string $x): void {}

function test(): void {
    $n = 42;
    takes_string($n);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag int passed to string (PHP type juggling), got: {diags:?}"
    );
}

// ─── Basic: null passed to non-nullable parameter ───────────────────────────

#[test]
fn flags_null_passed_to_non_nullable_param() {
    let php = r#"<?php
function takes_string(string $x): void {}

function test(): void {
    takes_string(null);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for null passed to string, got: {diags:?}"
    );
}

// ─── No diagnostic: correct types ──────────────────────────────────────────

#[test]
fn no_diagnostic_for_correct_types() {
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    $n = 42;
    takes_int($n);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag correct int argument, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_usleep_with_valid_integer_literal() {
    let php = r#"<?php
function test(): void {
    usleep(10_000);
}
"#;
    let diags = collect_with_stubs(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag integer literal within stub int range, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_string_to_string() {
    let php = r#"<?php
function takes_string(string $x): void {}

function test(): void {
    $s = "hello";
    takes_string($s);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag correct string argument, got: {diags:?}"
    );
}

// ─── No diagnostic: nullable parameter accepts null ─────────────────────────

#[test]
fn no_diagnostic_for_null_to_nullable() {
    let php = r#"<?php
function takes_nullable(?string $x): void {}

function test(): void {
    takes_nullable(null);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag null passed to nullable param, got: {diags:?}"
    );
}

// ─── No diagnostic: subclass passed to parent type ──────────────────────────

#[test]
fn no_diagnostic_for_subclass() {
    let php = r#"<?php
class Animal {}
class Cat extends Animal {}

function takes_animal(Animal $a): void {}

function test(): void {
    $cat = new Cat();
    takes_animal($cat);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag subclass Cat passed to Animal param, got: {diags:?}"
    );
}

// ─── No diagnostic: mixed parameter accepts anything ────────────────────────

#[test]
fn no_diagnostic_for_mixed_param() {
    let php = r#"<?php
function takes_mixed(mixed $x): void {}

function test(): void {
    takes_mixed(42);
    takes_mixed("hello");
    takes_mixed(null);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag arguments to mixed param, got: {diags:?}"
    );
}

// ─── No diagnostic: untyped parameter ───────────────────────────────────────

#[test]
fn no_diagnostic_for_untyped_param() {
    let php = r#"<?php
function takes_anything($x): void {}

function test(): void {
    takes_anything(42);
    takes_anything("hello");
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag arguments to untyped param, got: {diags:?}"
    );
}

// ─── No diagnostic: argument unpacking ──────────────────────────────────────

#[test]
fn no_diagnostic_for_unpacking() {
    let php = r#"<?php
function takes_ints(int $a, int $b): void {}

function test(): void {
    $args = ["hello", "world"];
    takes_ints(...$args);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag call with argument unpacking, got: {diags:?}"
    );
}

// ─── No diagnostic: unresolvable function ───────────────────────────────────

#[test]
fn no_diagnostic_for_unresolvable_function() {
    let php = r#"<?php
function test(): void {
    unknown_function(42);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag call to unresolvable function, got: {diags:?}"
    );
}

// ─── Flags: array passed to string ──────────────────────────────────────────

#[test]
fn flags_array_passed_to_string() {
    let php = r#"<?php
function takes_string(string $x): void {}

function test(): void {
    $arr = [1, 2, 3];
    takes_string($arr);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for array passed to string, got: {diags:?}"
    );
}

// ─── Flags: bool passed to string ───────────────────────────────────────────

#[test]
fn flags_bool_passed_to_string() {
    let php = r#"<?php
function takes_string(string $x): void {}

function test(): void {
    $b = true;
    takes_string($b);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for bool passed to string, got: {diags:?}"
    );
}

// ─── No diagnostic: int to float (PHP widening) ────────────────────────────

#[test]
fn no_diagnostic_for_int_to_float() {
    let php = r#"<?php
function takes_float(float $x): void {}

function test(): void {
    $n = 42;
    takes_float($n);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag int passed to float (PHP widening), got: {diags:?}"
    );
}

// ─── No diagnostic: callable param with closure ─────────────────────────────

#[test]
fn no_diagnostic_for_closure_to_callable() {
    let php = r#"<?php
function takes_callable(callable $fn): void {}

function test(): void {
    takes_callable(function() { return 1; });
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag closure passed to callable, got: {diags:?}"
    );
}

// ─── No diagnostic: object param with class instance ────────────────────────

#[test]
fn no_diagnostic_for_class_to_object() {
    let php = r#"<?php
class Foo {}

function takes_object(object $x): void {}

function test(): void {
    $f = new Foo();
    takes_object($f);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag class instance passed to object, got: {diags:?}"
    );
}

// ─── Flags: wrong class type ────────────────────────────────────────────────

#[test]
fn flags_wrong_class_type() {
    let php = r#"<?php
class Dog {}
class Cat {}

function takes_dog(Dog $d): void {}

function test(): void {
    $cat = new Cat();
    takes_dog($cat);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for Cat passed to Dog param, got: {diags:?}"
    );
}

// ─── No diagnostic: interface implementation ────────────────────────────────

#[test]
fn no_diagnostic_for_interface_impl() {
    let php = r#"<?php
interface Printable {
    public function print(): void;
}
class Report implements Printable {
    public function print(): void {}
}

function takes_printable(Printable $p): void {}

function test(): void {
    $r = new Report();
    takes_printable($r);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag interface impl passed to interface param, got: {diags:?}"
    );
}

// ─── Diagnostic has correct code and severity ───────────────────────────────

#[test]
fn diagnostic_has_correct_code_and_severity() {
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    $s = "hello";
    takes_int($s);
}
"#;
    let diags = collect(php);
    let type_diags: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(
                |c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_argument"),
            )
        })
        .collect();
    assert!(
        !type_diags.is_empty(),
        "Expected at least one type error diagnostic"
    );
    assert_eq!(
        type_diags[0].severity,
        Some(DiagnosticSeverity::ERROR),
        "Type error should be ERROR severity"
    );
    assert_eq!(
        type_diags[0].source.as_deref(),
        Some("phpantom"),
        "Source should be phpantom"
    );
}

// ─── Method calls: flags wrong type to method parameter ─────────────────────

#[test]
fn flags_wrong_type_to_method_param() {
    let php = r#"<?php
class Formatter {
    public function format(string $text): string {
        return $text;
    }
}

function test(): void {
    $f = new Formatter();
    $arr = [1, 2, 3];
    $f->format($arr);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for array passed to string method param, got: {diags:?}"
    );
}

// ─── Method calls: no diagnostic for correct type ───────────────────────────

#[test]
fn no_diagnostic_for_correct_method_arg() {
    let php = r#"<?php
class Formatter {
    public function format(string $text): string {
        return $text;
    }
}

function test(): void {
    $f = new Formatter();
    $f->format("hello");
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag correct string argument to method, got: {diags:?}"
    );
}

// ─── Static method calls ────────────────────────────────────────────────────

#[test]
fn flags_wrong_type_to_static_method() {
    let php = r#"<?php
class MathHelper {
    public static function add(int $a, int $b): int {
        return $a + $b;
    }
}

function test(): void {
    MathHelper::add("hello", "world");
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected type errors for strings passed to int static method params, got: {diags:?}"
    );
}

// ─── Constructor calls ──────────────────────────────────────────────────────

#[test]
fn flags_wrong_type_to_constructor() {
    let php = r#"<?php
class User {
    public function __construct(
        public string $name,
        public int $age,
    ) {}
}

function test(): void {
    new User(42, "not a number");
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected type errors for wrong types in constructor, got: {diags:?}"
    );
}

// ─── Multiple arguments: only wrong ones flagged ────────────────────────────

#[test]
fn flags_only_wrong_argument() {
    let php = r#"<?php
function mixed_params(int $a, string $b, float $c): void {}

function test(): void {
    $arr = [1, 2];
    mixed_params(42, $arr, 3.14);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    // Argument 2 ($b) should be flagged: array passed to string
    assert!(
        msgs.iter()
            .any(|m| m.contains("$b") && m.contains("string")),
        "Expected type error for arg 2 ($b), got: {msgs:?}"
    );
}

// ─── Union type: compatible if any branch matches ───────────────────────────

#[test]
fn no_diagnostic_for_matching_union_branch() {
    let php = r#"<?php
function takes_int_or_string(int|string $x): void {}

function test(): void {
    $s = "hello";
    takes_int_or_string($s);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string passed to int|string, got: {diags:?}"
    );
}

// ─── Literal values ─────────────────────────────────────────────────────────

#[test]
fn no_diagnostic_for_literal_int_to_int() {
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    takes_int(42);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag literal int passed to int, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_literal_string_to_string() {
    let php = r#"<?php
function takes_string(string $x): void {}

function test(): void {
    takes_string("hello");
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag literal string passed to string, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_literal_true_to_bool() {
    let php = r#"<?php
function takes_bool(bool $x): void {}

function test(): void {
    takes_bool(true);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag literal true passed to bool, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_literal_float_to_float() {
    let php = r#"<?php
function takes_float(float $x): void {}

function test(): void {
    takes_float(3.14);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag literal float passed to float, got: {diags:?}"
    );
}

// ─── No diagnostic for self/static parameters ──────────────────────────────

#[test]
fn no_diagnostic_for_self_param() {
    let php = r#"<?php
class Node {
    public function merge(self $other): void {}
}

function test(): void {
    $a = new Node();
    $b = new Node();
    $a->merge($b);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag self parameter (skipped conservatively), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_self_in_union_param() {
    let php = r#"<?php
class Decimal {
    public function add(int|self|string $value): self { return $this; }
}

function test(): void {
    $a = new Decimal();
    $b = new Decimal();
    $a->add($b);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Decimal passed to int|self|string (self in union), got: {diags:?}"
    );
}

// ─── No diagnostic: iterable param with array ───────────────────────────────

#[test]
fn no_diagnostic_for_array_to_iterable() {
    let php = r#"<?php
function takes_iterable(iterable $items): void {}

function test(): void {
    $arr = [1, 2, 3];
    takes_iterable($arr);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag array passed to iterable, got: {diags:?}"
    );
}

// ─── Message format ─────────────────────────────────────────────────────────

#[test]
fn message_mentions_param_name_and_types() {
    let php = r#"<?php
function takes_int(int $count): void {}

function test(): void {
    $s = "hello";
    takes_int($s);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(!msgs.is_empty(), "Expected at least one type error");
    let msg = &msgs[0];
    assert!(
        msg.contains("$count"),
        "Message should mention parameter name, got: {msg}"
    );
    assert!(
        msg.contains("int"),
        "Message should mention expected type, got: {msg}"
    );
    assert!(
        msg.contains("string"),
        "Message should mention actual type, got: {msg}"
    );
}

// ─── Message shows FQN when short names collide ─────────────────────────────

#[test]
fn message_always_shows_fqn() {
    // Diagnostic messages always show full type names (FQN) so the
    // developer can find and fix the types.  Short names strip the
    // namespace which is the very information needed to resolve a
    // mismatch.
    let php = r#"<?php
/** @param \Vendor\Foo $f */
function takes_vendor(\Vendor\Foo $f): void {}

function test(): void {
    /** @var \App\Foo $f */
    $f = null;
    takes_vendor($f);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        !msgs.is_empty(),
        "Expected a type error for different-FQN same-short-name classes"
    );
    let msg = &msgs[0];
    // The message must include namespace-qualified names so the two
    // types are distinguishable.  Never "expects Foo, got Foo".
    assert!(
        msg.contains("Vendor\\Foo") && msg.contains("App\\Foo"),
        "Message should show FQN when short names collide, got: {msg}"
    );
}

// ─── Built-in function with stubs ───────────────────────────────────────────

#[test]
fn flags_array_passed_to_stub_function() {
    // str_contains expects (string $haystack, string $needle)
    let php = r#"<?php
function test(): void {
    $arr = [1, 2, 3];
    str_contains($arr, "x");
}
"#;
    let diags = collect_with_stubs(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for array passed to str_contains, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_correct_stub_function() {
    let php = r#"<?php
function test(): void {
    str_contains("hello world", "hello");
}
"#;
    let diags = collect_with_stubs(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag correct string args to str_contains, got: {diags:?}"
    );
}

// ─── No diagnostic: nullable arg to nullable param ──────────────────────────

#[test]
fn no_diagnostic_for_nullable_to_nullable() {
    let php = r#"<?php
function takes_nullable(?int $x): void {}

function test(?int $val): void {
    takes_nullable($val);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag ?int passed to ?int, got: {diags:?}"
    );
}

// ─── No diagnostic: default value param when argument omitted ───────────────

#[test]
fn no_false_positive_for_default_params() {
    let php = r#"<?php
function with_defaults(int $a, string $b = "hello"): void {}

function test(): void {
    with_defaults(42);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag when fewer args than params (defaults cover), got: {diags:?}"
    );
}

// ─── Multiple calls in same function ────────────────────────────────────────

#[test]
fn flags_multiple_bad_calls() {
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    $s = "hello";
    takes_int($s);
    $arr = [1, 2];
    takes_int($arr);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.len() >= 2,
        "Expected at least 2 type errors, got {}: {msgs:?}",
        msgs.len()
    );
}

// ─── No diagnostic for array to array param ─────────────────────────────────

#[test]
fn no_diagnostic_for_array_to_array() {
    let php = r#"<?php
function takes_array(array $items): void {}

function test(): void {
    $arr = [1, 2, 3];
    takes_array($arr);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag array passed to array, got: {diags:?}"
    );
}

// ─── Flags: string literal passed to int param ─────────────────────────────

#[test]
fn flags_string_literal_to_int() {
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    takes_int("hello");
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for string literal to int, got: {diags:?}"
    );
}

// ─── Flags: null literal to non-nullable ────────────────────────────────────

#[test]
fn flags_null_literal_to_non_nullable() {
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    takes_int(null);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for null to int, got: {diags:?}"
    );
}

// ─── Nested calls ───────────────────────────────────────────────────────────

#[test]
fn no_diagnostic_in_nested_scope() {
    let php = r#"<?php
function takes_int(int $x): void {}

class Foo {
    public function bar(): void {
        $n = 42;
        takes_int($n);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag correct call inside method, got: {diags:?}"
    );
}

#[test]
fn flags_error_in_nested_scope() {
    let php = r#"<?php
function takes_int(int $x): void {}

class Foo {
    public function bar(): void {
        $s = "hello";
        takes_int($s);
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error inside method body, got: {diags:?}"
    );
}

// ─── No diagnostic for bool to bool ─────────────────────────────────────────

#[test]
fn no_diagnostic_for_bool_to_bool() {
    let php = r#"<?php
function takes_bool(bool $x): void {}

function test(): void {
    $b = false;
    takes_bool($b);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag bool passed to bool, got: {diags:?}"
    );
}

// ─── No diagnostic for null to nullable union ───────────────────────────────

#[test]
fn no_diagnostic_for_null_to_nullable_union() {
    let php = r#"<?php
function takes_nullable_union(string|null $x): void {}

function test(): void {
    takes_nullable_union(null);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag null to string|null, got: {diags:?}"
    );
}

// ─── No diagnostic for new expression to matching class param ───────────────

#[test]
fn no_diagnostic_for_new_to_class_param() {
    let php = r#"<?php
class User {}

function takes_user(User $u): void {}

function test(): void {
    takes_user(new User());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag new User() passed to User param, got: {diags:?}"
    );
}

// ─── Flags: incompatible new expression ─────────────────────────────────────

#[test]
fn flags_new_wrong_class() {
    let php = r#"<?php
class Dog {}
class Cat {}

function takes_dog(Dog $d): void {}

function test(): void {
    takes_dog(new Cat());
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for new Cat() passed to Dog param, got: {diags:?}"
    );
}

// ─── Numeric string literals vs numeric-string ──────────────────────────────

#[test]
fn no_diagnostic_for_numeric_string_literal_to_numeric_string() {
    let php = r#"<?php
/** @param numeric-string $v */
function takes_numeric_string(string $v): void {}

function test(): void {
    takes_numeric_string('0.00');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag numeric string literal '0.00' passed to numeric-string param, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_integer_string_literal_to_numeric_string() {
    let php = r#"<?php
/** @param numeric-string $v */
function takes_numeric_string(string $v): void {}

function test(): void {
    takes_numeric_string('42');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag numeric string literal '42' passed to numeric-string param, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_numeric_literal_to_union_with_numeric_string() {
    let php = r#"<?php
class Decimal {
    /** @param Decimal|int|numeric-string $value */
    public function add(int|self|string $value): self { return $this; }
}

function test(): void {
    $d = new Decimal();
    $d->add('0.00');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag '0.00' passed to Decimal|int|numeric-string union, got: {diags:?}"
    );
}

#[test]
fn flags_non_numeric_string_literal_to_numeric_string() {
    // String literals are now narrowed to their literal type in argument
    // diagnostics, so we CAN prove `'hello'` is not a numeric-string.
    let php = r#"<?php
/** @param numeric-string $v */
function takes_numeric_string(string $v): void {}

function test(): void {
    takes_numeric_string('hello');
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Should flag non-numeric string literal 'hello' passed to numeric-string param, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_numeric_string_literal_to_numeric_string_precise() {
    // A numeric string literal like '42' IS a valid numeric-string.
    let php = r#"<?php
/** @param numeric-string $v */
function takes_numeric_string(string $v): void {}

function test(): void {
    takes_numeric_string('42');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag numeric string literal '42' passed to numeric-string param, got: {diags:?}"
    );
}

// ─── Array shape vs generic array ───────────────────────────────────────────

#[test]
fn no_diagnostic_for_array_shape_to_generic_array_string_mixed() {
    let php = r#"<?php
function takes_data(array $data): void {}

/** @param array<string, mixed> $data */
function takes_typed_data(array $data): void {}

function test(): void {
    takes_typed_data(['id' => 1, 'refunded_amount' => 'foo']);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag array shape passed to array<string, mixed> param, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// New rules: bare array ↔ typed array
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_bare_array_to_typed_array_generic() {
    let php = r#"<?php
/** @param array<string> $items */
function takes_string_array(array $items): void {}

function test(): void {
    $arr = [];
    takes_string_array($arr);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag bare array passed to array<string>, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_bare_array_to_list_generic() {
    let php = r#"<?php
/** @param list<int> $ids */
function takes_ids(array $ids): void {}

function test(): void {
    $arr = [];
    takes_ids($arr);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag bare array passed to list<int>, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// New rules: nullable arg → non-nullable param (MAYBE)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_nullable_arg_to_non_nullable_param() {
    let php = r#"<?php
class Carbon {}

function takes_carbon(Carbon $c): void {}

function test(?Carbon $c): void {
    takes_carbon($c);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag ?Carbon passed to Carbon (developer may have null-checked), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_nullable_string_to_string() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(?string $s): void {
    takes_string($s);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag ?string passed to string (MAYBE), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_non_nullable_to_nullable_param() {
    let php = r#"<?php
class Carbon {}

function takes_nullable(?Carbon $c): void {}

function test(): void {
    $c = new Carbon();
    takes_nullable($c);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Carbon passed to ?Carbon, got: {diags:?}"
    );
}

#[test]
fn flags_nullable_string_to_int() {
    // ?string should NOT be accepted where int is expected —
    // the non-null part (string) is still incompatible with int.
    let php = r#"<?php
function takes_int(int $x): void {}

function test(?string $s): void {
    takes_int($s);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected type error for ?string passed to int, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// New rules: Stringable objects accepted as string
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_object_to_string() {
    let php = r#"<?php
class HtmlString {
    public function __toString(): string { return ''; }
}

function takes_string(string $s): void {}

function test(): void {
    $h = new HtmlString();
    takes_string($h);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Stringable object passed to string, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// New rules: PHP type juggling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_int_to_string_method() {
    let php = r#"<?php
class Logger {
    public function log(string $message): void {}
}

function test(): void {
    $l = new Logger();
    $l->log(42);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag int passed to string method param (PHP juggling), got: {diags:?}"
    );
}

#[test]
fn still_flags_array_to_string() {
    // array → string is NOT type juggling, it's a real error.
    let php = r#"<?php
function takes_string(string $x): void {}

function test(): void {
    $arr = [1, 2, 3];
    takes_string($arr);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected type error for array passed to string, got: {diags:?}"
    );
}

#[test]
fn still_flags_bool_to_int() {
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    $b = true;
    takes_int($b);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected type error for bool passed to int, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// New rules: list<X> ↔ array<int, X>
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_list_to_array_int() {
    let php = r#"<?php
/** @param array<int, string> $items */
function takes_indexed(array $items): void {}

/** @return list<string> */
function get_list(): array { return []; }

function test(): void {
    takes_indexed(get_list());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag list<string> passed to array<int, string>, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_array_int_to_list() {
    let php = r#"<?php
/** @param list<string> $items */
function takes_list(array $items): void {}

/** @return array<int, string> */
function get_array(): array { return []; }

function test(): void {
    takes_list(get_array());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag array<int, string> passed to list<string>, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// New rules: class-string covariance
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_class_string_covariance() {
    let php = r#"<?php
class Animal {}
class Cat extends Animal {}

/** @param class-string<Animal> $cls */
function takes_animal_class(string $cls): void {}

/** @return class-string<Cat> */
function get_cat_class(): string { return Cat::class; }

function test(): void {
    takes_animal_class(get_cat_class());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag class-string<Cat> passed to class-string<Animal>, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// New rules: iterable<...> accepts arrays
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_diagnostic_for_array_to_iterable_generic() {
    let php = r#"<?php
/** @param iterable<mixed> $items */
function takes_iterable_generic(iterable $items): void {}

function test(): void {
    $arr = [1, 2, 3];
    takes_iterable_generic($arr);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag array passed to iterable<mixed>, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Template parameter detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_false_positive_for_method_level_template_with_literal() {
    // PHPUnit's assertEquals has @template ExpectedType with
    // @param ExpectedType $expected.  When the argument is a string
    // literal, resolve_arg_text_to_type resolves it to `string` and
    // build_method_template_subs substitutes ExpectedType → string.
    // The param type becomes `string`, matching the argument.
    let php = r#"<?php
class TestCase {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     */
    public function assertEquals(mixed $expected, mixed $actual): void {}
}

function test(): void {
    $t = new TestCase();
    $t->assertEquals("hello", "world");
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string literal passed to method-level @template param, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_phpunit_assert_same_static() {
    // Real-world PHPUnit pattern: assertSame is a final public static
    // method with @template ExpectedType and @param ExpectedType $expected.
    let php = r#"<?php
class TestCase {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     */
    final public static function assertSame(mixed $expected, mixed $actual, string $message = ''): void {}
}

class MyTest extends TestCase {
    public function testFoo(): void {
        self::assertSame("hello", "world");
        static::assertSame(42, 42);
        TestCase::assertSame(true, false);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag literals passed to PHPUnit assertSame @template param, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_self_assert_same_with_enum_and_property() {
    // Real-world PHPUnit pattern: self::assertSame with enum cases,
    // variables, property accesses, and integer literals as arguments.
    // All of these should have ExpectedType substituted correctly.
    let php = r#"<?php
enum VerificationType { case SMS; case Email; }
enum VerificationState { case Pending; case Done; }

class VerificationCode {
    public VerificationType $type;
    public VerificationState $state;
    public int $attempts;
    public string $identifier;
}

class TestCase {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     */
    final public static function assertSame(mixed $expected, mixed $actual, string $message = ''): void {}
}

class MyTest extends TestCase {
    public function testSend(): void {
        $expectedPhoneNumber = '+4530694258';
        $verificationCode = new VerificationCode();

        self::assertSame($expectedPhoneNumber, $verificationCode->identifier);
        self::assertSame(VerificationType::SMS, $verificationCode->type);
        self::assertSame(VerificationState::Pending, $verificationCode->state);
        self::assertSame(0, $verificationCode->attempts);
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        !has_type_error(&diags),
        "Should not flag enum cases, variables, property access, or int literals \
         passed to self::assertSame @template param, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_method_template_with_property_access_arg() {
    // When the first argument to a method-level @template method is
    // $var->prop (property access on a non-$this variable), the
    // template param should be substituted from the property's type.
    let php = r#"<?php
class Order {
    public string $name;
    public int $quantity;
}

class TestCase {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     */
    final public static function assertSame(mixed $expected, mixed $actual, string $message = ''): void {}
}

class MyTest extends TestCase {
    public function testOrder(): void {
        $order = new Order();
        self::assertSame($order->name, "foo");
        self::assertSame($order->quantity, 42);
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        !has_type_error(&diags),
        "Should not flag $var->prop passed to method-level @template param, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_method_template_with_method_call_arg() {
    // When the first argument to a method-level @template method is
    // a method call like $obj->getName(), the template param should
    // be substituted from the method's return type.
    let php = r#"<?php
class Helper {
    public function getText(): string { return "hello"; }
}

class TestCase {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     */
    final public static function assertSame(mixed $expected, mixed $actual, string $message = ''): void {}
}

class MyTest extends TestCase {
    private Helper $helper;
    public function testHelper(): void {
        self::assertSame($this->helper->getText(), "world");
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        !has_type_error(&diags),
        "Should not flag $this->helper->getText() passed to method-level @template param, got: {msgs:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Class-level template parameter substitution
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_false_positive_for_class_level_template_param() {
    // When a class declares @template T and a method has @param T $item,
    // the type_error diagnostic should substitute T with the concrete type
    // from the variable's generic type annotation (e.g. Collection<User>).
    let php = r#"<?php
/**
 * @template T
 */
class Collection {
    /** @param T $item */
    public function add($item): void {}
}

class User {}

function test(): void {
    /** @var Collection<User> $users */
    $users = new Collection();
    $users->add(new User());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag User passed to Collection<User>::add(), got: {diags:?}"
    );
}

#[test]
fn flags_wrong_type_for_class_level_template_param() {
    // After class-level template substitution, passing the wrong type
    // should still produce a diagnostic.
    let php = r#"<?php
/**
 * @template T
 */
class TypedBox {
    /** @param T $value */
    public function set($value): void {}
}

class Apple {}
class Orange {}

function test(): void {
    /** @var TypedBox<Apple> $box */
    $box = new TypedBox();
    $box->set(new Orange());
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected type error for Orange passed to TypedBox<Apple>::set(), got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_class_level_template_with_two_params() {
    // Two-parameter generic: Collection<TKey, TValue> with both substituted.
    let php = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
class Map {
    /** @param TKey $key */
    public function get($key): void {}

    /** @param TValue $value */
    public function put($value): void {}
}

class Product {}

function test(): void {
    /** @var Map<string, Product> $map */
    $map = new Map();
    $map->put(new Product());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Product passed to Map<string, Product>::put(), got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_class_template_nullable_param() {
    // Template param used in a nullable union: T|null should accept T.
    let php = r#"<?php
/**
 * @template T
 */
class Optional {
    /** @param T|null $value */
    public function set($value): void {}
}

class Item {}

function test(): void {
    /** @var Optional<Item> $opt */
    $opt = new Optional();
    $opt->set(new Item());
    $opt->set(null);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Item or null passed to Optional<Item>::set(T|null), got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_class_template_inherited_method() {
    // Template substitution should work through inherited methods too.
    let php = r#"<?php
/**
 * @template T
 */
class BaseRepo {
    /** @param T $entity */
    public function save($entity): void {}
}

/**
 * @extends BaseRepo<User>
 */
class UserRepo extends BaseRepo {}

class User {}

function test(): void {
    $repo = new UserRepo();
    $repo->save(new User());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag User passed to UserRepo::save() (inherited from BaseRepo<User>), got: {diags:?}"
    );
}

#[test]
fn method_level_template_with_variable_arg() {
    // Method-level @template where the argument is a variable whose type
    // can be resolved (not a literal).  build_method_template_subs can
    // resolve $user to User via resolve_arg_text_to_type.
    let php = r#"<?php
class TestCase {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     * @param ExpectedType $actual
     */
    public function assertEquals($expected, $actual): void {}
}

class User {}

function test(): void {
    $t = new TestCase();
    $expected = new User();
    $actual = new User();
    $t->assertEquals($expected, $actual);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag matching variable types for method-level template, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_closure_literal_to_template_param() {
    // When a method declares @template TClosure of \Closure and the
    // call-site argument is a closure/arrow function literal, the
    // template should be substituted with Closure so no false positive
    // is emitted.
    let php = r#"<?php
class Mockery {
    /**
     * @template TClosure of \Closure
     * @param TClosure $closure
     * @return void
     */
    public static function on($closure): void {}
}

function test(): void {
    Mockery::on(fn(array $query): bool => true);
    Mockery::on(function (int $x): string { return "hi"; });
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag closure literal passed to @template TClosure of \\Closure, got: {diags:?}"
    );
}

// ─── Interface → concrete implementor: MAYBE (reverse hierarchy) ────────────

#[test]
fn no_diagnostic_for_interface_arg_to_concrete_param() {
    // CarbonInterface passed where Carbon is expected.
    // Carbon implements CarbonInterface, so the value *might* be
    // the right concrete type at runtime (MAYBE → stay silent).
    let php = r#"<?php
interface CarbonInterface {}
class Carbon implements CarbonInterface {}

function takes_carbon(Carbon $c): void {}

function test(CarbonInterface $ci): void {
    takes_carbon($ci);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag interface arg passed to concrete param (MAYBE), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_parent_arg_to_child_param() {
    // Parent class passed where child is expected.
    // The value might be the child at runtime.
    let php = r#"<?php
class Animal {}
class Cat extends Animal {}

function takes_cat(Cat $c): void {}

function test(Animal $a): void {
    takes_cat($a);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag parent arg to child param (MAYBE), got: {diags:?}"
    );
}

// ─── Final class: reverse direction is NO ───────────────────────────────────

#[test]
fn flags_final_class_arg_to_child_param() {
    // A final class cannot have subtypes, so if `Jack` is final and
    // does not extend `JackSparrow`, it is definitely NOT a
    // JackSparrow.  The reverse-direction MAYBE does not apply.
    let php = r#"<?php
final class Jack {}
class JackSparrow {}

function takes_sparrow(JackSparrow $j): void {}

function test(Jack $j): void {
    takes_sparrow($j);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Should flag final class that is not a subtype (NO), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_final_class_that_implements_interface() {
    // A final class that implements the expected interface is
    // definitely compatible (direction 1: arg extends param → YES).
    let php = r#"<?php
interface Printable {}
final class Report implements Printable {}

function takes_printable(Printable $p): void {}

function test(): void {
    $r = new Report();
    takes_printable($r);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Final class implementing interface should be accepted (YES), got: {diags:?}"
    );
}

#[test]
fn flags_final_class_to_unrelated_interface() {
    // A final class that does NOT implement the interface is
    // definitely wrong — it can't be narrowed to anything else.
    let php = r#"<?php
interface Serializable {}
final class Rock {}

function takes_serializable(Serializable $s): void {}

function test(Rock $r): void {
    takes_serializable($r);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Final class not implementing interface should be flagged (NO), got: {diags:?}"
    );
}

// ─── object and stdClass are not universal supertypes ───────────────────────

#[test]
fn no_diagnostic_for_object_arg_to_specific_class() {
    // `object` passed where a specific class is expected is MAYBE.
    // The developer may have narrowed via instanceof before the call.
    // We flag `$obj->method()` as unknown-member instead — that's
    // where the developer learns they need better types.
    let php = r#"<?php
class User {}

function takes_user(User $u): void {}

function test(object $o): void {
    takes_user($o);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "object to specific class should be MAYBE (silent), got: {diags:?}"
    );
}

#[test]
fn flags_stdclass_to_unrelated_class() {
    // stdClass is a concrete class, not a universal parent.
    // Passing stdClass where User is expected is wrong.
    let php = r#"<?php
class User {}

function takes_user(User $u): void {}

function test(): void {
    $o = new \stdClass();
    takes_user($o);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Should flag stdClass passed to unrelated class param, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_specific_class_to_object_param() {
    // Any class instance IS an object — this is always valid.
    let php = r#"<?php
class User {}

function takes_object(object $o): void {}

function test(): void {
    $u = new User();
    takes_object($u);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should accept class instance passed to object param, got: {diags:?}"
    );
}

// ─── Non-final parent with unrelated child: MAYBE ───────────────────────────

#[test]
fn no_diagnostic_for_non_final_unrelated_with_common_parent() {
    // If a non-final class is passed where a sibling subclass is
    // expected, the developer might have narrowed.  Stay silent.
    let php = r#"<?php
class Animal {}
class Dog extends Animal {}
class Cat extends Animal {}

function takes_dog(Dog $d): void {}

function test(Animal $a): void {
    takes_dog($a);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Non-final parent to child should be MAYBE (silent), got: {diags:?}"
    );
}

// ─── Hierarchy resolution: Collection implements Countable ──────────────────

#[test]
fn no_diagnostic_for_collection_implementing_countable() {
    // Collection implements Countable.  The hierarchy check should
    // resolve this through the class loader without needing a
    // blanket "any object → Countable" rule.
    let php = r#"<?php
interface Countable {
    public function count(): int;
}
class Collection implements Countable {
    public function count(): int { return 0; }
}

function takes_countable(Countable $c): void {}

function test(): void {
    $col = new Collection();
    takes_countable($col);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Collection implementing Countable should be accepted via hierarchy, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_generic_collection_to_countable() {
    // Generic Collection<int, User> still implements Countable.
    // The base_name() of Generic("Collection", [int, User]) is
    // "Collection", and the hierarchy walk should find Countable.
    let php = r#"<?php
interface Countable {
    public function count(): int;
}

/** @template T */
class Collection implements Countable {
    public function count(): int { return 0; }
}

/** @param Collection<int, string> $items */
function takes_countable(Countable $c): void {}

function test(): void {
    /** @var Collection<int, string> $col */
    $col = new Collection();
    takes_countable($col);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Generic Collection<int, string> implementing Countable should be accepted, got: {diags:?}"
    );
}

// ── Transitive interface inheritance ────────────────────────────────────────

#[test]
fn no_diagnostic_for_transitive_interface_inheritance() {
    // ResponseInterface extends MessageInterface.
    // Response implements ResponseInterface.
    // Passing Response where MessageInterface is expected should
    // succeed via transitive interface walk.
    let php = r#"<?php
interface MessageInterface {
    public function getBody(): string;
}
interface ResponseInterface extends MessageInterface {
    public function getStatusCode(): int;
}
class Response implements ResponseInterface {
    public function getBody(): string { return ''; }
    public function getStatusCode(): int { return 200; }
}

function takes_message(MessageInterface $msg): void {}

function test(): void {
    $r = new Response();
    takes_message($r);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Response implementing ResponseInterface (extends MessageInterface) should be accepted, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_deep_transitive_interface() {
    // A extends B extends C extends D.
    // Class implements A.
    // Passing Class where D is expected should work through
    // the full transitive interface chain.
    let php = r#"<?php
interface D {}
interface C extends D {}
interface B extends C {}
interface A extends B {}
class Impl implements A {}

function takes_d(D $x): void {}

function test(): void {
    $impl = new Impl();
    takes_d($impl);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Class implementing A (extends B extends C extends D) should satisfy D, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_parent_class_transitive_interface() {
    // Parent class implements an interface that extends another.
    // Child class should satisfy the grandparent interface.
    let php = r#"<?php
interface Base {}
interface Middle extends Base {}
class Parent1 implements Middle {}
class Child extends Parent1 {}

function takes_base(Base $x): void {}

function test(): void {
    $c = new Child();
    takes_base($c);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Child (extends Parent1 implements Middle extends Base) should satisfy Base, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_multi_extends_interface() {
    // Interface extends multiple parent interfaces.
    // Class implementing the child interface should satisfy any parent.
    let php = r#"<?php
interface Readable {}
interface Writable {}
interface ReadWritable extends Readable, Writable {}
class Stream implements ReadWritable {}

function takes_readable(Readable $r): void {}
function takes_writable(Writable $w): void {}

function test(): void {
    $s = new Stream();
    takes_readable($s);
    takes_writable($s);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Stream implementing ReadWritable (extends Readable, Writable) should satisfy both, got: {diags:?}"
    );
}

// ── Array slice covariance ──────────────────────────────────────────────────

#[test]
fn no_diagnostic_for_array_slice_subclass() {
    // Child[] should be accepted where Parent[] is expected.
    let php = r#"<?php
class Animal {}
class Cat extends Animal {}

/** @param Animal[] $items */
function takes_animals(array $items): void {}

function test(): void {
    /** @var Cat[] $cats */
    $cats = [];
    takes_animals($cats);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Cat[] should be accepted where Animal[] is expected, got: {diags:?}"
    );
}

// ── Object-like arg to callable param ───────────────────────────────────────

#[test]
fn no_diagnostic_for_object_to_callable_param() {
    // An object might implement __invoke, making it callable.
    // We can't verify this statically, so stay silent (MAYBE).
    let php = r#"<?php
class MyHandler {}

function takes_callable(callable $fn): void {}

function test(): void {
    $handler = new MyHandler();
    takes_callable($handler);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Object passed to callable param should be MAYBE (might have __invoke), got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_object_to_callable_union() {
    // When param is callable|array and arg is an object, the union
    // recursion should check each branch.  The callable branch
    // should accept the object (MAYBE via __invoke).
    let php = r#"<?php
class Sequence {}

/** @param callable|array<string, mixed> $state */
function apply_state(callable|array $state): void {}

function test(): void {
    $seq = new Sequence();
    apply_state($seq);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Object passed to callable|array union should be accepted (MAYBE), got: {diags:?}"
    );
}

// ── BackedEnum hierarchy ────────────────────────────────────────────────────

#[test]
fn no_diagnostic_for_backed_enum_to_backed_enum_param() {
    // A specific backed enum should be accepted where BackedEnum
    // is expected, since all backed enums implement BackedEnum.
    let php = r#"<?php
interface BackedEnum {}
enum Color: string implements BackedEnum {
    case Red = 'red';
    case Blue = 'blue';
}

function takes_backed_enum(BackedEnum $e): void {}

function test(): void {
    $c = Color::Red;
    takes_backed_enum($c);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Backed enum implementing BackedEnum should be accepted, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_list_of_backed_enum_to_array_int_backed_enum() {
    // list<Color> should be accepted where array<int, BackedEnum>
    // is expected, combining list↔array and enum hierarchy rules.
    let php = r#"<?php
interface BackedEnum {}
enum Color: string implements BackedEnum {
    case Red = 'red';
    case Blue = 'blue';
}

/** @param array<int, BackedEnum> $items */
function takes_backed_enums(array $items): void {}

function test(): void {
    /** @var list<Color> $colors */
    $colors = [Color::Red, Color::Blue];
    takes_backed_enums($colors);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "list<Color> should be accepted where array<int, BackedEnum> is expected, got: {diags:?}"
    );
}

// ── Implicit BackedEnum/UnitEnum interface on enums ─────────────────────────

#[test]
fn no_diagnostic_for_backed_enum_implicit_backed_enum_interface() {
    // PHP backed enums automatically implement BackedEnum.
    // The parser adds this implicit interface so the hierarchy check
    // recognises the relationship.
    let php = r#"<?php
interface UnitEnum {}
interface BackedEnum extends UnitEnum {}

enum Status: string {
    case Active = 'active';
    case Inactive = 'inactive';
}

function takes_backed_enum(BackedEnum $e): void {}

function test(): void {
    takes_backed_enum(Status::Active);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "String-backed enum should satisfy BackedEnum via implicit interface, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_unit_enum_implicit_unit_enum_interface() {
    // PHP enums without a backing type automatically implement UnitEnum.
    let php = r#"<?php
interface UnitEnum {}

enum Suit {
    case Hearts;
    case Diamonds;
}

function takes_unit_enum(UnitEnum $e): void {}

function test(): void {
    takes_unit_enum(Suit::Hearts);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Unit enum should satisfy UnitEnum via implicit interface, got: {diags:?}"
    );
}

// ── Anonymous class arguments ───────────────────────────────────────────────

#[test]
fn no_diagnostic_for_anonymous_class_extending_expected_type() {
    // `new class extends Foo { … }` passed where Foo is expected.
    // Anonymous classes can't be verified reliably (synthetic names
    // aren't globally indexed), so we stay silent.
    let php = r#"<?php
class Model {
    public function save(): void {}
}

function takes_model(Model $m): void {}

function test(): void {
    $anon = new class extends Model {};
    takes_model($anon);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Anonymous class extending Model should be accepted where Model is expected, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_anonymous_class_implementing_interface() {
    // `new class implements Iface { … }` passed where Iface is expected.
    let php = r#"<?php
interface Renderable {
    public function render(): string;
}

function takes_renderable(Renderable $r): void {}

function test(): void {
    $anon = new class implements Renderable {
        public function render(): string { return ''; }
    };
    takes_renderable($anon);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Anonymous class implementing Renderable should be accepted, got: {diags:?}"
    );
}

// ─── Type guard narrowing ──────────────────────────────────────────────

#[test]
fn no_diagnostic_when_is_string_guard_narrows_before_call() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(mixed $val): void {
    if (!is_string($val)) {
        return;
    }
    takes_string($val);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "is_string guard should narrow mixed to string, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_when_instanceof_guard_narrows_before_call() {
    let php = r#"<?php
class Foo {}
function takes_foo(Foo $f): void {}

function test(mixed $val): void {
    if (!($val instanceof Foo)) {
        throw new \Exception('not Foo');
    }
    takes_foo($val);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "instanceof guard should narrow mixed to Foo, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_when_null_coalesce_with_throw_narrows() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(array $params): void {
    $authToken = $params['authToken'] ?? null;
    if (!$authToken || !is_string($authToken)) {
        throw new \Exception('missing');
    }
    takes_string($authToken);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Guard clause with throw should narrow type before call site, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_when_is_int_guard_narrows_nullable() {
    let php = r#"<?php
function takes_int(int $i): void {}

function test(?int $val): void {
    if ($val === null) {
        return;
    }
    takes_int($val);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Null check with early return should narrow ?int to int, got: {diags:?}"
    );
}

// ─── Foreach variable reassignment should not leak into RHS ─────────────────

#[test]
fn no_diagnostic_for_foreach_var_reassigned_in_body() {
    // When $type is the foreach key (string), and then reassigned to
    // BackedEnum::from($type), the $type argument inside from() should
    // still resolve as string (the foreach key type), not as the
    // reassigned DeviationType.
    let php = r#"<?php
enum DeviationType: string {
    case Unknown = 'unknown';
    case Missing = 'missing';
}

class Foo {
    /** @var array<string, string> */
    private static array $regexes = [];

    public static function test(string $message): void {
        foreach (self::$regexes as $type => $regex) {
            if (preg_match($regex, $message, $matches)) {
                $type = DeviationType::from($type);
            }
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Foreach key $type should be string when passed to from(), not DeviationType: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_foreach_over_extends_subclass_with_scalar_element() {
    // When iterating over a subclass that extends a generic collection
    // with scalar type args (e.g. `IntCollection extends Collection<int, int>`),
    // the foreach element type should be the concrete scalar, not the raw
    // template parameter name.
    let php = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 * @implements \ArrayAccess<TKey, TValue>
 */
class Collection implements \ArrayAccess {
    /** @return TValue */
    public function offsetGet(mixed $offset): mixed {}
    public function offsetExists(mixed $offset): bool {}
    public function offsetSet(mixed $offset, mixed $value): void {}
    public function offsetUnset(mixed $offset): void {}
}

/** @extends Collection<int, int> */
final class IntCollection extends Collection {}

function test(): void {
    $ids = new IntCollection();
    foreach ($ids as $id) {
        array_key_exists($id, [1 => 'a']);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Foreach element over @extends Collection<int, int> should be int, not TValue: {diags:?}"
    );
}

// ─── Additional positive tests: clear type mismatches ───────────────────────

#[test]
fn flags_float_passed_to_int() {
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    $f = 1.5;
    takes_int($f);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for float passed to int, got: {diags:?}"
    );
}

#[test]
fn flags_string_passed_to_bool() {
    let php = r#"<?php
function takes_bool(bool $x): void {}

function test(): void {
    $s = "hello";
    takes_bool($s);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for string passed to bool, got: {diags:?}"
    );
}

#[test]
fn flags_only_wrong_argument_not_correct_ones() {
    let php = r#"<?php
function takes_three(int $a, string $b, int $c): void {}

function test(): void {
    $arr = [1, 2];
    takes_three(1, $arr, 3);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for array passed as string param, got: {diags:?}"
    );
    // Should flag exactly one argument (the second one)
    let type_errors: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.code
                == Some(tower_lsp::lsp_types::NumberOrString::String(
                    "type_mismatch_argument".to_string(),
                ))
        })
        .collect();
    assert_eq!(
        type_errors.len(),
        1,
        "Expected exactly 1 type error (for arg 2), got {}: {type_errors:?}",
        type_errors.len()
    );
}

#[test]
fn flags_class_passed_to_unrelated_class() {
    let php = r#"<?php
class Dog {}
class Cat {}

function takes_cat(Cat $c): void {}

function test(): void {
    $d = new Dog();
    takes_cat($d);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for Dog passed to Cat, got: {diags:?}"
    );
}

#[test]
fn flags_null_passed_to_non_nullable() {
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    takes_int(null);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for null passed to int, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_conditionable_when_with_bool() {
    // Laravel's Conditionable::when() has
    // @param (Closure($this): TWhenParameter)|TWhenParameter|null $value
    // When called with a bool, TWhenParameter should resolve to bool
    // (Direct mode), not to null (from the missing $default arg).
    let php = r#"<?php
trait Conditionable {
    /**
     * @template TWhenParameter
     * @template TWhenReturnType
     * @param (\Closure($this): TWhenParameter)|TWhenParameter|null $value
     * @param (callable($this, TWhenParameter): TWhenReturnType)|null $callback
     * @param (callable($this, TWhenParameter): TWhenReturnType)|null $default
     * @return $this|TWhenReturnType
     */
    public function when($value = null, ?callable $callback = null, ?callable $default = null) {
        return $this;
    }
}

class Builder {
    use Conditionable;
}

function test(): void {
    $b = new Builder();
    $b->when(true, function (Builder $q): void {});
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag bool passed to when() with Conditionable template, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_conditionable_when_with_integer_and_callback_param() {
    let php = r#"<?php
trait Conditionable {
    /**
     * @template TWhenParameter
     * @template TWhenReturnType
     * @param (\Closure($this): TWhenParameter)|TWhenParameter|null $value
     * @param (callable($this, TWhenParameter): TWhenReturnType)|null $callback
     * @param (callable($this, TWhenParameter): TWhenReturnType)|null $default
     * @return $this|TWhenReturnType
     */
    public function when($value = null, ?callable $callback = null, ?callable $default = null) {
        return $this;
    }
}

class Request {
    public function integer(string $key): int {
        return 1;
    }
}

class Builder {
    use Conditionable;

    public function whereHas(string $relation, callable $callback): self {
        return $this;
    }

    public function whereKey(int $id): self {
        return $this;
    }
}

function test(Request $request, Builder $builder): void {
    $builder->when(
        $request->integer('root_ancestor_id'),
        fn (Builder $q, int $id) => $q->whereHas('rootAncestor', fn (Builder $q) => $q->whereKey($id)),
    );
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag int passed to when() with callback param template, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_template_null_default_no_overwrite() {
    // When a template param is resolved from one binding, a later
    // binding with a missing arg and null default should not
    // overwrite the already-resolved value.
    let php = r#"<?php
class Container {
    /**
     * @template T
     * @param T $value
     * @param T $fallback
     * @return T
     */
    public function coalesce($value, $fallback = null) {
        return $value ?? $fallback;
    }
}

function takes_string(string $x): void {}

function test(): void {
    $c = new Container();
    $result = $c->coalesce("hello");
    takes_string($result);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not overwrite resolved template with null default from missing arg, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_function_level_template_param() {
    // Functions like throw_unless have @template TValue with
    // @param TValue $condition.  The template should be substituted
    // with the concrete arg type so the param type is no longer
    // the raw template name.
    let php = r#"<?php
/**
 * @template TValue
 * @param TValue $condition
 * @return TValue
 */
function throw_unless($condition, $exception = 'RuntimeException') {
    return $condition;
}

class Feature {}

function test(): void {
    $feature = new Feature();
    throw_unless($feature, new \Exception('Missing'));
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Feature passed to function-level @template TValue param, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_class_string_template_unwrapping() {
    // assertInstanceOf has @template ExpectedType of object with
    // @param class-string<ExpectedType> $expected.  When the arg
    // is class-string<Foo>, ExpectedType should resolve to Foo
    // (not class-string<Foo>), avoiding class-string<class-string<Foo>>.
    let php = r#"<?php
class Assert {
    /**
     * @template ExpectedType of object
     * @param class-string<ExpectedType> $expected
     */
    public static function assertInstanceOf(string $expected, mixed $actual): void {}
}

class Service {}

function test(): void {
    /** @var class-string<Service> $class */
    $class = Service::class;
    Assert::assertInstanceOf($class, new Service());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag class-string<Service> passed to class-string<ExpectedType>, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_unresolved_template_safety_net() {
    // When template substitution cannot fire (e.g. no arg text
    // available, or bindings missing), the raw template name
    // leaks into the param type.  The safety net recognises
    // short non-namespace names that can't be loaded as classes
    // and suppresses the diagnostic.
    let php = r#"<?php
/**
 * @template T
 * @param T $value
 * @return T
 */
function identity($value) { return $value; }

class Foo {}

function test(): void {
    identity(new Foo());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag when template param is unresolved, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_function_template_with_class_string_param() {
    // A function with @template T and @param class-string<T> $class
    // should not double-wrap class-string when the arg is Foo::class.
    let php = r#"<?php
/**
 * @template T of object
 * @param class-string<T> $class
 * @return T
 */
function make(string $class): object { return new $class(); }

class MyService {}

function test(): void {
    make(MyService::class);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag MyService::class passed to class-string<T>, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_template_resolved_through_method_chain() {
    // When the first argument to assertSame is a method chain like
    // `new Decimal($x)->toFixed(2)`, the template ExpectedType should
    // resolve to the return type of `toFixed()` (string), not to
    // the base class `Decimal`.
    let php = r#"<?php
class Decimal {
    public function __construct(string $value) {}
    public function toFixed(int $places = 0): string { return '0'; }
    public function mul(int $qty): self { return $this; }
}

class Assert {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     */
    final public static function assertSame(mixed $expected, mixed $actual): void {}
}

function test(): void {
    Assert::assertSame(new Decimal('1.5')->toFixed(2), '1.50');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string vs Decimal when chain resolves to string, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_template_resolved_through_enum_property_access() {
    // When the first argument to assertSame is `MyEnum::Case->value`,
    // the template should resolve to the backing type (int|string),
    // not to the enum class itself.
    let php = r#"<?php
enum Country: string {
    case DK = 'dk';
    case SE = 'se';
}

class Assert {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     */
    final public static function assertSame(mixed $expected, mixed $actual): void {}
}

function test(): void {
    Assert::assertSame(Country::DK->value, 'dk');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string vs enum when ->value resolves to backing type, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_template_resolved_through_enum_method_call() {
    // SizeUnit::g->translation($country, $qty) should resolve the
    // template to the return type of translation(), not to SizeUnit.
    let php = r#"<?php
enum SizeUnit: string {
    case g = 'g';
    case ml = 'ml';

    public function translation(string $country, int $qty): string {
        return $this->value;
    }
}

class Assert {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     */
    final public static function assertSame(mixed $expected, mixed $actual): void {}
}

function test(): void {
    Assert::assertSame(SizeUnit::g->translation('dk', 1), 'g');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string vs SizeUnit when method chain resolves to string, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_class_template_via_method_return_type() {
    // When a method returns a generic class (e.g. HasMany<Translation, Tag>),
    // and we call a method on that result whose parameter is typed with a
    // class-level template parameter (@param TRelatedModel $model), the type
    // checker must substitute the template with the concrete type from the
    // return type annotation.  Without this, we get a false positive:
    // "expects TRelatedModel, got Translation".
    let php = r#"<?php
/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class HasMany {
    /** @param TRelatedModel $model */
    public function save($model): void {}
}

class Translation {}
class Tag {
    /** @return HasMany<Translation, Tag> */
    public function translations(): HasMany { return new HasMany(); }
}

function test(): void {
    $tag = new Tag();
    $translation = new Translation();
    $tag->translations()->save($translation);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Translation passed to HasMany<Translation, Tag>::save(), got: {diags:?}"
    );
}

#[test]
fn flags_wrong_type_for_class_template_via_method_return_type() {
    // Companion to the no-false-positive test above: when the wrong type
    // is passed to a generic method resolved through a return type
    // annotation, the diagnostic should still fire.
    let php = r#"<?php
/**
 * @template TRelatedModel
 * @template TDeclaringModel
 */
class HasMany {
    /** @param TRelatedModel $model */
    public function save($model): void {}
}

class Translation {}
class Comment {}
class Tag {
    /** @return HasMany<Translation, Tag> */
    public function translations(): HasMany { return new HasMany(); }
}

function test(): void {
    $tag = new Tag();
    $comment = new Comment();
    $tag->translations()->save($comment);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Should flag Comment passed to HasMany<Translation, Tag>::save() which expects Translation"
    );
}

#[test]
fn no_false_positive_for_class_template_via_static_method_return_type() {
    // Like the class-template parameter test above, but the value flows
    // through a static method return type.
    let php = r#"<?php
/**
 * @template T
 */
class Repository {
    /** @param T $entity */
    public function persist($entity): void {}
}

class User {}

class RepositoryFactory {
    /** @return Repository<User> */
    public static function userRepo(): Repository { return new Repository(); }
}

function test(): void {
    $user = new User();
    RepositoryFactory::userRepo()->persist($user);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag User passed to Repository<User>::persist() via static return, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_class_template_via_function_return_type() {
    // When a standalone function returns a generic class, calling a method
    // on its result should substitute the class-level template parameters.
    let php = r#"<?php
/**
 * @template TItem
 */
class Collection {
    /** @param TItem $item */
    public function add($item): void {}
}

class Product {}

/** @return Collection<Product> */
function getProducts(): Collection { return new Collection(); }

function test(): void {
    $product = new Product();
    getProducts()->add($product);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Product passed to Collection<Product>::add() via function return, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_class_template_two_params_via_return_type() {
    // When a return type carries multiple template arguments, all should
    // be substituted correctly in parameter types.
    let php = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
class TypedMap {
    /** @param TKey $key */
    public function hasKey($key): bool { return false; }
    /** @param TValue $value */
    public function addValue($value): void {}
}

class Label {}

class Registry {
    /** @return TypedMap<string, Label> */
    public function labels(): TypedMap { return new TypedMap(); }
}

function test(): void {
    $reg = new Registry();
    $reg->labels()->hasKey('foo');
    $reg->labels()->addValue(new Label());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string/Label passed to TypedMap<string, Label> methods, got: {diags:?}"
    );
}

// ── Unresolved template params resolved to bounds/mixed ─────────────

#[test]
fn no_false_positive_for_new_generic_class_without_annotation() {
    // When a generic class is instantiated without a generic annotation
    // (e.g. `new Collection()`), unbound template params should resolve
    // to their declared upper bound or `mixed`, not leak as raw names.
    let php = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {
    /** @param TValue $item */
    public function add($item): void {}

    /** @param TKey $key */
    public function get($key): void {}
}

function test(): void {
    $items = new Collection();
    $items->add('hello');
    $items->get(42);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string/int passed to unbound TValue/TKey params on new Collection(), got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_method_level_template_unbound() {
    // Method-level @template params that cannot be bound from call-site
    // arguments should resolve to their upper bound or `mixed`.
    let php = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {
    /**
     * @template TReduceInitial
     * @template TReduceReturnType
     * @param callable(TReduceInitial|TReduceReturnType, TValue, TKey): TReduceReturnType $callback
     * @param TReduceInitial $initial
     * @return TReduceInitial|TReduceReturnType
     */
    public function reduce(callable $callback, $initial = null): mixed { return null; }
}

class Decimal {
    public function __construct(string $v) {}
    public function add(Decimal $other): Decimal { return $this; }
}

function takes_decimal(Decimal $d): void {}

function test(): void {
    $items = new Collection();
    $total = $items->reduce(function (Decimal $carry): Decimal {
        return $carry;
    }, new Decimal('0.00'));
    takes_decimal($total);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Decimal passed to Decimal when reduce return type has unbound TReduceReturnType, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_function_level_template_unbound_return() {
    // Function-level @template params that cannot be bound from
    // call-site arguments should resolve to their upper bound or
    // `mixed`, not leak as raw names into the return type.
    let php = r#"<?php
/**
 * @template TReduceReturnType
 * @return TReduceReturnType
 */
function reduce_result() { return null; }

function takes_int(int $x): void {}

function test(): void {
    $result = reduce_result();
    takes_int($result);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag mixed passed to int when function template is unbound, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_collect_helper_without_args() {
    // The `collect()` helper returns `Collection<TKey, TValue>` where
    // TKey and TValue are function-level templates bound via the $value
    // param.  When called with no args, all templates should resolve to
    // their bounds (array-key / mixed).
    let php = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 * @param iterable<TKey, TValue>|null $value
 * @return Collection<TKey, TValue>
 */
function make_collection($value = []) { return new Collection(); }

/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {
    /** @param TValue $item */
    public function add($item): void {}
}

function test(): void {
    $items = make_collection();
    $items->add('hello');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string passed to mixed (unbound TValue) on make_collection() result, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_static_method_template_with_closure_arg() {
    // Static method with @template TClosure of Closure and @param TClosure.
    // When a closure literal is passed, the template should be substituted
    // with Closure (the bound) if direct resolution fails.
    let php = r#"<?php
class Matcher {
    /**
     * @template TClosure of \Closure
     * @param TClosure $closure
     */
    public static function on($closure): void {}
}

function test(): void {
    Matcher::on(fn(array $query): bool => true);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Closure passed to TClosure (bound is Closure), got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_foreach_key_reassigned_with_return_in_body() {
    // Reproduces the real-world PurchaseFileDeviationMessage pattern:
    // foreach key $type is string, reassigned to DeviationType::from($type)
    // inside an if block that also has a return statement. The $type argument
    // inside from() must resolve to string (the foreach key type), not
    // DeviationType (the reassigned type from the prescan).
    let php = r#"<?php
enum DeviationType: string {
    case Unknown = 'unknown';
    case MissingItem = 'missing';
    case UnorderedItem = 'unordered';
}

class PurchaseFileDeviationMessage
{
    /** @var array<string, string> */
    private static array $unknownProductRegexes = [];

    public static function fromMessage(string $message): self
    {
        foreach (self::$unknownProductRegexes as $type => $regex) {
            if (preg_match($regex, $message, $matches)) {
                $type = DeviationType::from($type);

                if (array_key_exists('LineId', $matches)) {
                    $lineId = (int)$matches['LineId'];
                }

                return new self();
            }
        }

        return new self();
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Foreach key $type should be string when passed to from(), not DeviationType. Got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_interface_template_params_without_implements_generics() {
    // When a class implements a generic interface but provides no
    // @implements generics, the interface's template params should be
    // substituted with their declared bounds (or mixed) instead of
    // leaking as raw names like TKey / TValue into inherited methods.
    let php = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 */
interface BaseDataContract {
    /**
     * @param array<TKey, TValue> $items
     */
    public static function collect(mixed $items): mixed;
}

abstract class Data implements BaseDataContract {
    public static function collect(mixed $items): mixed {
        return $items;
    }
}

final class RunningBonus extends Data {
    public function __construct(public readonly float $points) {}
}

function test(): void {
    RunningBonus::collect([new \stdClass()]);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag array<int, stdClass> passed to array<TKey, TValue> — interface template params should resolve to bounds, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_interface_template_through_intermediate_parent() {
    // Same as above but with an extra level of inheritance: the interface
    // template params must not leak through Data into RunningBonus.
    let php = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 */
interface GenericContract {
    /**
     * @param TValue $item
     */
    public function add(mixed $item): void;
}

abstract class AbstractData implements GenericContract {
    public function add(mixed $item): void {}
}

class MiddleLayer extends AbstractData {}

final class ConcreteItem extends MiddleLayer {}

function test(): void {
    $item = new ConcreteItem();
    $item->add('hello');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string passed to mixed (TValue resolved to bound) through intermediate parent, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_inherited_method_with_generic_return_type() {
    // When a parent class has a method returning a generic class and
    // a child class inherits it, calling a method on the return value
    // must substitute the template params from the parent's annotation.
    let php = r#"<?php
/**
 * @template T
 */
class Container {
    /** @param T $item */
    public function store($item): void {}
}

class Product {}

class BaseService {
    /** @return Container<Product> */
    public function getContainer(): Container { return new Container(); }
}

class ChildService extends BaseService {}

function test(): void {
    $child = new ChildService();
    $product = new Product();
    $child->getContainer()->store($product);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Product passed to Container<Product>::store() via inherited method, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_nullable_generic_return_type() {
    // When a method returns `?Container<Product>` (nullable generic),
    // null-safe chaining or a guarded call should still resolve the
    // template params correctly.
    let php = r#"<?php
/**
 * @template T
 */
class Wrapper {
    /** @param T $item */
    public function wrap($item): void {}
}

class Widget {}

class Factory {
    /** @return Wrapper<Widget>|null */
    public function maybeCreate(): ?Wrapper { return new Wrapper(); }
}

function test(): void {
    $factory = new Factory();
    $w = $factory->maybeCreate();
    if ($w !== null) {
        $widget = new Widget();
        $w->wrap($widget);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Widget passed to Wrapper<Widget>::wrap() from nullable return, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_trait_method_with_generic_return_type() {
    // When a trait provides a method returning a generic class and
    // a class uses that trait, the generic return type must be
    // resolved correctly on the using class.
    let php = r#"<?php
/**
 * @template T
 */
class Bag {
    /** @param T $item */
    public function put($item): void {}
}

class Fruit {}

trait HasBag {
    /** @return Bag<Fruit> */
    public function getBag(): Bag { return new Bag(); }
}

class Basket {
    use HasBag;
}

function test(): void {
    $basket = new Basket();
    $fruit = new Fruit();
    $basket->getBag()->put($fruit);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Fruit passed to Bag<Fruit>::put() via trait method, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_class_string_variable_passed_as_string() {
    let php = r#"<?php
class Pen {}
class Container {
    /**
     * @template T
     * @param class-string<T>|null $abstract
     * @return ($abstract is class-string<T> ? T : static)
     */
    public function make(?string $abstract = null): mixed {
        return new static();
    }
}
class Demo {
    public function run(): void {
        $container = new Container();
        $cls = Pen::class;
        $pen = $container->make($cls);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag class-string variable passed to ?string param, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_unresolved_class_template_in_constructor() {
    let php = r#"<?php
/**
 * @template T
 */
class Box {
    /** @var T */
    public $value;

    /** @param T $value */
    public function __construct(mixed $value = null) { $this->value = $value; }
}

class Gift {}

class Context {
    /** @var Box<Gift> */
    public $chest;

    public function __construct() { $this->chest = new Box(new Gift()); }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Gift passed to unresolved @template T param in constructor, got: {diags:?}"
    );
}

// ─── No false positive: closure passed to nullable callable parameter ────────

#[test]
fn no_false_positive_array_filter_closure_callback() {
    let php = r#"<?php
function array_filter(array $array, ?callable $callback = null, int $mode = 0): array {}

class Pen {
    public function color(): string { return 'blue'; }
}

function test(): void {
    /** @var list<Pen> $pens */
    $pens = [];
    $filtered = array_filter($pens, fn(Pen $p) => $p->color() === 'blue');
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Should not flag array_filter callback as wrong type, got: {msgs:?}"
    );
}

// ─── No false positive: FQN-resolved Closure vs callable ────────────────────

/// When argument types are FQN-resolved (e.g. `\Closure` instead of
/// `Closure`), the subtype check `\Closure <: callable` must still hold.
#[test]
fn no_false_positive_fqn_closure_subtype_of_callable() {
    let php = r#"<?php
function takes_callable(callable $fn): void {}

function test(): void {
    takes_callable(fn() => 42);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Should not flag arrow function passed to callable param, got: {msgs:?}"
    );
}

/// Closure passed to `callable|null` union (docblock-style nullable).
#[test]
fn no_false_positive_closure_to_callable_or_null_union() {
    let php = r#"<?php
/**
 * @param callable|null $callback
 */
function maybe_call(callable|null $callback = null): void {}

function test(): void {
    maybe_call(fn() => true);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Should not flag closure passed to callable|null param, got: {msgs:?}"
    );
}

/// When the return type of the outermost method call is an unresolvable
/// class (not in the project), the resolver must not fall through and report
/// the type of the *argument* passed into that method instead.
#[test]
fn no_false_positive_for_nested_call_with_unresolvable_return_type() {
    use crate::common::create_psr4_workspace;

    let files = vec![
        (
            "src/ArtifactList.php",
            r#"<?php
namespace App;

/** @implements \Iterator<int, mixed> */
class ArtifactList implements \Iterator {
    public function current(): mixed { return null; }
    public function key(): int { return 0; }
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool { return false; }
}
"#,
        ),
        (
            "src/Source.php",
            r#"<?php
namespace App;

class Source {
    public function getClasses(): ArtifactList {
        return new ArtifactList();
    }
}
"#,
        ),
        (
            "src/ClassNode.php",
            r#"<?php
namespace App;

class ClassNode {
    /** @param \PDepend\Source\AST\ASTClass $node */
    public function __construct(\PDepend\Source\AST\ASTClass $node) {}
}
"#,
        ),
        (
            "src/TestCase.php",
            r#"<?php
namespace App;

use PDepend\Source\AST\ASTNode;

class TestCase {
    private function parseTestCaseSource(): Source {
        return new Source();
    }

    /** @return \PDepend\Source\AST\ASTNode */
    private function getNodeForCallingTestCase(\Iterator $nodes): ASTNode {
        /** @var ASTNode */
        return $nodes->current();
    }

    protected function getClass(): ClassNode {
        return new ClassNode(
            $this->getNodeForCallingTestCase(
                $this->parseTestCaseSource()->getClasses()
            )
        );
    }
}
"#,
        ),
    ];

    let composer = r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#;
    let (backend, dir) = create_psr4_workspace(composer, &files);
    let uri = format!("file://{}/src/TestCase.php", dir.path().display());
    let content = files[3].1;
    let mut diags = Vec::new();
    backend.collect_type_error_diagnostics(&uri, content, &mut diags);
    let msgs = type_error_messages(&diags);
    // The argument to ClassNode::__construct is the return value of
    // getNodeForCallingTestCase which returns ASTNode.  The diagnostic
    // must NOT say "got ArtifactList" (the type of the argument passed
    // *into* getNodeForCallingTestCase).
    for msg in &msgs {
        assert!(
            !msg.contains("ArtifactList"),
            "Nested call resolved to inner argument type instead of outermost return type: {msg}"
        );
    }
}

// ─── parent::__construct() with @extends generics ───────────────────────────

#[test]
fn no_false_positive_parent_construct_with_extends_generics() {
    let php = r#"<?php
/**
 * @template T of object
 */
class ItemResult {
    /** @param ?T $item */
    public function __construct(private readonly ?object $item) {}
}

/**
 * @extends ItemResult<BonusCashItem>
 */
final class BonusCashItemResult extends ItemResult {
    public function __construct(?BonusCashItem $credited) {
        parent::__construct($credited);
    }
}

class BonusCashItem {}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Expected no type errors for parent::__construct with @extends generics, got: {msgs:?}"
    );
}

// ─── Array access on bare `array` returns mixed ─────────────────────────────

#[test]
fn no_false_positive_array_access_on_bare_array() {
    let php = r#"<?php
function foo(array $params = []): void {
    $authToken = $params['authToken'] ?? null;
    if (!$authToken || !is_string($authToken)) {
        throw new \Exception('missing');
    }
    bar($authToken);
}
function bar(string $s): void {}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Expected no type errors for array access on bare array, got: {msgs:?}"
    );
}

/// When a file imports a namespaced class under the same short name as a
/// global class (e.g. `use App\Exceptions\Exception;`), the class_loader's
/// use-map shadows the global `\Exception` during hierarchy walks.  This
/// must not produce a false positive when the imported class ultimately
/// extends the global class that implements the expected interface.
#[test]
fn no_false_positive_when_use_map_shadows_global_parent() {
    let php = r#"<?php
namespace App\Http;

use App\Exceptions\MyException;
use Throwable;

function report(Throwable $e): void {}

function test(): void {
    report(new MyException('oops'));
}
"#;

    let backend = create_test_backend();
    let exception_php = r#"<?php
namespace App\Exceptions;

use Exception as NativeException;

class MyException extends NativeException {}
"#;
    backend.update_ast("file:///app/Exceptions/MyException.php", exception_php);
    backend.update_ast("file:///test.php", php);

    let mut out = Vec::new();
    backend.collect_type_error_diagnostics("file:///test.php", php, &mut out);
    let msgs = type_error_messages(&out);
    assert!(
        msgs.is_empty(),
        "Should not flag Exception subclass as incompatible with Throwable, got: {msgs:?}"
    );
}

// ─── Bare array (Array(mixed)) passed to typed array parameter ──────────────

#[test]
fn no_false_positive_bare_array_from_method_call_to_typed_array_param() {
    let php = r#"<?php
class ORM {
    /** @return array */
    public function getByQuery(string $class, string $query): array { return []; }
}

class Controller {
    /** @param array<Item> $items */
    public function process(array $items): void {}

    public function test(ORM $orm): void {
        $items = $orm->getByQuery('Item', 'SELECT * FROM items');
        $this->process($items);
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Should not flag bare array from method return as incompatible with typed array param, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_property_narrowed_via_instanceof() {
    let php = r#"<?php
interface MockInterface {
    public function shouldReceive(string $name): self;
}

class EpaymentService {
    public function annul(): bool { return true; }
}

class TestCase {
    private EpaymentService $service;

    protected function mockMethod(MockInterface $mock, string $method): void {}

    public function test(): void {
        if ($this->service instanceof MockInterface) {
            $this->mockMethod($this->service, 'annul');
        }
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Property narrowed via instanceof should be accepted as MockInterface, got: {msgs:?}"
    );
}

// ─── Literal string matching literal type in union ──────────────────────────

#[test]
fn no_false_positive_for_string_literal_matching_literal_type() {
    let php = r#"<?php
/** @param 'asc'|'desc' $direction */
function orderBy(string $column, string $direction): void {}

function test(): void {
    orderBy('id', 'desc');
    orderBy('name', 'asc');
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "String literal 'desc' should match literal type 'desc' in union, got: {msgs:?}"
    );
}

#[test]
fn flags_wrong_string_literal_for_literal_type() {
    let php = r#"<?php
/** @param 'asc'|'desc' $direction */
function orderBy(string $column, string $direction): void {}

function test(): void {
    orderBy('id', 'invalid');
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "String literal 'invalid' should NOT match 'asc'|'desc', got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_int_literal_matching_literal_type() {
    let php = r#"<?php
/** @param 1|2|3 $mode */
function setMode(int $mode): void {}

function test(): void {
    setMode(2);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Integer literal 2 should match literal type 2 in union, got: {msgs:?}"
    );
}

#[test]
fn flags_wrong_int_literal_for_literal_type() {
    let php = r#"<?php
/** @param 1|2|3 $mode */
function setMode(int $mode): void {}

function test(): void {
    setMode(99);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Integer literal 99 should NOT match 1|2|3, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_float_literal_matching_literal_type() {
    let php = r#"<?php
/** @param 1.5|2.5|3.5 $rate */
function setRate(float $rate): void {}

function test(): void {
    setRate(2.5);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Float literal 2.5 should match literal type 2.5 in union, got: {msgs:?}"
    );
}

#[test]
fn flags_wrong_float_literal_for_literal_type() {
    let php = r#"<?php
/** @param 1.5|2.5|3.5 $rate */
function setRate(float $rate): void {}

function test(): void {
    setRate(9.9);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Float literal 9.9 should NOT match 1.5|2.5|3.5, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_non_decimal_int_literals_matching_int_param() {
    // Hex, binary, octal, and underscore-separated integer literals are
    // narrowed to their parsed value, so they still satisfy an `int`
    // parameter (their raw source text would not parse back into a number).
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    takes_int(0xFF);
    takes_int(0b1010);
    takes_int(1_000);
    takes_int(0o17);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Non-decimal int literals should match int param, got: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn no_false_positive_for_hex_literal_matching_decimal_literal_union() {
    // `0x2` is value 2, which is a member of the decimal literal union.
    let php = r#"<?php
/** @param 1|2|3 $mode */
function setMode(int $mode): void {}

function test(): void {
    setMode(0x2);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Hex literal 0x2 should match decimal literal 2 in union, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_binary_and_octal_literals_matching_decimal_literal_union() {
    let php = r#"<?php
/** @param 2|8|10 $mode */
function setMode(int $mode): void {}

function test(): void {
    setMode(0b10);
    setMode(0o10);
    setMode(1_0);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Binary, octal, and underscored int literals should match decimal literal unions, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_scientific_float_literal_matching_decimal_literal_union() {
    let php = r#"<?php
/** @param 1000.0|2000.0 $value */
function setValue(float $value): void {}

function test(): void {
    setValue(1e3);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Scientific float literal 1e3 should match decimal literal 1000.0 in union, got: {msgs:?}"
    );
}
#[test]
fn no_false_positive_for_single_quoted_string_matching_double_quoted_literal_union() {
    let php = r#"<?php
/** @param "select"|"from"|"join" $type */
function addBinding(array $bindings, string $type): void {}

function test(): void {
    addBinding([], 'select');
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Single-quoted string literal should match double-quoted literal union member, got: {msgs:?}"
    );
}

// ─── array_map callback return type (#147) ──────────────────────────────────

#[test]
fn no_false_positive_for_array_map_with_scalar_return_type() {
    // array_map(fn(Item): string => ..., $items) should produce
    // list<string>, not list<Item>.  The callback's return type
    // determines the output element type.
    let php = r#"<?php
class Item {
    public function __construct(public string $id) {}
}

/** @param list<string> $ids */
function takesStrings(array $ids): void {}

/** @param list<Item> $items */
function run(array $items): void {
    takesStrings(array_map(fn(Item $item): string => $item->id, $items));
}
"#;
    let diags = collect_with_stubs(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "array_map with scalar return type should infer list<string>, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_array_map_inferred_return_type() {
    // array_map(fn($item) => $item->id, $items) — no explicit return
    // type hint.  The LSP should infer the return type from the body
    // expression: $item->id is string, so the result is list<string>.
    let php = r#"<?php
class Item {
    public function __construct(public string $id) {}
}

/** @param list<string> $ids */
function takesStrings(array $ids): void {}

/** @param list<Item> $items */
function run(array $items): void {
    takesStrings(array_map(fn($item) => $item->id, $items));
}
"#;
    let diags = collect_with_stubs(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "array_map should infer return type from body expression, got: {msgs:?}"
    );
}

// ─── strict_types=1 detection ───────────────────────────────────────────────

#[test]
fn strict_types_flags_int_passed_to_string() {
    let php = r#"<?php
declare(strict_types=1);

function takes_string(string $s): void {}

function test(): void {
    $x = 42;
    takes_string($x);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Under strict_types=1, int passed to string should be flagged, got: {diags:?}"
    );
}

#[test]
fn no_strict_types_allows_int_passed_to_string() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(): void {
    $x = 42;
    takes_string($x);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Without strict_types, int passed to string should be allowed, got: {msgs:?}"
    );
}

#[test]
fn strict_types_allows_int_passed_to_float() {
    // int → float is the one exception under strict_types=1
    let php = r#"<?php
declare(strict_types=1);

function takes_float(float $f): void {}

function test(): void {
    $x = 42;
    takes_float($x);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Under strict_types=1, int passed to float should still be allowed, got: {msgs:?}"
    );
}

#[test]
fn strict_types_flags_numeric_string_to_int() {
    let php = r#"<?php
declare(strict_types=1);

/** @param numeric-string $v */
function takes_numeric(string $v): void {}

function test(): void {
    takes_int(42);
}

function takes_int(int $n): void {}

function test2(): void {
    $x = '42';
    takes_int($x);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Under strict_types=1, string passed to int should be flagged even if numeric, got: {diags:?}"
    );
}

#[test]
fn strict_types_does_not_affect_concatenation() {
    // strict_types only affects scalar type declarations (function params,
    // return types, property assignments).  String concatenation with `.`
    // always coerces implicitly regardless of strict_types.
    let php = r#"<?php
declare(strict_types=1);

function test(): void {
    $x = 42;
    $s = 'count: ' . $x;
    echo $s;
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Concatenation should not be affected by strict_types, got: {msgs:?}"
    );
}

#[test]
fn strict_types_flags_int_literal_passed_to_string_param() {
    // Even an integer literal (not just a variable) should be flagged
    // under strict_types=1 when passed to a string parameter.
    let php = r#"<?php
declare(strict_types=1);

function takes_string(string $s): void {}

function test(): void {
    takes_string(42);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Under strict_types=1, int literal 42 passed to string param should be flagged, got: {diags:?}"
    );
}

#[test]
fn strict_types_flags_float_passed_to_string() {
    let php = r#"<?php
declare(strict_types=1);

function takes_string(string $s): void {}

function test(): void {
    $x = 3.14;
    takes_string($x);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Under strict_types=1, float passed to string should be flagged, got: {diags:?}"
    );
}

#[test]
fn no_strict_types_allows_float_passed_to_string() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(): void {
    takes_string(1.0);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Without strict_types, float passed to string should be allowed, got: {diags:?}"
    );
}

#[test]
fn no_strict_types_allows_numeric_string_literal_to_int() {
    let php = r#"<?php
function takes_int(int $n): void {}

function test(): void {
    takes_int('42');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Without strict_types, numeric string literal passed to int should be allowed, got: {diags:?}"
    );
}

#[test]
fn strict_types_flags_numeric_string_literal_to_int() {
    let php = r#"<?php
declare(strict_types=1);

function takes_int(int $n): void {}

function test(): void {
    takes_int('42');
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Under strict_types=1, numeric string literal passed to int should be flagged, got: {diags:?}"
    );
}

#[test]
fn int_range_rejects_float_literal() {
    let php = r#"<?php
/** @param int<0, max> $micros */
function takes_range($micros): void {}

function test(): void {
    takes_range(1.0);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Float literal should not satisfy int range parameter, got: {diags:?}"
    );
}

#[test]
fn int_range_accepts_hex_integer_literal() {
    let php = r#"<?php
/** @param int<0, 32> $value */
function takes_range($value): void {}

function test(): void {
    takes_range(0x10);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Hex integer literal within range should be allowed, got: {diags:?}"
    );
}

#[test]
fn int_range_accepts_binary_octal_and_underscored_integer_literals() {
    let php = r#"<?php
/** @param int<0, 32> $value */
function takes_range($value): void {}

function test(): void {
    takes_range(0b10000);
    takes_range(0o20);
    takes_range(1_6);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Binary, octal, and underscored integer literals within range should be allowed, got: {diags:?}"
    );
}

#[test]
fn string_literal_argument_ignores_quote_style() {
    // A double-quoted argument literal must match a single-quoted docblock
    // literal (and vice versa) when their unquoted contents are identical.
    let php = r#"<?php
/** @param 'asc'|'desc' $direction */
function order_by(string $column, string $direction): void {}

function test(): void {
    order_by('id', "desc");
    order_by('id', 'desc');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "String literal argument should match docblock literal regardless of quote style, got: {diags:?}"
    );
}

#[test]
fn string_literal_argument_still_flags_wrong_value() {
    // Normalising quote style must not swallow genuinely mismatched values.
    let php = r#"<?php
/** @param 'asc'|'desc' $direction */
function order_by(string $column, string $direction): void {}

function test(): void {
    order_by('id', "nope");
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "A string literal outside the allowed set should still be flagged, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_string_indexed_assignment() {
    // When a string variable is modified via bracket-index assignment
    // (`$str[0] = 'z'`), the variable should remain a `string` — it
    // must NOT be widened to `array<int, string>`.
    // See: https://github.com/PHPantom-dev/phpantom_lsp/issues/207
    let php = r#"<?php
function test(): void {
    $x = "abc";
    $x[0] = "z";
    echo bin2hex($x);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "String indexed assignment should preserve string type, got: {diags:?}"
    );
}

#[test]
fn no_false_positive_for_ternary_with_array_access_branch() {
    // When a ternary expression has an array-access branch that resolves
    // to `mixed` (from `array<string, mixed>`), the resulting variable
    // type should be `mixed|null`, not just `null`.
    // See: https://github.com/PHPantom-dev/phpantom_lsp/issues/206
    let php = r#"<?php
function takes_string(string $s): void {}

/**
 * @param array<string, mixed> $body
 */
function myFunction(array $body): void {
    $statementHandle = true ? $body['statementHandle'] : null;

    takes_string($statementHandle);
}
"#;
    let diags = collect(php);
    // `$statementHandle` is `mixed|null`; `mixed` is a supertype of `string`,
    // so passing it to a `string` parameter should NOT be flagged.
    // The bug was that the ternary resolved to just `null`.
    assert!(
        !has_type_error(&diags),
        "Ternary with array access branch should resolve to mixed|null, not null: {diags:?}"
    );
}
