use crate::common::{
    create_test_backend, create_test_backend_with_full_stubs,
    create_test_backend_with_function_stubs,
};
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn collect(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_argument_type_diagnostics(uri, php, &mut out);
    out
}

fn collect_with_stubs(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend_with_function_stubs();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_argument_type_diagnostics(uri, php, &mut out);
    out
}

fn collect_with_full_stubs(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_argument_type_diagnostics(uri, php, &mut out);
    out
}

/// Collect diagnostics through the full slow-diagnostic pipeline so that
/// the chain resolution cache is active (as it is during real analysis
/// and LSP requests).  Needed for tests that exercise cross-call-site
/// caching behaviour.
fn collect_slow(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, php, &mut out);
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
            .any(|m| m.contains("int") && m.contains("\"hello\"")),
        "Expected message mentioning int and the actual string literal, got: {msgs:?}"
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

#[test]
fn no_diagnostic_when_docblock_overrides_nullable_native() {
    // A `@param Item[]` docblock overriding a native `?array` still
    // accepts null at runtime, so passing null must not be flagged.
    let php = r#"<?php
class Item {}

class Component {
    /**
     * @param Item[] $items
     */
    public function __construct(?array $items) {}
}

function test(): void {
    new Component(null);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag null passed to a docblock-overridden nullable param, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_when_default_is_null() {
    // A parameter with a literal `null` default accepts null even when
    // the native hint is non-nullable (the pre-8.4 implicit-nullable form).
    let php = r#"<?php
function takes_default_null(string $baseurl = null): void {}

function test(): void {
    takes_default_null(null);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag null passed to a param with a null default, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_when_docblock_type_and_default_null() {
    // Combines both facets: docblock type plus a `null` default.
    let php = r#"<?php
class Item {}

function takes_items(array $items = null): void {}

function test(): void {
    takes_items(null);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag null passed to a param with a null default and array hint, got: {diags:?}"
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

#[test]
fn no_diagnostic_for_generic_class_to_object() {
    let php = r#"<?php
/** @template T */
class Collection {}

class Foo {}

function takes_object(object $x): void {}

/** @param Collection<Foo> $c */
function test(Collection $c): void {
    takes_object($c);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Type arguments have no say in whether a value is an object, got: {diags:?}"
    );
}

#[test]
fn flags_nullable_class_to_object() {
    let php = r#"<?php
class Foo {}

function takes_object(object $x): void {}

function test(?Foo $f): void {
    takes_object($f);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "`null` is not an object, so a nullable argument does not satisfy `object`"
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

// ─── No diagnostic: class named `Number` is not the `number` pseudo-type ─────

#[test]
fn no_diagnostic_for_class_named_number() {
    // `number` is a PHPDoc-only pseudo-type, but PHP allows a real class named
    // `Number` (e.g. PHP 8.4's `BcMath\Number`). The class must not be shadowed
    // by the pseudo-type, otherwise a valid argument is wrongly flagged.
    let php = r#"<?php
namespace App;

class Number {
    public function __construct(public string $value) {}
}

function scale(Number $n): void {}

function test(string $v): void {
    scale(new Number($v));
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A `Number` argument passed to a `Number` param must not be flagged, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_type_to_number_class_param() {
    // The `Number` class must still participate in genuine mismatch detection:
    // passing an unrelated class where a `Number` is expected is an error.
    let php = r#"<?php
namespace App;

class Number {}
class Money {}

function scale(Number $n): void {}

function test(): void {
    scale(new Money());
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Passing Money to a Number param should be flagged, got: {diags:?}"
    );
}

// ─── No diagnostic: a user class shadows an alias-with-native-counterpart ───

#[test]
fn no_diagnostic_for_class_named_integer_via_phpdoc_param() {
    // `integer` is only a legacy PHP alias for `int`, never a reserved
    // keyword, so a project may declare a real class named `Integer`. A
    // `@param Integer $value` annotation naming that class must resolve to
    // the class, not PHP's `int` alias.
    let php = r#"<?php
final class Integer {}

/** @param Integer $value */
function acceptsInteger($value): void {}

acceptsInteger(new Integer());
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "An `Integer` argument passed to a `@param Integer` param must not be flagged, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_type_to_integer_class_phpdoc_param() {
    // The `Integer` class must still participate in genuine mismatch
    // detection: passing an unrelated class where `Integer` is expected is
    // an error.
    let php = r#"<?php
final class Integer {}
final class Money {}

/** @param Integer $value */
function acceptsInteger($value): void {}

acceptsInteger(new Money());
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Passing Money to an `@param Integer` param should be flagged, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_classes_named_boolean_double_resource_via_phpdoc_param() {
    // `boolean`, `double`, and `resource` share the same status as
    // `integer`: PHP aliases/legacy names with no reserved keyword, so a
    // project may declare classes with these names and they must win over
    // the pseudo-type reading.
    let php = r#"<?php
final class Boolean {}
final class Double {}
final class Resource {}

/**
 * @param Boolean $b
 * @param Double $d
 * @param Resource $r
 */
function accepts($b, $d, $r): void {}

accepts(new Boolean(), new Double(), new Resource());
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Boolean/Double/Resource class instances must not be flagged against \
         same-named @param annotations, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_classes_named_scalar_and_numeric() {
    // `scalar` and `numeric` are PHPDoc-only pseudo-types with no native
    // spelling, so `Scalar` and `Numeric` are ordinary class names —
    // nikic/php-parser ships a `PhpParser\Node\Scalar`. Folding either into
    // the pseudo-type leaves the name unresolvable and every subtype check
    // against it fails.
    let php = r#"<?php
namespace App;

class Expr {}
class Scalar extends Expr {}
class Numeric extends Expr {}

function takesExpr(Expr $e): void {}

/**
 * @param Scalar $s
 * @param Numeric $n
 */
function accepts($s, $n): void {
    takesExpr($s);
    takesExpr($n);
}

function nativeParam(Scalar $s): void {
    takesExpr($s);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A `Scalar`/`Numeric` subclass passed to its parent's param must not be \
         flagged, got: {diags:?}"
    );
}

#[test]
fn flags_wrong_type_to_scalar_class_param() {
    // The `Scalar` class must still participate in genuine mismatch
    // detection rather than being silently unresolvable.
    let php = r#"<?php
namespace App;

class Scalar {}
class Money {}

function takesScalar(Scalar $s): void {}

function test(): void {
    takesScalar(new Money());
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Passing Money to a Scalar param should be flagged, got: {diags:?}"
    );
}

#[test]
fn lowercase_scalar_and_numeric_stay_pseudo_types() {
    // The lowercase spellings keep their PHPDoc meaning: `scalar` accepts an
    // `int`, and an object does not satisfy it.
    let php = r#"<?php
namespace App;

class Money {}

/** @param scalar $s */
function takesScalar($s): void {}

function test(): void {
    takesScalar(1);
    takesScalar(new Money());
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "An object passed to a `@param scalar` must be flagged, got: {diags:?}"
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

#[test]
fn no_diagnostic_for_self_param_reached_through_a_property() {
    // The receiver is a property, so the enclosing class (`Machine`) is
    // not the class declaring `canChangeTo` — `self` must bind to `State`.
    let php = r#"<?php
enum State: string
{
    case A = 'a';
    case B = 'b';

    public function canChangeTo(self $newState): bool { return true; }
}

class Machine
{
    public State $state = State::A;

    public function go(): void
    {
        $this->state->canChangeTo(State::B);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag an enum case passed to a self parameter of the same enum, got: {diags:?}"
    );
}

#[test]
fn self_param_mismatch_names_the_declaring_class() {
    let php = r#"<?php
class Node {
    public function merge(self $other): void {}
}
class Other {}
class Machine {
    public Node $node;
    public function go(): void {
        $this->node->merge(new Other());
    }
}
"#;
    let msgs = type_error_messages(&collect(php));
    assert_eq!(
        msgs,
        vec!["Argument 1 ($other) expects Node, got Other".to_string()],
        "The message must name the class `self` resolves to, not the keyword"
    );
}

#[test]
fn no_diagnostic_for_inherited_self_param_given_the_declaring_class() {
    // `self` in an inherited method binds to the class that declares it,
    // so `Base` is still an acceptable argument when called on a `Child`.
    let php = r#"<?php
class Base {
    public function merge(self $other): void {}
}
class Child extends Base {}
class Machine {
    public Child $child;
    public function go(): void {
        $this->child->merge(new Base());
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Base passed to an inherited self parameter, got: {diags:?}"
    );
}

#[test]
fn parent_param_is_resolved_to_the_parent_class() {
    let php = r#"<?php
class Base {}
class Other {}
class Mid extends Base {
    public function take(parent $p): void {}
}
class Machine {
    public Mid $mid;
    public function go(): void {
        $this->mid->take(new Base());
        $this->mid->take(new Other());
    }
}
"#;
    let msgs = type_error_messages(&collect(php));
    assert_eq!(
        msgs,
        vec!["Argument 1 ($p) expects Base, got Other".to_string()],
        "`parent` must resolve to Base: accept a Base, reject an unrelated class"
    );
}

#[test]
fn inherited_parent_param_resolves_to_the_declaring_classes_parent() {
    // `parent` in `Mid::take` means `Base`, whatever class the call is made
    // on, so calling it on a `Leaf` must still accept a `Base` and must not
    // silently demand a `Mid`.
    let php = r#"<?php
class Base {}
class Other {}
class Mid extends Base {
    public function take(parent $p): void {}
}
class Leaf extends Mid {}
class Machine {
    public Leaf $leaf;
    public function go(): void {
        $this->leaf->take(new Base());
        $this->leaf->take(new Other());
    }
}
"#;
    let msgs = type_error_messages(&collect(php));
    assert_eq!(
        msgs,
        vec!["Argument 1 ($p) expects Base, got Other".to_string()],
        "an inherited `parent` must stay bound to Base, not to the owner's parent Mid"
    );
}

#[test]
fn no_diagnostic_for_static_param_with_matching_enum_case() {
    let php = r#"<?php
enum State: string {
    case A = 'a';
    case B = 'b';

    /** @param static $newState */
    public function canChangeTo($newState): bool { return true; }
}
class Machine {
    public State $state = State::A;
    public function go(): void {
        $this->state->canChangeTo(State::B);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag an enum case passed to a `static` parameter, got: {diags:?}"
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

#[test]
fn no_diagnostic_for_traversable_class_to_iterable() {
    let php = r#"<?php
interface Traversable {}
interface Iterator extends Traversable {}

class Bag implements Iterator {}

function takes_iterable(iterable $items): void {}

function test(Bag $bag): void {
    takes_iterable($bag);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "`iterable` is `array|Traversable`, got: {diags:?}"
    );
}

#[test]
fn flags_non_traversable_class_to_iterable() {
    let php = r#"<?php
interface Traversable {}

class Bag {}

function takes_iterable(iterable $items): void {}

function test(Bag $bag): void {
    takes_iterable($bag);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "A class with no Traversable in its hierarchy cannot be iterated"
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
        msg.contains("\"hello\""),
        "Message should mention the actual literal type, got: {msg}"
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

/// Every value in `[1, 1.5, '123']` is numeric, so a read off the array is
/// numeric whichever entry the key lands on. Widening the members to
/// `int|float|string` at construction loses that: a bare `string` cannot be
/// proven numeric, and a value that is numeric on every branch gets reported.
#[test]
fn no_diagnostic_for_a_dynamic_key_read_off_an_all_numeric_literal_array() {
    let php = r#"<?php
/** @param numeric $v */
function takes_numeric($v): void {}

function test(): void {
    $values = [1, 1.5, '123'];
    $key = array_rand($values);
    takes_numeric($values[$key]);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Every entry of the literal array is numeric, got: {}",
        type_error_messages(&diags).join("; ")
    );
}

/// The literal-key counterpart of the case above: the entry addressed by
/// `[2]` is the exact value written at that position.
#[test]
fn no_diagnostic_for_a_literal_key_read_off_an_all_numeric_literal_array() {
    let php = r#"<?php
/** @param numeric $v */
function takes_numeric($v): void {}

function test(): void {
    $values = [1, 1.5, '123'];
    takes_numeric($values[2]);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "The entry at index 2 is the numeric string '123', got: {}",
        type_error_messages(&diags).join("; ")
    );
}

/// A value written after construction says the array is being built up, so
/// the entries it can hold are no longer just the ones spelled out.
#[test]
fn a_push_after_construction_widens_the_stored_literal_values() {
    let php = r#"<?php
/** @param numeric $v */
function takes_numeric($v): void {}

function test(string $extra): void {
    $values = [1, 1.5, '123'];
    $values[] = $extra;
    $key = array_rand($values);
    takes_numeric($values[$key]);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "The pushed string is not provably numeric, got no diagnostic"
    );
}

/// A dynamic key over a string-keyed literal reads the union of the values
/// written against those keys, not the base types they belong to.
#[test]
fn no_diagnostic_for_a_dynamic_key_read_off_a_string_keyed_numeric_literal_array() {
    let php = r#"<?php
/** @param numeric $v */
function takes_numeric($v): void {}

function test(string $k): void {
    $values = ['a' => 1, 'b' => 1.5, 'c' => '123'];
    takes_numeric($values[$k]);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Every value written into the shape is numeric, got: {}",
        type_error_messages(&diags).join("; ")
    );
}

/// A constant table is as readable through a dynamic key as a local literal
/// is: the entries it names are fixed at the point it is declared.
#[test]
fn no_diagnostic_for_a_dynamic_key_read_off_a_numeric_constant_table() {
    let php = r#"<?php
/** @param numeric $v */
function takes_numeric($v): void {}

class Ids {
    const TABLE = [1, 1.5, '123'];
}

function test(string $k): void {
    takes_numeric(Ids::TABLE[$k]);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Every entry of the constant table is numeric, got: {}",
        type_error_messages(&diags).join("; ")
    );
}

/// A read that lands on a value the target cannot hold still reports, and
/// names the specific value that fails rather than its base type.
#[test]
fn a_dynamic_key_read_reports_the_literal_value_that_fails() {
    // Under strict_types=1 neither the float nor the numeric-string
    // literal coerce into `int`, so the union still has a genuine
    // mismatch to report (outside strict_types, PHP's weak-typing
    // coercion makes all three literals valid `int` arguments).
    let php = r#"<?php
declare(strict_types=1);

function takesInt(int $x): void {}

function test(): void {
    $values = [1, 1.5, '123'];
    $key = array_rand($values);
    takesInt($values[$key]);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("1|1.5|'123'"), "{messages:?}");
}

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
fn no_diagnostic_for_non_falsy_string_to_non_empty_string() {
    // `non-falsy-string` (and its Psalm synonym `truthy-string`) excludes
    // both `""` and `"0"`, so it is strictly narrower than
    // `non-empty-string`, which excludes only `""`. Passing one where the
    // other is expected is sound.
    let php = r#"<?php
/** @param non-empty-string $value */
function takes_non_empty_string(string $value): void {}

/** @param non-falsy-string $v */
function probe(string $v): void {
    takes_non_empty_string($v);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag non-falsy-string passed to non-empty-string param, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_truthy_string_to_non_empty_string() {
    let php = r#"<?php
/** @param non-empty-string $value */
function takes_non_empty_string(string $value): void {}

/** @param truthy-string $v */
function probe(string $v): void {
    takes_non_empty_string($v);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag truthy-string passed to non-empty-string param, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_truthy_string_to_non_falsy_string() {
    // truthy-string and non-falsy-string are synonyms.
    let php = r#"<?php
/** @param non-falsy-string $value */
function takes_non_falsy_string(string $value): void {}

/** @param truthy-string $v */
function probe(string $v): void {
    takes_non_falsy_string($v);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag truthy-string passed to non-falsy-string param, got: {diags:?}"
    );
}

/// The `non-empty-…` refinements are all inhabited by the literal `"0"`,
/// which is falsy, so each one stops at `non-empty-string` and none of them
/// satisfies `non-falsy-string`.
#[test]
fn flags_non_empty_refinements_passed_to_non_falsy_string() {
    for refinement in [
        "non-empty-string",
        "non-empty-literal-string",
        "non-empty-lowercase-string",
        "non-empty-uppercase-string",
    ] {
        let php = format!(
            r#"<?php
/** @param non-falsy-string $value */
function takes_non_falsy_string(string $value): void {{}}

/** @param non-empty-string $lenient */
function takes_non_empty_string(string $lenient): void {{}}

/** @param {refinement} $v */
function probe(string $v): void {{
    takes_non_empty_string($v);
}}
"#
        );
        let diags = collect(&php);
        assert!(
            !has_type_error(&diags),
            "{refinement} is a non-empty-string, got: {diags:?}"
        );

        let php = php.replace("takes_non_empty_string($v);", "takes_non_falsy_string($v);");
        let diags = collect(&php);
        assert!(
            has_type_error(&diags),
            "{refinement} admits \"0\", so it is not a non-falsy-string, got: {diags:?}"
        );
    }
}

/// A bare `string` might satisfy any of the value-shape string refinements
/// at runtime, so passing one where such a refinement is declared is a
/// MAYBE and stays silent. Every member of the family has to be listed for
/// that: one left out turns into a false positive on the first call site
/// that uses it. `class-string` and `interface-string` are the deliberate
/// exception, and are reported the way PHPStan reports them.
#[test]
fn no_diagnostic_for_bare_string_passed_to_any_string_refinement() {
    for refinement in [
        "non-empty-string",
        "numeric-string",
        "literal-string",
        "non-empty-literal-string",
        "lowercase-string",
        "non-empty-lowercase-string",
        "uppercase-string",
        "non-empty-uppercase-string",
        "truthy-string",
        "non-falsy-string",
        "callable-string",
        "trait-string",
        "enum-string",
    ] {
        let php = format!(
            r#"<?php
/** @param {refinement} $value */
function takes(string $value): void {{}}

function probe(string $v): void {{
    takes($v);
}}
"#
        );
        let diags = collect(&php);
        assert!(
            !has_type_error(&diags),
            "a bare string might be a {refinement} at runtime, got: {diags:?}"
        );
    }
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

// ─── Object shape vs concrete class / object literal ────────────────────────

#[test]
fn no_diagnostic_for_class_satisfying_object_shape() {
    let php = r#"<?php
final class Reading {
    public int $foo = 1;
}

/** @param object{foo: int} $shape */
function takesObjectShape(object $shape): void {}

function test(): void {
    takesObjectShape(new Reading());
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag a class with a matching public property against an object shape, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_object_literal_satisfying_object_shape() {
    let php = r#"<?php
/** @param object{foo: int} $shape */
function takesObjectShape(object $shape): void {}

function test(): void {
    takesObjectShape((object) ['foo' => 1]);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag an anonymous object literal with a matching field against an object shape, got: {diags:?}"
    );
}

#[test]
fn diagnostic_for_class_with_mistyped_property_against_object_shape() {
    let php = r#"<?php
final class Mistyped {
    public string $foo = 'one';
}

/** @param object{foo: int} $shape */
function takesObjectShape(object $shape): void {}

function test(): void {
    takesObjectShape(new Mistyped());
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Should flag a class whose property type doesn't match the object shape, got: {diags:?}"
    );
}

#[test]
fn diagnostic_for_class_missing_property_against_object_shape() {
    let php = r#"<?php
final class Unrelated {
    public int $bar = 1;
}

/** @param object{foo: int} $shape */
function takesObjectShape(object $shape): void {}

function test(): void {
    takesObjectShape(new Unrelated());
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Should flag a class that has no property matching the object shape, got: {diags:?}"
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
// Nullable arg → non-nullable param: reported, in either spelling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn nullable_arg_to_non_nullable_param_is_reported() {
    let php = r#"<?php
class Carbon {}

function takes_carbon(Carbon $c): void {}

function test(?Carbon $c): void {
    takes_carbon($c);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("?Carbon"),
        "the message should name the type passed, got {messages:?}"
    );
}

#[test]
fn nullable_string_to_string_is_reported() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(?string $s): void {
    takes_string($s);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

/// The same call written with the union spelling of the same type, which
/// used to be the only one of the two that was reported: which spelling a
/// value carries is an accident of how it was produced, so both have to
/// reach the same verdict.
#[test]
fn a_union_spelled_nullable_argument_reads_the_same_as_the_short_one() {
    let php = r#"<?php
function takes_string(string $s): void {}

/** @param string|null $s */
function test($s): void {
    takes_string($s);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

/// A guard the walker can follow leaves nothing to report, in either
/// spelling — the check is reading the flow rather than reporting every
/// nullable argument on sight.
#[test]
fn a_guarded_nullable_argument_is_not_reported() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(?string $s): void {
    if ($s === null) {
        return;
    }
    takes_string($s);
}

/** @param string|null $s */
function testUnion($s): void {
    if (!$s) {
        return;
    }
    takes_string($s);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A nullable argument whose parameter admits null as one of its union
/// members is fine, and was reported before `?T` was decomposed into the
/// union it stands for: the whole `?Color` matched no single member of
/// `Htmlable|BackedEnum|string|null`, while its halves match one each.
#[test]
fn a_nullable_argument_satisfies_a_union_parameter_that_admits_null() {
    let php = r#"<?php
interface Htmlable {}
enum Color: string { case Red = 'red'; }

function render(Htmlable|BackedEnum|string|null $value): void {}

function test(?Color $color): void {
    render($color);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

// ═══════════════════════════════════════════════════════════════════════════
// A `void` call hands back no value, so passing it on is reported
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_void_call_passed_as_an_argument_is_reported() {
    let php = r#"<?php
function log_it(string $message): void {}
function takes_string(string $s): void {}

function test(): void {
    takes_string(log_it("hi"));
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("void") && messages[0].contains("returns no value"),
        "the message should say the expression yields nothing, got {messages:?}"
    );
}

#[test]
fn a_void_method_call_passed_as_an_argument_is_reported() {
    let php = r#"<?php
class Logger {
    /** @return void */
    public function write(string $message) {}
}

function takes_string(string $s): void {}

function test(Logger $logger): void {
    takes_string($logger->write("hi"));
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

/// A `void` argument is wrong whatever the parameter accepts: PHP 8 passes
/// the `null` it substitutes, so a nullable parameter hides the mistake
/// rather than excusing it.
#[test]
fn a_void_call_is_reported_against_a_nullable_parameter() {
    let php = r#"<?php
function log_it(): void {}
function takes_nullable(?string $s): void {}

function test(): void {
    takes_nullable(log_it());
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

/// `never` is not `void`: a call that never returns hands the parameter no
/// value because the program does not get that far, which is sound for any
/// parameter type.
#[test]
fn a_never_returning_call_passed_as_an_argument_is_not_reported() {
    let php = r#"<?php
function fail(string $message): never { throw new RuntimeException($message); }
function takes_string(string $s): void {}

function test(): void {
    takes_string(fail("boom"));
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Nothing produces a value of type `void`, so a parameter annotated with
/// it is the annotation being wrong, not the argument.
#[test]
fn a_void_parameter_annotation_does_not_report_its_arguments() {
    let php = r#"<?php
/** @param void $x */
function takes_void($x): void {}

function test(): void {
    takes_void("hi");
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
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

#[test]
fn no_diagnostic_for_static_return_stringable_to_string() {
    let php = r#"<?php
class Node implements Stringable {
    public function __get($name): static {
        return $this;
    }
    public function __toString(): string { return ''; }
}

function test(): void {
    $n = new Node();
    throw new \Exception($n->Body);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag static(Stringable) object passed to string, got: {diags:?}"
    );
}

#[test]
fn type_error_for_stringable_to_string_under_strict_types() {
    let php = r#"<?php
declare(strict_types=1);

class Name {
    public function __toString(): string { return 'x'; }
}

function takes_string(string $value): void {}

function test(Name $name): void {
    takes_string($name);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected type error for Stringable object passed to string under strict_types=1, got: {diags:?}"
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
fn no_diagnostic_for_list_of_shapes_to_array_int_of_generic_arrays() {
    let php = r#"<?php
/** @param array<int, array<string, mixed>> $rows */
function takes_rows(array $rows): void {}

function test(): void {
    $rows = [];
    $rows[] = ['item' => 'a', 'qty' => 1];
    takes_rows($rows);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag a list of array shapes passed to \
         array<int, array<string, mixed>>, got: {diags:?}"
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

#[test]
fn no_diagnostic_for_string_literal_naming_subclass() {
    let php = r#"<?php
class Animal {}
class Cat extends Animal {}

/** @param class-string<Animal> $cls */
function takes_animal_class(string $cls): void {}

function test(): void {
    takes_animal_class('Cat');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string literal 'Cat' passed to class-string<Animal>, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_string_literal_naming_bound_class_itself() {
    let php = r#"<?php
class Animal {}

/** @param class-string<Animal> $cls */
function takes_animal_class(string $cls): void {}

function test(): void {
    takes_animal_class('Animal');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string literal 'Animal' passed to class-string<Animal>, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_unresolvable_string_literal_class_string() {
    let php = r#"<?php
class Animal {}

/** @param class-string<Animal> $cls */
function takes_animal_class(string $cls): void {}

function test(): void {
    takes_animal_class('SomeUnknownClass');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should stay silent when the string literal can't be resolved to a class, got: {diags:?}"
    );
}

#[test]
fn flags_string_literal_naming_unrelated_class() {
    let php = r#"<?php
class Animal {}
class Vehicle {}

/** @param class-string<Animal> $cls */
function takes_animal_class(string $cls): void {}

function test(): void {
    takes_animal_class('Vehicle');
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Should flag string literal 'Vehicle' passed to class-string<Animal>, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_escaped_backslash_string_literal_naming_subclass() {
    // `'App\\Vehicle'` is the single-quoted spelling of the runtime value
    // `App\Vehicle` (PHP decodes `\\` to `\`). The literal's decoded content,
    // not its source spelling, must resolve to the class.
    let php = r#"<?php
namespace App;

class Animal {}
class Vehicle extends Animal {}

/** @param class-string<\App\Animal> $cls */
function takes_animal_class(string $cls): void {}

function test(): void {
    takes_animal_class('App\\Vehicle');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag 'App\\\\Vehicle' passed to class-string<App\\Animal>, got: {diags:?}"
    );
}

#[test]
fn flags_escaped_backslash_string_literal_naming_unrelated_class() {
    let php = r#"<?php
namespace App;

class Animal {}
class Vehicle {}

/** @param class-string<\App\Animal> $cls */
function takes_animal_class(string $cls): void {}

function test(): void {
    takes_animal_class('App\\Vehicle');
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Should flag 'App\\\\Vehicle' passed to class-string<App\\Animal> once the escaped literal is resolved, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_union_of_class_strings_bound_by_template() {
    // Regression: `$className` iterated over a class-constant array is a
    // union of `class-string`s.  Passing it to a `@template T of Bound`
    // parameter typed `class-string<T>` must bind `T` to the union of the
    // inner classes (checking each against the bound), not fall back to the
    // declared bound.  When the bound is a short name imported from another
    // namespace, that fallback produces a false positive because the short
    // bound name cannot be resolved to its FQN during the hierarchy walk.
    use crate::common::create_psr4_workspace;

    let files = vec![
        (
            "src/Models/Models.php",
            r#"<?php
namespace App\Models;

abstract class AbstractEntity {}
abstract class AbstractRenderable extends AbstractEntity {}
class Page extends AbstractRenderable {}
class CustomPage extends AbstractEntity {}
"#,
        ),
        (
            "src/Services/OrmService.php",
            r#"<?php
namespace App\Services;

use App\Models\AbstractEntity;

class OrmService {
    /**
     * @template T of AbstractEntity
     * @param class-string<T> $class
     * @return T[]
     */
    public function getByQuery(string $class, string $query): array { return []; }
}
"#,
        ),
        (
            "src/Http/ExplorerController.php",
            r#"<?php
namespace App\Http;

use App\Models\CustomPage;
use App\Models\Page;
use App\Services\OrmService;

class ExplorerController {
    public function run(OrmService $orm): void {
        foreach ([Page::class, CustomPage::class] as $className) {
            $rows = $orm->getByQuery($className, 'SELECT 1');
        }
    }
}
"#,
        ),
    ];

    let composer = r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#;
    let (backend, dir) = create_psr4_workspace(composer, &files);
    // Index every file up front, mirroring the CLI's eager scan so the
    // call target and its `@template` docblock are available.
    for (rel_path, php) in &files {
        let file_uri = format!("file://{}/{}", dir.path().display(), rel_path);
        backend.update_ast(&file_uri, php);
    }
    let uri = format!(
        "file://{}/src/Http/ExplorerController.php",
        dir.path().display()
    );
    let content = files[2].1;
    let mut diags = Vec::new();
    backend.collect_argument_type_diagnostics(&uri, content, &mut diags);
    assert!(
        !has_type_error(&diags),
        "Should not flag a union of class-strings satisfying a template bound, got: {}",
        type_error_messages(&diags).join(", ")
    );
}

#[test]
fn no_diagnostic_for_class_string_union_param_bound_by_template() {
    // A variable declared `class-string<A|B>` passed to a `class-string<T>`
    // template parameter must bind `T` to the whole union `A|B`, not truncate
    // it to the first member.  Truncation produced a bogus "expects
    // class-string<A>, got A|B" mismatch.
    let php = r#"<?php
class FunctionNode {}
class MethodNode {}

class Builder {
    /**
     * @template T of object
     * @param class-string<T> $className
     * @return T
     */
    public function build(string $className): object { return new $className(); }
}

/** @param class-string<FunctionNode|MethodNode> $mockBuilder */
function demo(Builder $b, string $mockBuilder): void
{
    $b->build($mockBuilder);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag class-string<A|B> passed to class-string<T>, got: {}",
        type_error_messages(&diags).join(", ")
    );
}

#[test]
fn no_diagnostic_for_string_literal_class_name_expect_exception() {
    let php = r#"<?php
class TestCase {
    /** @param class-string<\Throwable> $exception */
    function expectException(string $exception): void {}

    function test(): void {
        $this->expectException('RuntimeException');
    }
}
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag 'RuntimeException' passed to class-string<Throwable>, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_string_literal_binding_class_string_template() {
    // A string literal naming a class must bind the template `T` to the
    // class it names, not to the literal's own `string` type. Binding to
    // `string` would produce the absurd `class-string<string>` parameter
    // and a guaranteed mismatch against the literal argument.
    let php = r#"<?php
class TestCase {
    /**
     * @template T
     * @param class-string<T> $expected
     * @param mixed $actual
     */
    function assertInstanceOf(string $expected, $actual): void {}

    function test(): void {
        $this->assertInstanceOf('Iterator', $this);
    }
}
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag string literal 'Iterator' bound through class-string<T>, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_class_const_binding_class_string_template() {
    // The `::class` argument path must keep binding `T` to the named
    // class, matching the string-literal path.
    let php = r#"<?php
class Animal {}

/**
 * @template T
 * @param class-string<T> $expected
 */
function assert_class(string $expected): void {}

function test(): void {
    assert_class(Animal::class);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Animal::class bound through class-string<T>, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_class_const_binding_bare_template() {
    // A template param bound directly from a `::class` argument
    // (`@param T $expected`, no `class-string<>` wrapper) must infer
    // `class-string<T>` — the argument's actual type — not the bare
    // class name. Binding to the bare class produces a spurious
    // "expects Carbon, got class-string<Carbon>" mismatch because the
    // parameter type is compared against the very argument that bound
    // it. This is the `Mockery::type(SomeClass::class)` pattern.
    let php = r#"<?php
class Carbon {}

class Matcher {
    /**
     * @template TExpectedType
     * @param TExpectedType $expected
     */
    public static function type($expected): void {}
}

function test(): void {
    Matcher::type(Carbon::class);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Carbon::class bound through bare @param T, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_class_const_binding_bare_template_function() {
    // The same self-referential binding for a function-level template.
    let php = r#"<?php
class Carbon {}

/**
 * @template TExpectedType
 * @param TExpectedType $expected
 */
function matcher($expected): void {}

function test(): void {
    matcher(Carbon::class);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag Carbon::class bound through bare @param T function, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_sibling_class_strings_passed_to_static_bound() {
    let php = r#"<?php
/**
 * @phpstan-type Input array{
 *     type: 'S1'|'S2'|'S3',
 *     data: array<string, mixed>
 * }
 */
abstract class AClass
{
    /**
     * @param Input $data
     * @return array<string, mixed>
     */
    public static function foo(array $data): array
    {
        return match ($data["type"]) {
            "S1" => static::bar(SClass1::class, $data["data"]),
            "S2" => static::bar(SClass2::class, $data["data"]),
            "S3" => static::bar(SClass3::class, $data["data"]),
        };
    }

    /**
     * @param class-string<static> $class
     * @param array<string, mixed> $data
     * @return array<string, mixed>
     */
    private static function bar(string $class, array $data): array
    {
        return $data;
    }
}

final class SClass1 extends AClass {}
final class SClass2 extends AClass {}
final class SClass3 extends AClass {}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag sibling class-string constants passed to class-string<static>, got: {diags:?}"
    );
}

#[test]
fn diagnose_unrelated_class_string_passed_to_static_bound() {
    let php = r#"<?php
abstract class Base273 {
    /** @param class-string<static> $class */
    private static function make(string $class): void {}

    public static function run(): void {
        static::make(Unrelated273::class);
    }
}

final class Child273 extends Base273 {}
final class Unrelated273 {}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Should flag unrelated class-string passed to class-string<static>"
    );
}

#[test]
fn no_diagnostic_for_child_class_string_via_explicit_call() {
    let php = r#"<?php
abstract class Base273b {
    /** @param class-string<static> $class */
    public static function make(string $class): void {}
}

final class Child273b extends Base273b {}

function test273b(): void {
    Base273b::make(Child273b::class);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag child class-string passed to class-string<static> via explicit call, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_self_class_string_to_static_bound() {
    let php = r#"<?php
abstract class Base273c {
    /** @param class-string<static> $class */
    private static function make(string $class): void {}

    public static function run(): void {
        self::make(static::class);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Should not flag static::class passed to class-string<static> via self:: call, got: {diags:?}"
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
fn no_false_positive_for_self_assert_same_with_untyped_class_const() {
    // Real-world PHPUnit pattern: static::assertSame(Command::INVALID, $x)
    // where INVALID is an *untyped* class constant with an int value.
    // The template param must bind to the constant's value type (int),
    // not the constant's owning class (Command).
    let php = r#"<?php
class Command {
    const SUCCESS = 0;
    const INVALID = 23;
}

class TestCase {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     */
    final public static function assertSame(mixed $expected, mixed $actual, string $message = ''): void {}
}

class MyTest extends TestCase {
    public function testExitCode(): void {
        $exitCode = 23;
        static::assertSame(Command::INVALID, $exitCode);
        self::assertSame(Command::SUCCESS, 0);
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        !has_type_error(&diags),
        "Untyped class constant argument should bind the template param to \
         its value type (int), not the owning class, got: {msgs:?}"
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

#[test]
fn no_false_positive_when_template_argument_is_its_own_sole_binding_site() {
    // A template param that is bound only by the parameter being
    // checked is circular: the substituted parameter type came from
    // resolving that exact argument, so comparing the argument to it
    // again — through a resolver that may disagree with the one used
    // for template binding — can only produce a false positive (see
    // bugs.md B4). This mirrors the real-world PHPUnit pattern
    // `assertSame(url('/login'), $x)`, where `url()`'s conditional
    // return type resolves differently for the two passes: the
    // template-binding pass sees `UrlGenerator`, the argument-type pass
    // sees `string`.
    let php = r#"<?php
class UrlGenerator {}

/**
 * @return ($path is null ? UrlGenerator : string)
 */
function url(?string $path = null): UrlGenerator|string {}

class TestCase {
    /**
     * @template ExpectedType
     * @param ExpectedType $expected
     */
    final public static function assertSame(mixed $expected, mixed $actual, string $message = ''): void {}
}

class MyTest extends TestCase {
    public function testLogin(): void {
        self::assertSame(url('/login'), 'https://example.test/login');
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        !has_type_error(&diags),
        "Should not flag an argument against a parameter type substituted from that \
         same argument, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_when_the_other_binding_site_was_not_passed() {
    // Laravel's `travelTo` names `TDate` in both `$date` and the optional
    // `$callback`'s callable signature. A call that passes only the date
    // binds `TDate` from that one argument, so checking the argument
    // against the substitution is exactly as circular as it would be if
    // `$callback` did not exist — the second binding site contributes
    // nothing when the caller leaves it out.
    let php = r#"<?php
class Carbon {
    public static function create(int $year): ?Carbon { return null; }
}

class TestCase {
    /**
     * @template TReturn of mixed
     * @template TDate of \DateTimeInterface|\Closure|Carbon|string|bool|null
     * @param  TDate  $date
     * @param  (callable(TDate): TReturn)|null  $callback
     * @return ($callback is null ? void : TReturn)
     */
    public function travelTo($date, $callback = null) {}
}

class MyTest extends TestCase {
    public function testTime(): void {
        $this->travelTo(Carbon::create(2024));
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        !has_type_error(&diags),
        "The unfilled second binding site is not an independent check, got: {msgs:?}"
    );
}

#[test]
fn skipping_a_self_bound_parameter_leaves_the_rest_of_the_call_checked() {
    // Only the parameter the template was bound from goes unchecked. The
    // ordinary parameters beside it are unaffected.
    let php = r#"<?php
class Carbon {
    public static function create(int $year): ?Carbon { return null; }
}

class TestCase {
    /**
     * @template TDate of Carbon|string
     * @param  TDate  $date
     * @param  (callable(TDate): void)|null  $callback
     */
    public function travelTo($date, $callback = null, int $times = 1) {}
}

class MyTest extends TestCase {
    public function testTime(): void {
        $this->travelTo(Carbon::create(2024), null, 'not an int');
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "a string argument does not satisfy the `int $times` parameter"
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
fn flags_interface_arg_to_concrete_param() {
    // CarbonInterface passed where Carbon is expected: any other
    // implementation of the interface would be a type error, so the
    // downcast is reported rather than assumed to work out.
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
        has_type_error(&diags),
        "Should flag interface arg passed to concrete param, got: {diags:?}"
    );
}

#[test]
fn flags_parent_arg_to_child_param() {
    // Parent class passed where a child is expected.  Nothing proves the
    // value is the child, so the downcast is reported.
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
        has_type_error(&diags),
        "Should flag parent arg to child param, got: {diags:?}"
    );
}

/// An `instanceof` check ahead of the call proves the narrower type, and
/// then the same call is fine.
#[test]
fn no_diagnostic_for_parent_arg_narrowed_by_instanceof() {
    let php = r#"<?php
class Animal {}
class Cat extends Animal {}

function takes_cat(Cat $c): void {}

function test(Animal $a): void {
    if ($a instanceof Cat) {
        takes_cat($a);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "instanceof-narrowed parent should satisfy the child param, got: {diags:?}"
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
fn flags_non_final_parent_where_sibling_subclass_expected() {
    // A value typed as the shared parent could be either sibling, so
    // passing it where one of them is declared is a downcast.
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
        has_type_error(&diags),
        "Should flag parent passed where a sibling subclass is declared, got: {diags:?}"
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

// ─── Nullability-only argument mismatches (downgrade-nullable-argument-mismatch) ───

#[test]
fn negated_and_guard_leaves_null_reachable_as_an_error_by_default() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(mixed $password): void {
    if ($password !== null && ! is_string($password)) {
        throw new \Exception('bad');
    }
    takes_string($password);
}
"#;
    let diags = collect(php);
    let messages = type_error_messages(&diags);
    assert_eq!(messages.len(), 1, "got {diags:?}");
    assert!(
        messages[0].contains("null does not satisfy string"),
        "expected a nullability-only mismatch, got: {messages:?}"
    );
    let type_error = diags
        .iter()
        .find(|d| has_type_error(std::slice::from_ref(d)))
        .unwrap();
    assert_eq!(
        type_error.severity,
        Some(DiagnosticSeverity::ERROR),
        "default severity must stay ERROR so existing projects see no change"
    );
}

#[test]
fn nullability_only_mismatch_can_be_downgraded_to_a_warning() {
    let backend = create_test_backend();
    let uri = "file:///nullable_downgrade.php";
    let php = r#"<?php
function takes_string(string $s): void {}

function test(mixed $password): void {
    if ($password !== null && ! is_string($password)) {
        throw new \Exception('bad');
    }
    takes_string($password);
}
"#;
    {
        let mut cfg = backend.config();
        cfg.diagnostics.downgrade_nullable_argument_mismatch = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);
    let mut diags = Vec::new();
    backend.collect_argument_type_diagnostics(uri, php, &mut diags);
    let type_errors: Vec<&Diagnostic> = diags
        .iter()
        .filter(|d| has_type_error(std::slice::from_ref(d)))
        .collect();
    assert_eq!(type_errors.len(), 1, "got {diags:?}");
    assert_eq!(type_errors[0].severity, Some(DiagnosticSeverity::WARNING));
}

#[test]
fn a_declared_nullable_argument_is_the_same_gap_as_the_union_spelling() {
    let backend = create_test_backend();
    let uri = "file:///nullable_downgrade_declared.php";
    let php = r#"<?php
function takes_string(string $s): void {}

function test(?string $a, string|null $b, ?array $c): void {
    takes_string($a);
    takes_string($b);
    takes_string($c);
}
"#;
    {
        let mut cfg = backend.config();
        cfg.diagnostics.downgrade_nullable_argument_mismatch = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);
    let mut diags = Vec::new();
    backend.collect_argument_type_diagnostics(uri, php, &mut diags);
    let severities: Vec<Option<DiagnosticSeverity>> = diags
        .iter()
        .filter(|d| has_type_error(std::slice::from_ref(d)))
        .map(|d| d.severity)
        .collect();
    assert_eq!(
        severities,
        vec![
            Some(DiagnosticSeverity::WARNING),
            Some(DiagnosticSeverity::WARNING),
            // `?array` fails on more than its nullability.
            Some(DiagnosticSeverity::ERROR),
        ],
        "got {diags:?}"
    );
}

#[test]
fn a_declared_nullable_argument_stays_an_error_by_default() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(?string $a): void {
    takes_string($a);
}
"#;
    let diags = collect(php);
    let type_errors: Vec<&Diagnostic> = diags
        .iter()
        .filter(|d| has_type_error(std::slice::from_ref(d)))
        .collect();
    assert_eq!(type_errors.len(), 1, "got {diags:?}");
    assert_eq!(type_errors[0].severity, Some(DiagnosticSeverity::ERROR));
}

#[test]
fn a_genuine_type_mismatch_stays_an_error_even_when_downgrade_is_enabled() {
    let backend = create_test_backend();
    let uri = "file:///nullable_downgrade_control.php";
    let php = r#"<?php
function takes_string(string $s): void {}

function test(array|string $val): void {
    takes_string($val);
}
"#;
    {
        let mut cfg = backend.config();
        cfg.diagnostics.downgrade_nullable_argument_mismatch = Some(true);
        backend.set_config(cfg);
    }
    backend.update_ast(uri, php);
    let mut diags = Vec::new();
    backend.collect_argument_type_diagnostics(uri, php, &mut diags);
    let type_errors: Vec<&Diagnostic> = diags
        .iter()
        .filter(|d| has_type_error(std::slice::from_ref(d)))
        .collect();
    assert_eq!(type_errors.len(), 1, "got {diags:?}");
    assert_eq!(type_errors[0].severity, Some(DiagnosticSeverity::ERROR));
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

#[test]
fn no_diagnostic_null_init_foreach_untyped_array_is_null_guard() {
    // Issue #252: untyped foreach iterable must still seed the loop
    // value as mixed so `$x = $value` after `$x = null` is not a no-op,
    // and `if (is_null($x)) return;` can strip null before the call.
    let php = r#"<?php
function non_nullable(int $y): void {}

function x($array): void {
    $x = null;

    foreach ($array as $value) {
        $x = $value;
    }

    if (is_null($x)) {
        return;
    }

    non_nullable($x);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "untyped foreach + is_null early return should not leave $x as null, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_null_init_foreach_typed_array_is_null_guard() {
    let php = r#"<?php
function non_nullable(int $y): void {}

function x(array $array): void {
    $x = null;

    foreach ($array as $value) {
        $x = $value;
    }

    if (is_null($x)) {
        return;
    }

    non_nullable($x);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "typed array foreach + is_null early return control case failed, got: {diags:?}"
    );
}

// ─── Foreach variable reassignment should not leak into RHS ─────────────────

#[test]
fn no_diagnostic_for_dim_write_to_foreach_value_variable() {
    // The foreach rebinds $step to a fresh element every iteration, so
    // the `$step['fo'] = ...` write at the bottom of the body is dead
    // state by the time the next iteration starts.  When it leaked
    // through the back-edge the `is_string()` guard could no longer
    // narrow the ternary's true arm to string.
    let php = r#"<?php
function takes_string(string $s): void {}

function normalize(mixed $steps): void {
    if (is_array($steps)) {
        foreach ($steps as $step) {
            if (!is_array($step)) {
                throw new \RuntimeException('not array');
            }
            $raw = isset($step['fo']) && is_string($step['fo']) ? $step['fo'] : '{}';
            takes_string($raw);
            $step['fo'] = [1, 2];
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A dim-write to the foreach value variable must not leak into the \
         next iteration's binding, got: {diags:?}"
    );
}

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
fn flags_float_passed_to_int_under_strict_types() {
    let php = r#"<?php
declare(strict_types=1);

function takes_int(int $x): void {}

function test(): void {
    $f = 1.5;
    takes_int($f);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Expected a type error for float passed to int under strict_types=1, got: {diags:?}"
    );
}

#[test]
fn no_strict_types_allows_float_passed_to_int() {
    // Outside declare(strict_types=1), PHP coerces (truncates) a float
    // argument passed where int is expected instead of raising a
    // TypeError, so there is nothing to flag.
    let php = r#"<?php
function takes_int(int $x): void {}

function test(): void {
    $f = 1.5;
    takes_int($f);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Without strict_types, float passed to int should be allowed, got: {diags:?}"
    );
}

// ─── int / int reaching an int parameter ────────────────────────────────────

/// `int / int` is `int|float`, but the float half only turns up when the
/// division does not come out even, which is a property of the values and
/// not of the operands' types.  Enforcing the whole union would flag every
/// `$total / $count` handed to an `int` parameter, so the union the
/// operator produces is benevolent and one branch fitting is enough.
#[test]
fn strict_types_allows_int_division_passed_to_int() {
    let php = r#"<?php
declare(strict_types=1);

function takes_int(int $x): void {}

function test(int $total, int $count): void {
    takes_int($total / $count);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "int/int passed to an int parameter should be allowed under strict_types, got: {diags:?}"
    );
}

/// The same through a variable, since the marker has to survive being
/// stored in the scope and read back out.
#[test]
fn strict_types_allows_int_division_through_a_variable() {
    let php = r#"<?php
declare(strict_types=1);

function takes_int(int $x): void {}

function test(int $total, int $count): void {
    $per = $total / $count;
    takes_int($per);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "An int/int result held in a variable should reach an int parameter, got: {diags:?}"
    );
}

/// …and through `/=`, which infers its result the same way.
#[test]
fn strict_types_allows_int_division_assignment_operator() {
    let php = r#"<?php
declare(strict_types=1);

function takes_int(int $x): void {}

function test(int $total, int $count): void {
    $total /= $count;
    takes_int($total);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "An int/int result built with /= should reach an int parameter, got: {diags:?}"
    );
}

/// The benevolence belongs to the division operator, not to unions at
/// large.  A declared `int|float` still has to satisfy the parameter with
/// every member, which is what keeps the union rule worth having.
#[test]
fn strict_types_flags_declared_int_float_union_passed_to_int() {
    let php = r#"<?php
declare(strict_types=1);

function takes_int(int $x): void {}

/** @param int|float $v */
function test($v): void {
    takes_int($v);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "A declared int|float passed to an int parameter should be flagged, got: {diags:?}"
    );
}

/// A division whose operand is *already* `int|float` inherits that union
/// rather than inventing one, so it stays strict.
#[test]
fn strict_types_flags_division_of_an_int_float_union_passed_to_int() {
    let php = r#"<?php
declare(strict_types=1);

function takes_int(int $x): void {}

/** @param int|float $v */
function test($v, int $count): void {
    takes_int($v / $count);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Dividing a declared int|float should stay strict, got: {diags:?}"
    );
}

// ─── int ** int reaching an int parameter ───────────────────────────────────

/// `int ** int` is `int|float`, since PHP promotes the result to a float
/// on overflow (`2 ** 64`) or a negative exponent (`2 ** -1`) — a property
/// of the values rather than of the operands' types, so the union is
/// benevolent the same way `int / int` is.
#[test]
fn strict_types_allows_int_exponentiation_passed_to_int() {
    let php = r#"<?php
declare(strict_types=1);

function takes_int(int $x): void {}

function test(int $base, int $exp): void {
    takes_int($base ** $exp);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "int**int passed to an int parameter should be allowed under strict_types, got: {diags:?}"
    );
}

/// …and through `**=`, which infers its result the same way.
#[test]
fn strict_types_allows_int_exponentiation_assignment_operator() {
    let php = r#"<?php
declare(strict_types=1);

function takes_int(int $x): void {}

function test(int $base, int $exp): void {
    $base **= $exp;
    takes_int($base);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "An int**int result built with **= should reach an int parameter, got: {diags:?}"
    );
}

/// An exponentiation whose operand is *already* `int|float` inherits that
/// union rather than inventing one, so it stays strict.
#[test]
fn strict_types_flags_exponentiation_of_an_int_float_union_passed_to_int() {
    let php = r#"<?php
declare(strict_types=1);

function takes_int(int $x): void {}

/** @param int|float $v */
function test($v, int $exp): void {
    takes_int($v ** $exp);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "Exponentiating a declared int|float should stay strict, got: {diags:?}"
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

// ── Generic type-argument mismatches at call sites ──────────────────

#[test]
fn diagnostic_for_mismatched_generic_type_argument() {
    // `Box<string>` (inferred from the constructor argument) is not a
    // `Box<int>`, and the nominal `Box` <: `Box` fallback must not
    // rescue it.  `strict_types` because without it a `string` type
    // argument is still a candidate for PHP's int coercion.
    let php = r#"<?php
declare(strict_types=1);

/** @template T */
final class Box {
    /** @param T $value */
    public function __construct(public mixed $value) {}
}

/** @param Box<int> $box */
function takesIntBox(Box $box): void {}

takesIntBox(new Box(1));
takesIntBox(new Box('x'));
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one type error for Box<string> where Box<int> is required, got: {msgs:?}"
    );
    assert!(
        msgs[0].contains("Box<int>") && msgs[0].contains("Box<string>"),
        "Message should name both generic types, got: {msgs:?}"
    );
}

#[test]
fn diagnostic_for_generic_type_argument_outside_template_bound() {
    // `@template T of HasName` makes `NamedBox<AnonymousUser>` an
    // incompatible argument where `NamedBox<User>` is required.
    let php = r#"<?php
interface HasName {
    public function name(): string;
}

final class User implements HasName {
    public function name(): string { return 'user'; }
}

final class AnonymousUser {}

/** @template T of HasName */
final class NamedBox {
    /** @param T $value */
    public function __construct(public object $value) {}
}

/** @param NamedBox<User> $box */
function takesNamedBox(NamedBox $box): void {}

takesNamedBox(new NamedBox(new User()));
takesNamedBox(new NamedBox(new AnonymousUser()));
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one type error for NamedBox<AnonymousUser>, got: {msgs:?}"
    );
}

#[test]
fn no_diagnostic_for_covariant_generic_type_argument() {
    // A narrower type argument still satisfies the wider one.
    let php = r#"<?php
class Animal {}
class Cat extends Animal {}

/** @template T */
final class Box {
    /** @param T $value */
    public function __construct(public mixed $value) {}
}

/** @param Box<Animal> $box */
function takesAnimalBox(Box $box): void {}

takesAnimalBox(new Box(new Cat()));
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Box<Cat> should satisfy Box<Animal>, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_generic_argument_with_unknown_type_argument() {
    // An unparameterised `Box` says nothing about its type argument,
    // so it must not be reported against `Box<int>`.
    let php = r#"<?php
/** @template T */
final class Box {
    /** @param T $value */
    public function __construct(public mixed $value) {}
}

/** @param Box<int> $box */
function takesIntBox(Box $box): void {}

/** @param Box $box */
function forward(Box $box): void {
    takesIntBox($box);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A bare Box must stay silent against Box<int>, got: {diags:?}"
    );
}

#[test]
fn diagnostic_for_mismatched_generic_type_argument_without_strict_types() {
    // A type argument is not a coercion site: PHP converts a `string`
    // passed to an `int` parameter, but never the `T` inside a `Box<T>`
    // handed over whole.  So the mismatch is reported whether or not the
    // file declared `strict_types`.
    let php = r#"<?php
/** @template T */
final class Box {
    /** @param T $value */
    public function __construct(public mixed $value) {}
}

/** @param Box<int> $box */
function takesIntBox(Box $box): void {}

function f(): void {
    /** @var Box<string> $box */
    $box = new Box('x');
    takesIntBox($box);
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one type error for Box<string> where Box<int> is required, got: {msgs:?}"
    );
    assert!(
        msgs[0].contains("Box<int>") && msgs[0].contains("Box<string>"),
        "Message should name both generic types, got: {msgs:?}"
    );
}

#[test]
fn diagnostic_for_mismatched_generic_type_argument_in_both_directions() {
    // The coercion leniency ran both ways without `strict_types`, so an
    // `int` type argument reaching a `string` one has to be reported too.
    let php = r#"<?php
/** @template T */
final class Box {
    /** @param T $value */
    public function __construct(public mixed $value) {}
}

/** @param Box<string> $box */
function takesStringBox(Box $box): void {}

takesStringBox(new Box(1));
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one type error for Box<int> where Box<string> is required, got: {msgs:?}"
    );
}

#[test]
fn no_diagnostic_for_template_bound_from_two_generic_parameters() {
    // `T` is bound at two sites, so it is what both arguments have in
    // common — neither may be measured against the other's type.
    let php = r#"<?php
declare(strict_types=1);

/** @template-covariant T */
interface Wrap {}

/** @return Wrap<int> */
function wrapInt(): Wrap {}

/** @return Wrap<string> */
function wrapString(): Wrap {}

/**
 * @template T
 * @param Wrap<T> $first
 * @param Wrap<T> $second
 */
function combine(Wrap $first, Wrap $second): void {}

combine(wrapInt(), wrapString());
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A multi-bound template must not measure one argument against another, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_for_template_bound_from_two_array_parameters() {
    // `T[]` at two binding sites makes `T` the union of both arguments'
    // element types, so `[1, 2]` must not be measured against the
    // `string` taken from its sibling.
    let php = r#"<?php
declare(strict_types=1);

/**
 * @template T
 * @param T[] $first
 * @param T[] $second
 */
function combine(array $first, array $second): void {}

combine([1, 2], ['a', 'b']);

/**
 * @template T
 * @param array<T> $first
 * @param array<T> $second
 */
function combineWrapped(array $first, array $second): void {}

combineWrapped([1, 2], ['a', 'b']);
combineWrapped([], ['a', 'b']);
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A template bound from two array parameters must union both element types, got: {diags:?}"
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
    backend.collect_argument_type_diagnostics(&uri, content, &mut diags);
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
    backend.collect_argument_type_diagnostics("file:///test.php", php, &mut out);
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

interface Unrelated {}

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

#[test]
fn no_false_positive_for_two_level_property_narrowed_via_instanceof() {
    // The declared class of a two-level path must survive the check the
    // same way a one-level path's does, so the value stays both types.
    let php = r#"<?php
interface MockInterface {
    public function shouldReceive(string $name): self;
}

class EpaymentService {
    public function annul(): bool { return true; }
}

class Holder {
    public EpaymentService $service;
}

class TestCase {
    private Holder $holder;

    protected function mockMethod(MockInterface $mock, string $method): void {}

    protected function realMethod(EpaymentService $service): void {}

    public function test(): void {
        if ($this->holder->service instanceof MockInterface) {
            $x = $this->holder->service;
            $this->mockMethod($x, 'annul');
            $this->realMethod($x);
        }
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "A two-level property narrowed via instanceof keeps its declared class, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_call_narrowed_via_instanceof() {
    // The check is written on the call itself, and the argument repeats
    // that call verbatim.  Both spellings share a narrowing key, so the
    // argument is measured against the narrowed type rather than the
    // method's declared return type.
    let php = r#"<?php
interface MockInterface {
    public function shouldReceive(string $name): self;
}

class EpaymentService {
    public function annul(): bool { return true; }
}

class TestCase {
    protected function service(): EpaymentService {}

    protected function mockMethod(MockInterface $mock, string $method): void {}

    protected function realMethod(EpaymentService $service): void {}

    public function test(): void {
        if ($this->service() instanceof MockInterface) {
            $this->mockMethod($this->service(), 'annul');
            $this->realMethod($this->service());
        }
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Call narrowed via instanceof should be accepted as MockInterface, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_compound_and_instanceof_narrowing() {
    // `$x instanceof Aye && $x instanceof Bee` proves both at once, so
    // `$x` is `Aye&Bee`.  Joining the two as `Aye|Bee` instead judged
    // the value against `Bee` as well and rejected it.
    let php = r#"<?php
interface Aye { public function a(): void; }
interface Bee { public function b(): void; }

function wantsAye(Aye $x): void {}

function test(object $thing): void {
    if ($thing instanceof Aye && $thing instanceof Bee) {
        wantsAye($thing);
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "A subject proven to be both classes should satisfy either one, got: {msgs:?}"
    );
}

#[test]
fn compound_or_instanceof_narrowing_stays_a_union() {
    // The `||` counterpart proves only that the value is one of the two,
    // so passing it where just one is accepted must still be reported.
    let php = r#"<?php
interface Aye { public function a(): void; }
interface Bee { public function b(): void; }

function wantsAye(Aye $x): void {}

function test(object $thing): void {
    if ($thing instanceof Aye || $thing instanceof Bee) {
        wantsAye($thing);
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "An `||`-narrowed subject is still a union, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_property_narrowed_by_compound_and_instanceof() {
    // The property-path re-walk reaches the same "both hold" conclusion
    // for `$this->prop` as the forward walker does for a local.
    let php = r#"<?php
interface Aye { public function a(): void; }
interface Bee { public function b(): void; }

class Base {
    public function base(): void {}
}

class Holder {
    private Base $thing;

    private function wantsAye(Aye $x): void {}

    public function test(): void {
        if ($this->thing instanceof Aye && $this->thing instanceof Bee) {
            $this->wantsAye($this->thing);
        }
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "A property proven to be both classes should satisfy either one, got: {msgs:?}"
    );
}

#[test]
fn no_false_positive_for_asserted_mock_intersection() {
    // `assert($x instanceof MockInterface)` on a value already typed as
    // an unrelated concrete class proves it is both at once, exactly as
    // the `if`-based form does.
    let php = r#"<?php
interface MockInterface {
    public function shouldReceive(string $name): self;
}

class EpaymentService {
    public function annul(): bool { return true; }
}

class TestCase {
    protected function service(): EpaymentService {}

    protected function mockMethod(MockInterface $mock, string $method): void {}

    protected function realMethod(EpaymentService $service): void {}

    public function test(): void {
        $service = $this->service();
        assert($service instanceof MockInterface);
        $this->mockMethod($service, 'annul');
        $this->realMethod($service);
    }
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "An asserted mock satisfies both its class and the asserted interface, got: {msgs:?}"
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
fn no_false_positive_for_ternary_with_string_literal_branches() {
    let php = r#"<?php
/** @param 'asc'|'desc' $direction */
function orderBy(string $column, string $direction): void {}

function test(bool $flag): void {
    orderBy('id', $flag ? 'asc' : 'desc');
}
"#;
    let diags = collect(php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "Ternary with literal branches should match 'asc'|'desc' (issue #180), got: {msgs:?}"
    );
}

#[test]
fn canonical_resolver_covers_compound_and_signed_literal_expressions() {
    let php = r#"<?php
/** @param 'asc'|'desc' $direction */
function orderBy(string $direction): void {}
/** @param -3 $value */
function takesNegative(int $value): void {}

function test(bool $flag): void {
    orderBy(match ($flag) {
        true => 'asc',
        false => 'desc',
    });
    orderBy('asc' ?? 'desc');
    orderBy(($flag ? 'asc' : 'desc'));
    takesNegative(-3);
}
"#;
    let msgs = type_error_messages(&collect(php));
    assert!(
        msgs.is_empty(),
        "Canonical expression resolution should cover match, coalesce, parentheses, and signed literals: {msgs:?}"
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
    // resolved to their canonical parsed value, so they still satisfy an `int`
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

// ─── Resource → object migrated handles (phpstorm-stubs) ─────────────────────

#[test]
fn no_diagnostic_for_finfo_handle_after_false_check() {
    // finfo_open() returns `finfo|false` on PHP 8.1+ (via a
    // `#[LanguageLevelTypeAware]` attribute), even though its `@return`
    // docblock still says the legacy `resource|false`. After narrowing away
    // `false`, the handle is a `finfo`, which finfo_file()/finfo_close()
    // accept. No `type_mismatch_argument` should fire.
    let php = r#"<?php
function check_finfo(): void
{
    $finfo = finfo_open(FILEINFO_MIME_TYPE);
    if ($finfo === false) {
        throw new RuntimeException('finfo_open failed');
    }

    finfo_file($finfo, __FILE__);
    finfo_close($finfo);
}
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        !has_type_error(&diags),
        "finfo handle should resolve to finfo, not resource|false: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn no_diagnostic_for_pgsql_result_handle_after_false_check() {
    // pg_query() returns `PgSql\Result|false` on PHP 8.1+; pg_fetch_assoc()
    // and pg_free_result() expect `PgSql\Result`. Chained handle migration.
    let php = r#"<?php
function check_pgsql(): void
{
    $connection = pg_connect('host=localhost dbname=test');
    if ($connection === false) {
        throw new RuntimeException('pg_connect failed');
    }

    $result = pg_query($connection, 'select 1');
    if ($result === false) {
        throw new RuntimeException('pg_query failed');
    }

    pg_fetch_assoc($result);
    pg_free_result($result);
}
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        !has_type_error(&diags),
        "pg handles should resolve to their 8.1+ object types: {:?}",
        type_error_messages(&diags)
    );
}

// ─── int<0,max> passed to non-negative-int — no diagnostic (#170) ──────────

#[test]
fn no_diagnostic_for_int_range_passed_to_non_negative_int() {
    let php = r#"<?php
/**
 * @param non-negative-int $count
 */
function addCount(int $count): void {}

function test(array $items): void {
    /** @var int<0, max> $count */
    $count = 5;
    addCount($count);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "int<0,max> should be compatible with non-negative-int (issue #170): {:?}",
        type_error_messages(&diags)
    );
}

// ─── positive-int passed to non-negative-int — no diagnostic ───────────────

#[test]
fn no_diagnostic_for_positive_int_passed_to_non_negative_int() {
    let php = r#"<?php
/**
 * @param non-negative-int $count
 */
function addCount(int $count): void {}

/**
 * @param positive-int $n
 */
function test(int $n): void {
    addCount($n);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "positive-int should be compatible with non-negative-int: {:?}",
        type_error_messages(&diags)
    );
}

// ─── int<1,100> passed to non-negative-int — no diagnostic ─────────────────

#[test]
fn no_diagnostic_for_bounded_int_range_passed_to_non_negative_int() {
    let php = r#"<?php
/**
 * @param non-negative-int $count
 */
function addCount(int $count): void {}

function test(): void {
    /** @var int<1, 100> $count */
    $count = 50;
    addCount($count);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "int<1,100> should be compatible with non-negative-int: {:?}",
        type_error_messages(&diags)
    );
}

// ─── Integer literals passed to named refined-int pseudo-types (B50) ───────

#[test]
fn no_diagnostic_for_int_literal_passed_to_non_negative_int() {
    let php = r#"<?php
/**
 * @param non-negative-int $count
 */
function takesNonNeg(int $count): void {}

function test(): void {
    takesNonNeg(1);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "literal 1 should satisfy non-negative-int: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn no_diagnostic_for_int_literal_passed_to_positive_int() {
    let php = r#"<?php
/**
 * @param positive-int $count
 */
function takesPositive(int $count): void {}

function test(): void {
    takesPositive(1);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "literal 1 should satisfy positive-int: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn type_error_for_zero_literal_passed_to_positive_int() {
    let php = r#"<?php
/**
 * @param positive-int $count
 */
function takesPositive(int $count): void {}

function test(): void {
    takesPositive(0);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "literal 0 should violate positive-int: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn type_error_for_negative_literal_passed_to_non_negative_int() {
    let php = r#"<?php
/**
 * @param non-negative-int $count
 */
function takesNonNeg(int $count): void {}

function test(): void {
    takesNonNeg(-1);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "literal -1 should violate non-negative-int: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn no_diagnostic_for_int_literal_passed_to_non_zero_int() {
    let php = r#"<?php
/**
 * @param non-zero-int $count
 */
function takesNonZero(int $count): void {}

function test(): void {
    takesNonZero(1);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "literal 1 should satisfy non-zero-int: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn type_error_for_zero_literal_passed_to_non_zero_int() {
    let php = r#"<?php
/**
 * @param non-zero-int $count
 */
function takesNonZero(int $count): void {}

function test(): void {
    takesNonZero(0);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "literal 0 should violate non-zero-int: {:?}",
        type_error_messages(&diags)
    );
}

// ─── Issue #167: assignment in if-branch must not leak into elseif condition ─

#[test]
fn no_false_positive_for_var_reassigned_in_if_branch() {
    let php = r#"<?php
function normalizeBooleanString(string $value): bool
{
    if (mb_strtolower($value) === 't') {
        $value = true;
    } elseif (mb_strtolower($value) === 'f') {
        $value = false;
    }

    return (bool) $value;
}
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        !has_type_error(&diags),
        "Assignment in if-branch must not affect elseif condition (issue #167): {:?}",
        type_error_messages(&diags)
    );
}

// ─── Issue #173: iterator_to_array must return array, not iterator ──────────

#[test]
fn no_false_positive_for_iterator_to_array_with_class_element() {
    let php = r#"<?php
class Foo {}

function take_array(array $array): void {}

/** @param \Iterator<Foo> $iterator */
function make_array(\Iterator $iterator): array {
    return take_array(iterator_to_array($iterator));
}
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        !has_type_error(&diags),
        "iterator_to_array should return array, not Iterator (issue #173): {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn no_false_positive_for_iterator_to_array_with_scalar_element() {
    let php = r#"<?php
function take_array(array $array): void {}

/** @param \Iterator<string> $iterator */
function make_array(\Iterator $iterator): array {
    return take_array(iterator_to_array($iterator));
}
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        !has_type_error(&diags),
        "iterator_to_array with scalar element should also return array: {:?}",
        type_error_messages(&diags)
    );
}

// ─── Issue #165: strtr() with replace-pairs array — overloaded signature ────

#[test]
fn no_false_positive_for_strtr_with_array() {
    let php = r#"<?php
$result = strtr('Hello :name', [
    ':name' => 'Alex',
]);
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        !has_type_error(&diags),
        "strtr() with array replace_pairs should be valid (issue #165): {:?}",
        type_error_messages(&diags)
    );
}

// ─── Issue #167 (variant): simpler reproduction without stubs ───────────────

#[test]
fn no_false_positive_for_var_reassigned_in_if_branch_simple() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(string $value): void
{
    if ($value === 't') {
        $value = true;
    } elseif ($value === 'f') {
        takes_string($value);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "After $value = $value[0], type should no longer be array|false (issue #169): {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn no_false_positive_for_is_numeric_narrowed_string_param() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(mixed $value): void
{
    if (is_numeric($value)) {
        takes_string($value);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "is_numeric($value) should narrow to numeric-string|int|float, still valid for a string param: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn no_false_positive_for_is_a_allow_string_narrowed_array_key() {
    let php = r#"<?php
class Extension {}

/**
 * @param class-string<Extension> $className
 */
function takes_extension_class_string(string $className): void {}

/**
 * @param array{class: string} $config
 */
function activate(array $config): void
{
    if (!is_a($config['class'], Extension::class, true)) {
        throw new RuntimeException('not an Extension');
    }

    takes_extension_class_string($config['class']);
}
"#;
    // Use the slow pipeline so the forward-walked diagnostic scope cache
    // is active, matching real analysis where array-index narrowing keys
    // are recorded.
    let diags = collect_slow(php);
    assert!(
        !has_type_error(&diags),
        "is_a($config['class'], Extension::class, true) guard should narrow to class-string<Extension>: {:?}",
        type_error_messages(&diags)
    );
}

// ─── Issue #166: @phpstan-type aliases must not trigger false positives ──────

#[test]
fn no_false_positive_for_phpstan_type_alias_to_array() {
    let php = r#"<?php
/**
 * @phpstan-type Payload = array{name: string, phone: string}
 */
final class Api {
    /** @var Payload */
    private array $payload;

    public function __construct() {
        $this->payload = ['name' => 'Alex', 'phone' => '123'];
    }

    public function submit(): array {
        return Sender::post('/lead', $this->payload);
    }
}

final class Sender {
    public static function post(string $url, ?array $postData = null): array {
        return [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "@phpstan-type Payload alias should be compatible with ?array (issue #166): {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn no_false_positive_for_phpstan_type_alias_to_string_union() {
    let php = r#"<?php
/**
 * @phpstan-type Field = 'id'|'name'
 */
final class Filter {
    /**
     * @param Field $field
     */
    public function isDefined($field): bool {
        return property_exists($this, $field);
    }
}
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        !has_type_error(&diags),
        "@phpstan-type Field alias should be compatible with string (issue #166): {:?}",
        type_error_messages(&diags)
    );
}

// ─── Issue #169: reassign from own array offset must update type ────────────

#[test]
fn no_false_positive_after_reassign_from_array_offset() {
    let php = r#"<?php
function takes_string(string $s): void {}

function test(string $input): void
{
    /** @var list<string>|false $value */
    $value = ['a', 'b'];
    $value = $value[0];
    takes_string($value);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "After $value = $value[0], type should no longer be array|false (issue #169): {:?}",
        type_error_messages(&diags)
    );
}

// ─── Narrower intersection passed to broader intersection — no diagnostic ───

#[test]
fn no_diagnostic_for_narrower_intersection_argument() {
    // A value typed with *more* intersection members satisfies a
    // parameter typed with a subset of those members. This is the
    // common Mockery pattern where `Mockery::mock()` returns
    // `TheClass&MockInterface&LegacyMockInterface` and is passed to a
    // parameter typed `TheClass&MockInterface`.
    let php = r#"<?php
interface AA {}
interface BB {}
interface CC {}

function takes(AA&BB $value): void {}

/**
 * @param AA&BB&CC $value
 */
function test(mixed $value): void {
    takes($value);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A narrower intersection (AA&BB&CC) must satisfy a broader one (AA&BB): {:?}",
        type_error_messages(&diags)
    );
}

// ─── Framework testing mock() helper resolves to the mock intersection ──────

#[test]
fn testing_mock_helper_satisfies_array_and_typed_params() {
    // Laravel's `TestCase::mock()` is declared as returning a bare
    // `Mockery\MockInterface`, discarding the mocked class. The patch
    // makes it generic so `$this->mock(IRule::class)` resolves to
    // `MockInterface&IRule`, which then satisfies both an
    // `array<IRule>` element type and a plain `IRule` parameter. The
    // helper is inherited from a base class, matching the real
    // framework layout.
    let php = r#"<?php
namespace Mockery {
    interface LegacyMockInterface {}
    interface MockInterface extends LegacyMockInterface {}
}
namespace App {
    interface IRule { public function check(): bool; }

    class Consumer {
        /** @param array<IRule> $rules */
        public function __construct(array $rules) {}
    }

    class Needs {
        public function handle(IRule $rule): void {}
    }

    class TestBase {
        /**
         * @param string $abstract
         * @return \Mockery\MockInterface
         */
        protected function mock($abstract) {}
    }

    class ExampleTest extends TestBase {
        public function test(): void {
            $rule = $this->mock(IRule::class);
            new Consumer([$rule]);
            (new Needs())->handle($rule);
        }
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "a mock of IRule must satisfy array<IRule> and IRule parameters: {:?}",
        type_error_messages(&diags)
    );
}

// ─── String literal naming a function is a valid callable ───────────────────

#[test]
fn no_false_positive_string_literal_passed_to_callable_param() {
    // A string literal that names a function (e.g. 'trim', 'intval') is a
    // valid PHP callable and must not be flagged when passed to a
    // `callable` / `?callable` parameter.
    let php = r#"<?php
function apply(?callable $callback, array $items): array
{
    return $callback === null ? $items : array_map($callback, $items);
}

function test(array $items): void
{
    apply('intval', $items);
    apply('trim', $items);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A string literal naming a function must satisfy a ?callable parameter: {:?}",
        type_error_messages(&diags)
    );
}

// ─── Conditional return through @mixin uses the mixin's template default ─────

#[test]
fn conditional_return_via_mixin_uses_template_default() {
    // `@mixin Req` where `Req` is `@template TAsync of bool = false` behaves
    // like `@mixin Req<false>`, so a conditional return keyed on `TAsync`
    // collapses to the then-branch. Without the default the merged method
    // loses the mixin origin and resolution falls to the else-branch,
    // producing a false argument type mismatch.
    let php = r#"<?php
/**
 * @template TAsync of bool = false
 */
class Req
{
    /**
     * @phpstan-return (TAsync is false ? \DateTime : \Exception)
     */
    public function get()
    {
        return new \DateTime();
    }
}

/**
 * @mixin Req
 */
class Fac {}

function takesDate(\DateTime $d): void {}

function test(Fac $f): void
{
    $response = $f->get();
    takesDate($response);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Conditional return through @mixin must resolve TAsync to its default (false): {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn conditional_return_on_generic_class_uses_template_default() {
    let php = r#"<?php
/**
 * @template TAsync of bool = false
 */
class PendingRequest
{
    /**
     * @return Response|PromiseInterface
     * @phpstan-return (TAsync is false ? Response : PromiseInterface)
     */
    public function get(string $url) {}
}

class Response {}
interface PromiseInterface {}

function takesResponse(Response $response): void {}
function takesPromise(PromiseInterface $promise): void {}

function test(PendingRequest $request): void
{
    $response = $request->get('/users');
    takesResponse($response);
}

/** @param PendingRequest<true> $request */
function testAsync(PendingRequest $request): void
{
    takesPromise($request->get('/users'));
}

/** @mixin PendingRequest */
class HttpFactory {}

/** @method static Response|PromiseInterface get(string $url) */
class HttpFacade
{
    protected static function getFacadeAccessor()
    {
        return HttpFactory::class;
    }
}

function testFacade(): void
{
    takesResponse(HttpFacade::get('/users'));
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A generic class's default template argument must decide its method's conditional return: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn selecting_a_non_default_template_argument_overrides_the_default() {
    let php = r#"<?php
/**
 * @template TAsync of bool = false
 */
class PendingRequest
{
    /**
     * @template T of bool = true
     * @param T $async
     * @return self<T>
     */
    public function async(bool $async = true) {}

    /**
     * @return Response|PromiseInterface
     * @phpstan-return (TAsync is false ? Response : PromiseInterface)
     */
    public function get(string $url) {}
}

class Response {}
interface PromiseInterface {}

function takesPromise(PromiseInterface $promise): void {}

function testDirect(PendingRequest $request): void
{
    takesPromise($request->async()->get('/users'));
}

/** @mixin PendingRequest */
class HttpFactory {}

/**
 * @method static PendingRequest async(bool $async = true)
 * @method static Response|PromiseInterface get(string $url)
 */
class HttpFacade
{
    protected static function getFacadeAccessor()
    {
        return HttpFactory::class;
    }
}

function testFactory(HttpFactory $factory): void
{
    takesPromise($factory->async()->get('/users'));
}

function testFacade(): void
{
    takesPromise(HttpFacade::async()->get('/users'));
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "An explicitly selected template argument must beat the declared default, through a mixin and a facade as well as directly: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn a_conditional_keyed_on_a_method_template_is_decided_by_its_argument() {
    let php = r#"<?php
class Alpha {}
class Beta {}

class Picker
{
    /**
     * @template T
     * @param T $value
     * @return (T is int ? Alpha : Beta)
     */
    public function pick($value) {}

    /**
     * @template T of string
     * @param T $value
     * @return (T is 'a' ? Alpha : Beta)
     */
    public function pickLiteral(string $value) {}
}

function takesAlpha(Alpha $a): void {}
function takesBeta(Beta $b): void {}

function test(Picker $picker): void
{
    takesAlpha($picker->pick(1));
    takesBeta($picker->pick('x'));
    takesAlpha($picker->pickLiteral('a'));
    takesBeta($picker->pickLiteral('b'));
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A conditional keyed on the method's own @template must be decided by the argument that binds it: {:?}",
        type_error_messages(&diags)
    );
}

// ─── class-string<T>|T union binds T to the class, not the class-string ─────

#[test]
fn variadic_class_string_or_instance_union_binds_inner_class() {
    // The Mockery pattern: `mock(...$args)` declares
    // `@param array<class-string<TMock>|TMock|...> $args`. Passing
    // `Foo::class` must bind TMock to Foo (matching the
    // `class-string<TMock>` alternative), not to `class-string<Foo>`,
    // so the returned mock satisfies a parameter typed `Foo`.
    let php = r#"<?php
interface LegacyMockInterface {}
interface MockInterface {}
interface Connector {}

class MockFactory
{
    /**
     * @template TMock of object
     *
     * @param array<class-string<TMock>|TMock|Closure(LegacyMockInterface&MockInterface&TMock):LegacyMockInterface&MockInterface&TMock|array<TMock>> $args
     *
     * @return LegacyMockInterface&MockInterface&TMock
     */
    public static function mock(...$args) {}
}

function takes(Connector $c): void {}

function test(): void
{
    $mock = MockFactory::mock(Connector::class);
    takes($mock);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "TMock must bind to Connector, not class-string<Connector>: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn variadic_class_string_or_instance_union_binds_instance_directly() {
    // Same union hint, but passing an object instance instead of a
    // `::class` constant must bind TMock directly to the instance type.
    let php = r#"<?php
interface LegacyMockInterface {}
interface MockInterface {}
interface Connector {}
class RealConnector implements Connector {}

class MockFactory
{
    /**
     * @template TMock of object
     *
     * @param array<class-string<TMock>|TMock|array<TMock>> $args
     *
     * @return LegacyMockInterface&MockInterface&TMock
     */
    public static function mock(...$args) {}
}

function takes(Connector $c): void {}

function test(): void
{
    $mock = MockFactory::mock(new RealConnector());
    takes($mock);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "TMock must bind to RealConnector when given an instance: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn unwrapped_class_string_or_instance_union_binds_inner_class() {
    // The container pattern without an array wrapper:
    // `@param class-string<T>|T $abstract` with `Foo::class` must bind
    // T to Foo, not to class-string<Foo>.
    let php = r#"<?php
interface Connector {}

class Container
{
    /**
     * @template T of object
     *
     * @param class-string<T>|T $abstract
     *
     * @return T
     */
    public static function make($abstract) {}
}

function takes(Connector $c): void {}

function test(): void
{
    $instance = Container::make(Connector::class);
    takes($instance);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "T must bind to Connector, not class-string<Connector>: {:?}",
        type_error_messages(&diags)
    );
}

// ─── Template bindings must not leak across call sites ───────────────────────

#[test]
fn template_binding_does_not_leak_across_call_sites() {
    // Two methods each pass a differently-typed local variable (both named
    // `$stmt`) through a pair of `@template T` identity helpers.  The chain
    // resolution cache keys call chains by subject text, so without a scope
    // discriminator the binding `T => ForNode` from the first method would
    // leak into the second, wrongly flagging the second call's argument.
    let php = r#"<?php
class Node {}
class Stmt extends Node {}
class ForNode extends Stmt {}
class WhileNode extends Stmt {}

class Parser
{
    /**
     * @template T of Node
     * @param T $node
     * @return T
     */
    protected function setPositions(Node $node): Node
    {
        return $node;
    }

    /**
     * @template T of Stmt
     * @param T $stmt
     * @return T
     */
    private function parseBody(Stmt $stmt): Stmt
    {
        return $stmt;
    }

    private function parseFor(): ForNode
    {
        $stmt = new ForNode();
        return $this->setPositions($this->parseBody($stmt));
    }

    private function parseWhile(): WhileNode
    {
        $stmt = new WhileNode();
        return $this->setPositions($this->parseBody($stmt));
    }
}
"#;
    let diags = collect_slow(php);
    assert!(
        !has_type_error(&diags),
        "Template bindings leaked across call sites: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn per_site_binding_still_flags_real_mismatch() {
    // Same identity-helper call text at two sites with differently-typed
    // `$node`.  The first site is well-typed; the second passes the wrong
    // type onward.  The per-site cache discriminator must keep the second
    // site's binding distinct so the real mismatch is still reported (and
    // not masked by sharing the first site's clean result).
    let php = r#"<?php
class Alpha {}
class Beta {}

function needsAlpha(Alpha $a): void {}

class Helper
{
    /**
     * @template T
     * @param T $node
     * @return T
     */
    public function identity($node)
    {
        return $node;
    }

    public function ok(): void
    {
        $node = new Alpha();
        needsAlpha($this->identity($node));
    }

    public function bad(): void
    {
        $node = new Beta();
        needsAlpha($this->identity($node));
    }
}
"#;
    let diags = collect_slow(php);
    let msgs = type_error_messages(&diags);
    assert_eq!(
        msgs.len(),
        1,
        "Expected exactly one mismatch (the Beta site), got: {msgs:?}"
    );
    assert!(
        msgs[0].contains("Alpha") && msgs[0].contains("Beta"),
        "Expected Alpha/Beta mismatch, got: {msgs:?}"
    );
}

#[test]
fn no_diagnostic_for_imported_type_alias_param_with_two_leading_spaces() {
    // A method typed `@param CountParams $query`, where `CountParams` is
    // declared via `@phpstan-type` on another class and pulled in with
    // `@phpstan-import-type`, must not be treated as a class reference.
    // The import tag is written with two spaces after the asterisk — the
    // style found in real vendor code — which previously caused the tag
    // to be dropped, leaving the alias to be namespace-resolved as a
    // class and flagged against the passed array shape.
    use crate::common::create_psr4_workspace;

    let files = vec![
        (
            "src/Types.php",
            r#"<?php
namespace App;

/**
 * @phpstan-type CountParams array{
 *     index: string,
 *     body?: array<mixed>,
 * }
 */
class ElasticsearchTypes {}
"#,
        ),
        (
            "src/Service.php",
            r#"<?php
namespace App;

/**
 *  @phpstan-import-type CountParams from ElasticsearchTypes
 */
class Service {
    /**
     * @param CountParams $query
     */
    public function count(array $query = []): int { return 0; }
}
"#,
        ),
        (
            "src/Caller.php",
            r#"<?php
namespace App;

class Caller {
    public function run(Service $service): void {
        $service->count([
            'index' => 'foo',
            'body' => ['x' => 1],
        ]);
    }
}
"#,
        ),
    ];

    let composer = r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#;
    let (backend, dir) = create_psr4_workspace(composer, &files);
    for (rel_path, php) in &files {
        let file_uri = format!("file://{}/{}", dir.path().display(), rel_path);
        backend.update_ast(&file_uri, php);
    }
    let uri = format!("file://{}/src/Caller.php", dir.path().display());
    let content = files[2].1;
    let mut diags = Vec::new();
    backend.collect_argument_type_diagnostics(&uri, content, &mut diags);
    assert!(
        !has_type_error(&diags),
        "Imported type alias parameter must not be treated as a class, got: {}",
        type_error_messages(&diags).join(", ")
    );
}

/// A `new ReflectionClass($classString)` where the argument is a
/// `class-string<T>` binds the class template parameter `T` to the object
/// type, not the class-string itself.  The phpstorm-stubs constructor is
/// annotated `@param class-string<T>|T $objectOrClass` but its native hint
/// comes from a `#[LanguageLevelTypeAware]` attribute resolving to
/// `object|string`.  The docblock type must still refine that native union
/// so `$reflection->newInstanceArgs(...)` resolves to `T` (the
/// instance), not `class-string<T>`.
#[test]
fn no_false_positive_for_reflection_class_new_instance_args() {
    const REFLECTION_STUB: &str = r#"<?php
/**
 * @template T of object
 */
class ReflectionClass {
    /** @param class-string<T>|T $objectOrClass */
    public function __construct(#[LanguageLevelTypeAware(['8.0' => 'object|string'], default: '')] $objectOrClass) {}
    /** @return T */
    public function newInstanceArgs(array $args = []): object {}
}
"#;
    let mut class_stubs: std::collections::HashMap<&'static str, &'static str> =
        std::collections::HashMap::new();
    class_stubs.insert("ReflectionClass", REFLECTION_STUB);
    let backend = phpantom_lsp::Backend::new_test_with_all_stubs(
        class_stubs,
        std::collections::HashMap::new(),
        std::collections::HashMap::new(),
    );
    let uri = "file:///reflection.php";
    let content = r#"<?php
class AbstractASTNode {}
class ASTAnonymousClass {}

class Test {
    protected function createNodeInstance(): AbstractASTNode|ASTAnonymousClass
    {
        /** @var class-string<AbstractASTNode|ASTAnonymousClass> */
        $class = substr(static::class, 0, -4);

        $reflection = new ReflectionClass($class);

        return $reflection->newInstanceArgs([__METHOD__]);
    }
}
"#;

    backend.update_ast(uri, content);
    let mut out = Vec::new();
    backend.collect_return_type_diagnostics(uri, content, &mut out);
    assert!(
        out.is_empty(),
        "newInstanceArgs on ReflectionClass<T> should resolve to the instance type, \
         not a class-string, got: {out:?}"
    );
}

/// The same call against the *embedded* stubs, which declare
/// `newInstanceArgs()` as `@return T|null`.  The method throws rather
/// than returning null, so the instance it builds is as non-null as the
/// one `newInstance()` returns and neither call needs a null check.
#[test]
fn reflection_class_new_instance_args_is_not_nullable() {
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///reflection_new_instance_args.php";
    let content = r#"<?php
class Node {}

class NodeBuilder {
    public function fromArgs(): Node
    {
        /** @var class-string<Node> $class */
        $class = substr(static::class, 0, -7);
        $reflection = new ReflectionClass($class);

        return $reflection->newInstanceArgs([__METHOD__]);
    }

    public function fromVariadic(): Node
    {
        /** @var class-string<Node> $class */
        $class = substr(static::class, 0, -7);
        $reflection = new ReflectionClass($class);

        return $reflection->newInstance(__METHOD__);
    }
}
"#;
    backend.update_ast(uri, content);
    let mut out = Vec::new();
    backend.collect_return_type_diagnostics(uri, content, &mut out);
    assert!(
        out.is_empty(),
        "newInstanceArgs must match newInstance and return a plain Node, got: {out:?}"
    );
}

/// PHP calls an autoloader with the name of the class it is looking for,
/// so an untyped `$class` parameter is a `string` even though the stub
/// only says `callable`.  Without that, string builtins applied to it
/// return their full union and the next call reports a mismatch.
#[test]
fn spl_autoload_register_closure_param_is_a_string() {
    let diags = collect_with_full_stubs(
        r#"<?php
spl_autoload_register(function ($class): void {
    $file = __DIR__ . strtr(str_replace('App\\', '', $class), '\\', '/') . '.php';
    require $file;
});
"#,
    );
    assert!(
        diags.is_empty(),
        "An autoloader's $class parameter should be inferred as string, got: {diags:?}"
    );
}

/// The patched callback type keeps its nullable wrapper: registering the
/// default autoloader by passing no callback at all is still valid.
#[test]
fn spl_autoload_register_still_accepts_no_callback() {
    let diags = collect_with_full_stubs(
        r#"<?php
function myAutoload(string $class): void { echo $class; }

spl_autoload_register();
spl_autoload_register(null);
spl_autoload_register('myAutoload');
"#,
    );
    assert!(
        diags.is_empty(),
        "spl_autoload_register must still accept a null or string callback, got: {diags:?}"
    );
}

// ─── model-property<Model> type validation ──────────────────────────────────

#[test]
fn no_false_positive_model_property_valid_column() {
    let php = r#"<?php
class Process {
    public string $name;
    public int $status;
}

class Service {
    /** @param model-property<Process> $column */
    public function sortBy(string $column): void {}
}

function test(): void {
    $svc = new Service();
    $svc->sortBy('name');
    $svc->sortBy('status');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A string literal naming a valid property must satisfy model-property<Model>: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn model_property_flags_invalid_column() {
    let php = r#"<?php
class Process {
    public string $name;
    public int $status;
}

class Service {
    /** @param model-property<Process> $column */
    public function sortBy(string $column): void {}
}

function test(): void {
    $svc = new Service();
    $svc->sortBy('nonexistent');
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "A string literal that is NOT a property of the model should be flagged"
    );
}

#[test]
fn no_false_positive_model_property_non_literal_string() {
    let php = r#"<?php
class Process {
    public string $name;
    public int $status;
}

class Service {
    /** @param model-property<Process> $column */
    public function sortBy(string $column): void {}
}

function test(string $col): void {
    $svc = new Service();
    $svc->sortBy($col);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "A non-literal string must be accepted for model-property (MAYBE): {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn no_false_positive_model_property_to_model_property() {
    let php = r#"<?php
class Process {
    public string $name;
}

class Outer {
    /** @param model-property<Process> $column */
    public function sortBy(string $column): void {}

    /** @param model-property<Process> $col */
    public function proxy(string $col): void {
        $this->sortBy($col);
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Passing model-property to model-property of the same model must not error: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn model_property_array_flags_invalid_element() {
    let php = r#"<?php
class Process {
    public string $name;
    public int $status;
}

class Service {
    /** @param array<model-property<Process>, mixed> $params */
    public function test(array $params): void {}
}

function test(): void {
    $svc = new Service();
    $svc->test(['nonexistent' => 1]);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "A string literal in an array that is NOT a property of the model should be flagged"
    );
}

#[test]
fn no_false_positive_model_property_array_valid_elements() {
    let php = r#"<?php
class Process {
    public string $name;
    public int $status;
}

class Service {
    /** @param array<model-property<Process>, mixed> $params */
    public function test(array $params): void {}
}

function test(): void {
    $svc = new Service();
    $svc->test(['name' => 1, 'status' => 2]);
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "Valid property names in an array should not be flagged: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn model_property_list_flags_invalid_value() {
    let php = r#"<?php
class Process {
    public string $name;
    public int $status;
}

class Service {
    /** @param list<model-property<Process>> $columns */
    public function test(array $columns): void {}
}

function test(): void {
    $svc = new Service();
    $svc->test(['nonexistent']);
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "A string literal value in a list that is NOT a property should be flagged"
    );
}

// ─── Built-in names shadowed by vendor polyfills ─────────────────────────────

/// A vendor polyfill (e.g. symfony/polyfill-php84) may declare a legacy
/// `final class RoundingMode` whose "cases" are plain int constants,
/// registered in the classmap alongside the real built-in enum.  The
/// embedded stub must win: `RoundingMode::HalfAwayFromZero` passed to a
/// `RoundingMode` parameter is an enum case, not an int.
#[test]
fn builtin_enum_case_prefers_stub_over_polyfill_classmap_entry() {
    let backend = create_test_backend_with_full_stubs();

    // Simulate the polyfill's legacy-class variant on disk, registered
    // in the classmap under the built-in's global name.
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let polyfill = dir.path().join("RoundingMode.php");
    std::fs::write(
        &polyfill,
        concat!(
            "<?php\n",
            "if (\\PHP_VERSION_ID < 80100) {\n",
            "    final class RoundingMode\n",
            "    {\n",
            "        const HalfAwayFromZero = 0;\n",
            "        const HalfTowardsZero = 1;\n",
            "        const HalfEven = 2;\n",
            "    }\n",
            "}\n",
        ),
    )
    .expect("failed to write polyfill file");
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "RoundingMode".to_string(),
            Url::from_file_path(&polyfill).unwrap().to_string(),
        );
    }

    let php = r#"<?php
function takes_mode(RoundingMode $mode): void {}

function test(): void {
    takes_mode(RoundingMode::HalfAwayFromZero);
}
"#;
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_argument_type_diagnostics(uri, php, &mut out);
    assert!(
        !has_type_error(&out),
        "Enum case of a built-in must resolve to the enum, not the polyfill's int constant: {:?}",
        type_error_messages(&out)
    );
}

// ─── Inherited @param docblock on an override ───────────────────────────────

/// PHP forces an override to restate the native hint, so the ancestor's
/// `@param` (narrowed by `@implements`) still describes what arrives.
#[test]
fn no_diagnostic_for_param_narrowed_by_inherited_template_docblock() {
    let php = r#"<?php
class Node {}
class CallLike extends Node {}
/** @template-covariant TNodeType of Node */
interface Rule {
    /** @param TNodeType $node */
    public function processNode(Node $node): array;
}
class Helper {
    public function take(CallLike $c): int { return 1; }
}
/** @implements Rule<CallLike> */
final class MyRule implements Rule {
    private Helper $h;
    public function processNode(Node $node): array {
        $this->h->take($node);
        return [];
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "@implements Rule<CallLike> narrows $$node to CallLike, got: {diags:?}"
    );
}

/// The inherited docblock only applies when the override kept the
/// ancestor's native hint.  Widening it deliberately keeps the wider type.
#[test]
fn flags_param_where_override_widened_the_native_hint() {
    let php = r#"<?php
class Node {}
class CallLike extends Node {}
/** @template-covariant TNodeType of CallLike */
interface Rule {
    /** @param TNodeType $node */
    public function processNode(CallLike $node): array;
}
class Helper {
    public function take(CallLike $c): int { return 1; }
}
/** @implements Rule<CallLike> */
final class MyRule implements Rule {
    private Helper $h;
    public function processNode(mixed $node): array {
        return [];
    }
}
class Caller {
    public function go(Helper $h, Node $n): void {
        $h->take($n);
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "a plain Node argument is still flagged against a CallLike param, got: {diags:?}"
    );
}

// ─── match ($x::class) arm narrowing ────────────────────────────────────────

/// A `match ($node::class)` dispatch table narrows the subject in each arm,
/// so passing it to a handler declared for that exact class is fine.  This
/// is how visitor dispatch is written since PHP 8.
#[test]
fn no_diagnostic_for_argument_narrowed_by_match_on_class_constant() {
    let php = r#"<?php
class Node {}
class CallLike extends Node {}
class StaticCall extends Node {}
class FuncCall extends Node {}
class Visitor {
    public function visitStaticCall(StaticCall $node): void {}
    public function visitFuncCall(FuncCall $node): void {}
    public function visitCall(CallLike $node): void {}
    public function dispatch(Node $node): void {
        match ($node::class) {
            StaticCall::class => $this->visitStaticCall($node),
            FuncCall::class => $this->visitFuncCall($node),
            default => null,
        };
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "match ($$node::class) arms should narrow the argument, got: {diags:?}"
    );
}

#[test]
fn flags_argument_in_match_arm_that_narrows_to_a_different_class() {
    let php = r#"<?php
class Node {}
class StaticCall extends Node {}
class FuncCall extends Node {}
class Visitor {
    public function visitFuncCall(FuncCall $node): void {}
    public function dispatch(Node $node): void {
        match ($node::class) {
            StaticCall::class => $this->visitFuncCall($node),
            default => null,
        };
    }
}
"#;
    let diags = collect(php);
    assert!(
        has_type_error(&diags),
        "an arm narrowed to StaticCall cannot feed a FuncCall param, got: {diags:?}"
    );
}

#[test]
fn no_diagnostic_or_short_circuit_narrows_null_for_rhs_call() {
    // Issue #325: `$x === null || !take_not_null($x)` — the RHS only
    // executes when `$x` is NOT null, so `$x` is `int` there.
    let php = r#"<?php
function take_not_null(int $x): void {}

function test(): void {
    $x = isset($_POST['x']) ? (int) $_POST['x'] : null;

    if ($x === null || !take_not_null($x)) {
        return;
    }
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "OR short-circuit should narrow $x to int on RHS, got: {diags:?}"
    );
}

#[test]
fn conditional_nested_in_generic_return_collapses_against_call_args() {
    // `groupBy('key')` returns `static<($groupBy is array|string ? array-key
    // : TGroupKey), …>`. Grouping by a string decides the condition, so the
    // result's key type is `array-key` and a string key satisfies `get()`.
    // Left unevaluated, the conditional binds to `TKey` and no argument can
    // ever match it.
    let php = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 */
class Coll
{
    /**
     * @template TGroupKey of array-key
     *
     * @param  (callable(TValue, TKey): TGroupKey)|array|string  $groupBy
     * @return static<
     *  ($groupBy is (array|string)
     *      ? array-key
     *      : TGroupKey),
     *  static<TKey, TValue>
     * >
     */
    public function groupBy($groupBy) {}

    /**
     * @param  TKey  $key
     * @return TValue|null
     */
    public function get($key) {}
}

function test(Coll $c): void
{
    $c->groupBy('key')->get('bucket');
}
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "a conditional nested in the generic return must collapse against the call args, got: {:?}",
        type_error_messages(&diags)
    );
}

#[test]
fn undecided_conditional_param_type_compares_as_branch_union() {
    // The condition names a parameter of an earlier call, so it cannot be
    // decided here. The value still satisfies one branch or the other, so
    // compare against `int|string` — a string passes, an object does not.
    let php = r#"<?php
class Bucket {}

class Holder
{
    /** @param ($unknown is int ? int : string) $key */
    public function take($key): void {}
}

function test(Holder $h): void
{
    $h->take('bucket');
    $h->take(new Bucket());
}
"#;
    let diags = collect(php);
    let messages = type_error_messages(&diags);
    assert_eq!(
        messages.len(),
        1,
        "only the DateTime argument may be rejected, got: {messages:?}"
    );
    assert!(
        messages[0].contains("expects int|string"),
        "the message must name the collapsed branch union, got: {messages:?}"
    );
}

/// A generic class instantiated with no template-binding constructor
/// argument resolves its template params to their declared bounds, and the
/// resulting `Foo<bound>` type must carry the class's fully qualified name.
/// A short base name is unloadable from the call site's namespace, so the
/// value looked incompatible with a parameter typed by the same class.
#[test]
fn instantiated_generic_resolved_to_bounds_matches_plain_parameter() {
    use crate::common::create_psr4_workspace;

    const COMPOSER: &str =
        r#"{"autoload": {"psr-4": {"App\\": "app/", "Acme\\Decimal\\": "dec/"}}}"#;
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            (
                "dec/Decimal.php",
                r#"<?php
namespace Acme\Decimal;

/**
 * @template-covariant TNotZero of bool = bool
 */
final class Decimal {
    public function __construct(int|self|string $value) {}
}
"#,
            ),
            (
                "app/Helper.php",
                r#"<?php
namespace App;

use Acme\Decimal\Decimal;

class Helper {
    public static function formatPrice(Decimal $amount): string { return ''; }
}
"#,
            ),
        ],
    );
    let uri = format!("file://{}/app/Run.php", dir.path().display());
    let php = "<?php\n\\App\\Helper::formatPrice(new \\Acme\\Decimal\\Decimal('0.00'));\n";
    backend.update_ast(&uri, php);
    let mut out = Vec::new();
    backend.collect_argument_type_diagnostics(&uri, php, &mut out);
    assert!(
        !has_type_error(&out),
        "expected no type error, got {:?}",
        type_error_messages(&out)
    );
}

/// An unexpanded `@phpstan-type` alias used with generic arguments
/// (`Results<int>`) is as unverifiable as the bare alias name, so it must
/// not produce a mismatch either.
#[test]
fn unloadable_short_generic_name_does_not_report_a_mismatch() {
    let php = r#"<?php
/**
 * @param Results<int> $r
 */
function takesResults($r): void {}

takesResults([1, 2]);
"#;
    let diags = collect(php);
    assert!(
        !has_type_error(&diags),
        "expected no type error, got {:?}",
        type_error_messages(&diags)
    );
}

// ─── Re-keying callbacks rebind the key template ────────────────────────────

/// A generic collection shaped like Laravel's `Illuminate\Support\Collection`:
/// `keyBy()` re-keys it, so the key template of the returned collection comes
/// from the callback's return type.
const REKEYING_COLLECTION: &str = r#"<?php
class Item {
    public string $slug = '';

    public function getSlug(): string { return $this->slug; }

    /** @return array<string, Item> */
    public function toPair(): array { return ['x' => $this]; }
}

/**
 * @template TKey of array-key
 * @template TValue
 */
class Coll
{
    /**
     * @template TNewKey of array-key
     *
     * @param (callable(TValue, TKey): TNewKey)|array|string $keyBy
     * @return static<($keyBy is (array|string) ? array-key : TNewKey), TValue>
     */
    public function keyBy($keyBy) {}

    /**
     * @template TMapWithKeysKey of array-key
     * @template TMapWithKeysValue
     *
     * @param callable(TValue, TKey): array<TMapWithKeysKey, TMapWithKeysValue> $callback
     * @return static<TMapWithKeysKey, TMapWithKeysValue>
     */
    public function mapWithKeys(callable $callback) {}

    /** @param TKey|null $key */
    public function get($key) {}
}

/** @extends Coll<int, Item> */
final class ItemCollection extends Coll {}
"#;

/// Build a source file that re-keys a `Coll<int, Item>` with `$callback` and
/// then looks the result up with an argument no key type accepts, so the
/// diagnostic spells out the key type `get()` was checked against.
fn rekeyed_lookup_message(callback: &str, chained: bool) -> String {
    let call = if chained {
        format!("$c->keyBy({callback})->get([1]);")
    } else {
        format!("$keyed = $c->keyBy({callback});\n$keyed->get([1]);")
    };
    let php = format!(
        "{REKEYING_COLLECTION}\n/** @var Coll<int, Item> $c */\n$c = new Coll();\n{call}\n"
    );
    let messages = type_error_messages(&collect(&php));
    assert_eq!(
        messages.len(),
        1,
        "expected exactly one type error for `{callback}`, got {messages:?}"
    );
    messages.into_iter().next().unwrap()
}

#[test]
fn keyby_with_annotated_callback_rebinds_the_key_template() {
    for chained in [false, true] {
        assert!(
            rekeyed_lookup_message("fn (Item $i): string => $i->slug", chained)
                .contains("expects string|null"),
            "chained={chained}"
        );
    }
}

/// `static fn` and `static function` are ordinary closure literals; the
/// modifier must not stop the callback's return type from binding the
/// template.
#[test]
fn keyby_with_static_callback_rebinds_the_key_template() {
    for callback in [
        "static fn (Item $i): string => $i->slug",
        "static function (Item $i): string { return $i->slug; }",
    ] {
        for chained in [false, true] {
            assert!(
                rekeyed_lookup_message(callback, chained).contains("expects string|null"),
                "callback={callback} chained={chained}"
            );
        }
    }
}

/// Without a return-type annotation the callback's body is the only source
/// for the key type, so it has to survive into the resolved call.
#[test]
fn keyby_with_unannotated_callback_binds_the_key_from_the_body() {
    for callback in [
        "fn (Item $i) => $i->slug",
        "function (Item $i) { return $i->slug; }",
    ] {
        for chained in [false, true] {
            assert!(
                rekeyed_lookup_message(callback, chained).contains("expects string|null"),
                "callback={callback} chained={chained}"
            );
        }
    }
}

/// A body that is a *call* has to bind the template from the callee's return
/// type, just like a body that is a property read or a literal.
#[test]
fn keyby_with_unannotated_call_body_binds_the_key_from_the_return_type() {
    for callback in [
        "fn (Item $i) => $i->getSlug()",
        "function (Item $i) { return $i->getSlug(); }",
    ] {
        for chained in [false, true] {
            assert!(
                rekeyed_lookup_message(callback, chained).contains("expects string|null"),
                "callback={callback} chained={chained}"
            );
        }
    }
}

// ─── A standalone `@var` docblock narrows a non-Expression statement ───────

/// A generic collection with a `get()` whose parameter type comes from the
/// class-level `@template`, matching Laravel's `Illuminate\Support\Collection`.
const GENERIC_COLLECTION: &str = r#"<?php
namespace App\Models;

class Loaf {}

/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection
{
    /** @param TKey|null $key */
    public function get($key) {}
}
"#;

/// A standalone `/** @var Collection<string, Loaf> $byName */` followed by
/// an `echo` (rather than a bare `$byName->get(...)` expression statement)
/// used to lose the `<string, Loaf>` generic arguments once the diagnostic
/// scope cache was active: the cache recorded the pre-docblock scope at the
/// echo statement's start offset and never re-recorded it after the
/// docblock was applied, so a lookup for the call site inside the echo's
/// expression saw `$byName` typed only as bare `Collection` and fell back
/// to `TKey`'s bound (`array-key`) instead of the annotated `string`. Every
/// Blade `{{ $byName->get(...) }}` compiles to exactly this shape
/// (`echo e( $byName->get(...) );`), which is how the bug originally
/// surfaced.
#[test]
fn standalone_var_docblock_narrows_echo_statement() {
    let php = format!(
        "{GENERIC_COLLECTION}\nfunction e($x) {{ return $x; }}\n\
         function render() {{\n\
         /** @var \\App\\Models\\Collection<string, \\App\\Models\\Loaf> $byName */\n\
         echo e( $byName->get([1]) );\n}}\n"
    );
    let diags = collect_slow(&php);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.iter().any(|m| m.contains("expects string|null")),
        "expected TKey to narrow to string, got {msgs:?}"
    );
}

// ─── A union parameter hint binds through the alternative that matches ──────

/// `Collection::wrap()`'s shape: the argument is either the element itself or
/// a container of elements, so the template binds one level deeper for a
/// container argument and directly for a scalar one.
const UNION_WRAP_COLLECTION: &str = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 */
class Wrapper
{
    /**
     * @template TWrapValue
     *
     * @param  iterable<array-key, TWrapValue>|TWrapValue  $value
     * @return static<array-key, TWrapValue>
     */
    public static function wrap($value) {}

    /** @param TValue $value */
    public function push($value) {}
}

/** @return array<string> */
function names(): array { return []; }
"#;

/// Report the type error `push()` raises for `Wrapper::wrap($arg)`, which
/// spells out what `TWrapValue` bound to.
fn wrapped_push_message(arg: &str) -> String {
    let php = format!("{UNION_WRAP_COLLECTION}\n$w = Wrapper::wrap({arg});\n$w->push([1]);\n");
    let messages = type_error_messages(&collect(&php));
    assert_eq!(
        messages.len(),
        1,
        "expected exactly one type error for `{arg}`, got {messages:?}"
    );
    messages.into_iter().next().unwrap()
}

#[test]
fn a_container_argument_binds_the_template_through_the_iterable_alternative() {
    assert!(wrapped_push_message("names()").contains("expects string"));
}

/// The bare `TWrapValue` alternative matches anything, so it must still win
/// when the argument is not a container.
#[test]
fn a_scalar_argument_binds_the_template_through_the_bare_alternative() {
    assert!(wrapped_push_message("'solo'").contains("expects string"));
}

/// An array *literal* argument resolves to a bare `array` with no element
/// type, unlike a variable or call typed `array<string>`. The iterable
/// alternative must still bind through its element rather than leaving
/// `TWrapValue` unbound.
#[test]
fn an_array_literal_argument_binds_the_template_through_the_iterable_alternative() {
    assert!(wrapped_push_message("['a', 'b']").contains("expects string"));
}

// ─── A method-level template survives into a directly chained call ──────────

/// A generic wrapper with both a static factory and an instance method that
/// bind a method-level `@template` from their argument and return a re-typed
/// wrapper.  `push()` then reports what that template bound to.
const METHOD_TEMPLATE_FACTORY: &str = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 */
class Wrapper
{
    /**
     * @template TMakeValue
     *
     * @param  iterable<array-key, TMakeValue>  $value
     * @return static<array-key, TMakeValue>
     */
    public static function make($value) {}

    /**
     * @template TReValue
     *
     * @param  iterable<array-key, TReValue>  $value
     * @return static<array-key, TReValue>
     */
    public function rewrap($value) {}

    /** @param TValue $value */
    public function push($value) {}
}

/** @return array<string> */
function names(): array { return []; }
"#;

/// Report the type error `push()` raises for `$expr->push([1])`, where the
/// receiver is a call that binds a method-level template.
fn factory_push_message(expr: &str) -> String {
    let php = format!("{METHOD_TEMPLATE_FACTORY}\n{expr}->push([1]);\n");
    let messages = type_error_messages(&collect(&php));
    assert_eq!(
        messages.len(),
        1,
        "expected exactly one type error for `{expr}`, got {messages:?}"
    );
    messages.into_iter().next().unwrap()
}

#[test]
fn a_static_factory_binds_its_template_into_a_chained_call() {
    assert!(factory_push_message("Wrapper::make(names())").contains("expects string"));
}

#[test]
fn an_instance_method_binds_its_template_into_a_chained_call() {
    let php = format!("{METHOD_TEMPLATE_FACTORY}\n$w = new Wrapper();\n");
    assert!(
        type_error_messages(&collect(&format!("{php}$w->rewrap(names())->push([1]);\n")))
            .concat()
            .contains("expects string")
    );
}

#[test]
fn a_static_factory_binds_its_template_through_a_variable() {
    let php = format!("{METHOD_TEMPLATE_FACTORY}\n$w = Wrapper::make(names());\n$w->push([1]);\n");
    assert!(
        type_error_messages(&collect(&php))
            .concat()
            .contains("expects string")
    );
}

/// A collection subclass that fixes its key/value templates purely
/// through `@extends` (no `@template` of its own — `ItemCollection` from
/// `REKEYING_COLLECTION`) still has its key template rebound by
/// `keyBy()`. Before the fix, the rebind was silently dropped and the
/// subclass kept whatever `@extends` had baked in (`int`), so `get()`
/// was checked against the stale key type.
#[test]
fn keyby_rebinds_the_key_template_on_an_extends_fixed_subclass() {
    for chained in [false, true] {
        let call = if chained {
            "$c->keyBy(fn (Item $i): string => $i->slug)->get([1]);".to_string()
        } else {
            "$keyed = $c->keyBy(fn (Item $i): string => $i->slug);\n$keyed->get([1]);".to_string()
        };
        let php = format!("{REKEYING_COLLECTION}\n$c = new ItemCollection();\n{call}\n");
        let messages = type_error_messages(&collect(&php));
        assert_eq!(messages.len(), 1, "chained={chained}, got {messages:?}");
        assert!(
            messages[0].contains("expects string|null"),
            "chained={chained}: {messages:?}"
        );
    }
}

/// `keyBy('column')` on the same `@extends`-fixed subclass widens the key
/// to `array-key` (the conditional's `array|string` branch), same as on a
/// plain `Coll` — the rebind must not keep the subclass's stale `int`
/// binding just because the new key type happens to be a string literal.
#[test]
fn keyby_with_literal_key_still_correct_on_an_extends_fixed_subclass() {
    let php = format!(
        "{REKEYING_COLLECTION}\n$c = new ItemCollection();\n$keyed = $c->keyBy('slug');\n$keyed->get('x');\n"
    );
    let messages = type_error_messages(&collect(&php));
    assert!(
        messages.is_empty(),
        "expected no type error, got {messages:?}"
    );
}

// ─── mapWithKeys() binds the callback's array key/value, not its whole return type ───

/// `mapWithKeys()`'s callback returns `array<TMapWithKeysKey,
/// TMapWithKeysValue>`; the new collection's key template must come from
/// the array's *key*, not the callback's whole return type. The
/// callback's bare `: array` annotation carries no key/value info, so
/// the binding falls through to the body expression — here a call whose
/// own declared return type does.
#[test]
fn mapwithkeys_binds_the_key_from_the_callback_body_when_the_annotation_is_bare() {
    let php = format!(
        "{REKEYING_COLLECTION}\n$c = new Coll();\n$keyed = $c->mapWithKeys(fn (Item $i): array => $i->toPair());\n$keyed->get([1]);\n"
    );
    let messages = type_error_messages(&collect(&php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("expects string|null"), "{messages:?}");
}

/// Regression guard for the exact false positive reported in the
/// backlog: before the fix, `TMapWithKeysKey` bound to the callback's
/// whole return type (`array`), so a string key argument was rejected.
#[test]
fn mapwithkeys_does_not_bind_the_key_to_the_whole_return_type() {
    let php = format!(
        "{REKEYING_COLLECTION}\n$c = new Coll();\n$keyed = $c->mapWithKeys(fn (Item $i): array => ['x' => $i]);\n$keyed->get('dk');\n"
    );
    let messages = type_error_messages(&collect(&php));
    assert!(
        messages.is_empty(),
        "expected no type error, got {messages:?}"
    );
}

// ─── PHPDoc pseudo-type spellings we have no model for ──────────────────────

/// A hyphenated spelling that no keyword covers used to be qualified against
/// the file's namespace and enforced as if it were a class, so every call site
/// was checked against a type nothing can satisfy.
#[test]
fn unmodelled_pseudo_type_param_does_not_reject_every_argument() {
    let php = r#"<?php
namespace App;

/** @param decimal-int-string $value */
function acceptsDecimalIntString($value): void {}

acceptsDecimalIntString('123');
acceptsDecimalIntString(42);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(
        messages.is_empty(),
        "expected no type error for an unmodelled spelling, got {messages:?}"
    );
}

/// `pure-callable` is recognized; only its purity is unmodelled. Widening it to
/// `callable` keeps the check that a non-callable argument is wrong.
#[test]
fn pure_callable_param_still_rejects_a_non_callable() {
    let php = r#"<?php
namespace App;

/** @param pure-callable $value */
function acceptsPureCallable($value): void {}

acceptsPureCallable(static fn (int $v): int => $v + 1);
acceptsPureCallable(1);
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("callable"), "{messages:?}");
}

/// `value-of<T>` over a concrete shape is the union of the shape's values, so
/// a value of the shape passes and one of its *keys* does not.
#[test]
fn value_of_over_a_concrete_shape_is_enforced_as_its_value_union() {
    let php = r#"<?php
namespace App;

/** @param value-of<array{a: int, b: int}> $value */
function acceptsShapeValue($value): void {}

acceptsShapeValue(1);
acceptsShapeValue('a');
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'a'"), "{messages:?}");
}

/// `key-of<T>` likewise, in the other direction.
#[test]
fn key_of_over_a_concrete_shape_is_enforced_as_its_key_union() {
    let php = r#"<?php
namespace App;

/** @param key-of<array{name: string, age: int}> $key */
function acceptsShapeKey($key): void {}

acceptsShapeKey('name');
acceptsShapeKey('age');
acceptsShapeKey('missing');
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'missing'"), "{messages:?}");
}

/// When the operand cannot be read (here a constant on a class outside the
/// project), the operator names no set of values. It has to widen rather than
/// reject: as a parameter to the bound its result falls within, as an argument
/// to "unknown".
#[test]
fn unevaluated_key_of_widens_instead_of_rejecting() {
    let php = r#"<?php
namespace App;

/** @param key-of<\Vendor\Config::MAP> $key */
function acceptsConfigKey($key): void {}

/** @return value-of<\Vendor\Config::MAP> */
function firstValue() { return 1; }

acceptsConfigKey('debug');
acceptsConfigKey('anything');

function acceptsInt(int $value): void {}
acceptsInt(firstValue());
"#;
    let messages = type_error_messages(&collect(php));
    assert!(
        messages.is_empty(),
        "expected no type error while the operand is unreadable, got {messages:?}"
    );
}

/// A constant naming an array literal is a readable operand: `CONST[T]`
/// with `T` bound to one of its keys is that key's own value type, not the
/// union of every value the table holds.
#[test]
fn index_access_over_a_constant_resolves_per_templated_key() {
    let php = r#"<?php
namespace App;

const ID_TABLE = ['immutable' => 1, 'mutable' => 'two'];

/**
 * @template T of key-of<ID_TABLE>
 * @param T $type
 * @return ID_TABLE[T]
 */
function lookUp(string $type = 'immutable'): int|string { return ID_TABLE[$type]; }

function takesInt(int $id): void {}
function takesString(string $id): void {}

takesInt(lookUp('immutable'));
takesString(lookUp('mutable'));
takesInt(lookUp('mutable'));
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'two'"), "{messages:?}");
}

/// An omitted argument binds its template from the parameter's default
/// value, so `lookUp()` reads `CONST[T]` for the default key exactly as
/// `lookUp('immutable')` does.
#[test]
fn index_access_over_a_constant_resolves_from_a_parameter_default() {
    let php = r#"<?php
namespace App;

const ID_TABLE = ['immutable' => 1, 'mutable' => 'two'];

/**
 * @template T of key-of<ID_TABLE>
 * @param T $type
 * @return ID_TABLE[T]
 */
function lookUp(string $type = 'immutable'): int|string { return ID_TABLE[$type]; }

/**
 * @template T of key-of<ID_TABLE>
 * @param T $type
 * @return ID_TABLE[T]
 */
function lookUpString(string $type = 'mutable'): int|string { return ID_TABLE[$type]; }

function takesInt(int $id): void {}
function takesString(string $id): void {}

takesInt(lookUp());
takesString(lookUpString());
takesInt(lookUpString());
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'two'"), "{messages:?}");
}

/// The same, for a method whose omitted argument defaults to a table key.
#[test]
fn index_access_over_a_constant_resolves_from_a_method_parameter_default() {
    let php = r#"<?php
namespace App;

class Ids {
    const TABLE = ['immutable' => 1, 'mutable' => 'two'];

    /**
     * @template T of key-of<Ids::TABLE>
     * @param T $type
     * @return Ids::TABLE[T]
     */
    public function lookUp(string $type = 'immutable') { return self::TABLE[$type]; }

    /**
     * @template T of key-of<Ids::TABLE>
     * @param T $type
     * @return Ids::TABLE[T]
     */
    public function lookUpString(string $type = 'mutable') { return self::TABLE[$type]; }
}

function takesInt(int $id): void {}
function takesString(string $id): void {}

$ids = new Ids();
takesInt($ids->lookUp());
takesString($ids->lookUpString());
takesInt($ids->lookUpString());
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'two'"), "{messages:?}");
}

/// The same, for a class constant operand.
#[test]
fn index_access_over_a_class_constant_resolves_per_templated_key() {
    let php = r#"<?php
namespace App;

class Ids { const TABLE = ['immutable' => 1, 'mutable' => 'two']; }

/**
 * @template T of key-of<Ids::TABLE>
 * @param T $type
 * @return Ids::TABLE[T]
 */
function lookUp(string $type) { return Ids::TABLE[$type]; }

function takesInt(int $id): void {}

takesInt(lookUp('immutable'));
takesInt(lookUp('mutable'));
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

/// A `@template T of array<array-key, mixed>` bound from an array-literal
/// argument takes the argument's own shape, not the erased bound, so
/// `key-of<T>` projects the literal's actual keys.
#[test]
fn key_of_template_binds_to_call_site_array_literal_shape() {
    let php = r#"<?php
namespace App;

/**
 * @template T of array<array-key, mixed>
 * @param T $items
 * @return key-of<T>
 */
function firstKey(array $items) { return array_key_first($items); }

/** @param 'debug'|'verbose' $flag */
function acceptsFlagName(string $flag): void {}

acceptsFlagName(firstKey(['debug' => false, 'verbose' => true]));
acceptsFlagName(firstKey(['other' => false]));
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'other'"), "{messages:?}");
}

/// The counterpart to the `key-of` case above: `value-of<T>` over a
/// template bound from an array literal projects each element's own
/// literal value, not the scalar type it widens to.
#[test]
fn value_of_template_binds_to_call_site_array_literal_values() {
    let php = r#"<?php
namespace App;

/**
 * @template T of array<array-key, mixed>
 * @param T $items
 * @return value-of<T>
 */
function firstValue(array $items) { foreach ($items as $v) { return $v; } throw new \RuntimeException(); }

/** @param 1|10 $level */
function acceptsLevel(int $level): void {}

/** @param 'on'|'off' $mode */
function acceptsMode(string $mode): void {}

acceptsLevel(firstValue(['low' => 1, 'high' => 10]));
acceptsMode(firstValue(['a' => 'on', 'b' => 'off']));
acceptsLevel(firstValue(['high' => 99]));
acceptsMode(firstValue(['a' => 'maybe']));
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 2, "got {messages:?}");
    assert!(messages[0].contains("99"), "{messages:?}");
    assert!(messages[1].contains("'maybe'"), "{messages:?}");
}

/// A `key-of<CONSTANT>` parameter constrains the call even though the
/// function declares no `@template`: the constant is as readable from a
/// plain signature as it is from a templated one.
#[test]
fn key_of_over_a_constant_is_enforced_without_a_template() {
    let php = r#"<?php
namespace App;

const ID_TABLE = ['immutable' => 1, 'mutable' => 'two'];

/** @param key-of<ID_TABLE> $key */
function acceptsKey(string $key): void {}

acceptsKey('immutable');
acceptsKey('mutable');
acceptsKey('nope');
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'nope'"), "{messages:?}");
}

/// The same for a class constant read through a method parameter.
#[test]
fn key_of_over_a_class_constant_is_enforced_on_a_method() {
    let php = r#"<?php
namespace App;

class Ids {
    const TABLE = ['immutable' => 1, 'mutable' => 'two'];

    /** @param key-of<Ids::TABLE> $key */
    public function pick(string $key): void {}
}

function run(Ids $ids): void {
    $ids->pick('mutable');
    $ids->pick('nope');
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'nope'"), "{messages:?}");
}

/// And a `value-of<CONSTANT>` return is the table's value union, so a
/// caller that only accepts one half of it is reported.
#[test]
fn value_of_over_a_constant_types_an_untemplated_return() {
    let php = r#"<?php
namespace App;

const ID_TABLE = ['immutable' => 1, 'mutable' => 'two'];

/** @return value-of<ID_TABLE> */
function anyValue() { return 1; }

function takesInt(int $id): void {}
function takesIntOrString(int|string $id): void {}

takesIntOrString(anyValue());
takesInt(anyValue());
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("'two'"), "{messages:?}");
}

/// Inside the body, the parameter holds one of the table's keys: the
/// declaration is read where the walker seeds the scope, not only where
/// the call site checks the argument.
#[test]
fn key_of_over_a_constant_types_the_parameter_in_the_body() {
    let php = r#"<?php
namespace App;

const ID_TABLE = ['immutable' => 1, 'mutable' => 'two'];

function takesInt(int $x): void {}
function takesKey(string $x): void {}

/** @param key-of<ID_TABLE> $key */
function acceptsKey(string $key): void {
    takesKey($key);
    takesInt($key);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("'immutable'|'mutable'"),
        "{messages:?}"
    );
}

/// The same on a method, where the parameter type the walker reads comes
/// from the merged class rather than the source docblock.  Both spellings
/// of the owning class resolve.
#[test]
fn key_of_over_a_class_constant_types_a_method_parameter() {
    let php = r#"<?php
namespace App;

function takesInt(int $x): void {}

class Ids {
    const TABLE = ['immutable' => 1, 'mutable' => 'two'];

    /** @param key-of<Ids::TABLE> $key */
    public function pick(string $key): void {
        takesInt($key);
    }

    /** @param key-of<self::TABLE> $key */
    public function pickSelf(string $key): void {
        takesInt($key);
    }
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 2, "got {messages:?}");
    assert!(
        messages.iter().all(|m| m.contains("'immutable'|'mutable'")),
        "{messages:?}"
    );
}

// ── interface-string ────────────────────────────────────────────────

#[test]
fn a_class_name_is_not_an_interface_string() {
    // `interface-string` constrains the name the string holds, not the
    // type that name denotes, so a class that implements the interface
    // is still the wrong kind of name.
    let php = r#"<?php
namespace App;

interface SomeInterface {}

final class SomeClass implements SomeInterface {}

enum SomeEnum {}

/** @param interface-string $value */
function acceptsInterfaceString($value): void {}

acceptsInterfaceString(SomeInterface::class);
acceptsInterfaceString(SomeClass::class);
acceptsInterfaceString(SomeEnum::class);
acceptsInterfaceString('App\SomeClass');
acceptsInterfaceString('App\SomeInterface');
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 3, "got {messages:?}");
    assert!(
        messages.iter().all(|m| m.contains("interface-string")),
        "each message should name the expected type, got {messages:?}"
    );
}

#[test]
fn an_unknown_class_name_is_accepted_as_an_interface_string() {
    // The name may well belong to an interface in a file we never
    // indexed, and a bare `class-string` says nothing either way.
    let php = r#"<?php
namespace App;

/** @param interface-string $value */
function acceptsInterfaceString($value): void {}

/** @param class-string $name */
function forward(string $name): void {
    acceptsInterfaceString($name);
    acceptsInterfaceString('App\Unindexed');
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

#[test]
fn an_interface_string_satisfies_a_string_parameter() {
    let php = r#"<?php
namespace App;

/** @return interface-string */
function returnsInterfaceString(): string { return \Countable::class; }

function acceptsString(string $value): void {}

/** @param class-string $name */
function acceptsClassString(string $name): void {}

acceptsString(returnsInterfaceString());
acceptsClassString(returnsInterfaceString());
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A closure whose declared return type contradicts the one the parameter's
/// `callable(...)` spelling promises is a mismatch the callee will hit on the
/// first call, not a signature we merely failed to verify.
#[test]
fn callable_spec_rejects_a_closure_returning_the_wrong_type() {
    let php = r#"<?php
declare(strict_types=1);

namespace App;

/** @param callable(int): string $callback */
function takesStringCallback(callable $callback): void {}

takesStringCallback(static fn (int $value): string => (string) $value);
takesStringCallback(static fn (int $value): int => $value);
takesStringCallback(static function (int $value): int { return $value; });
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 2, "got {messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("return type int does not satisfy string")),
        "{messages:?}"
    );
}

/// A closure that declares no return type carries none on its resolved type,
/// and neither does a first-class callable — both have to stay silent rather
/// than be read as returning nothing.
#[test]
fn callable_spec_stays_silent_when_the_closure_declares_no_return_type() {
    let php = r#"<?php
namespace App;

/** @param callable(int): string $callback */
function takesStringCallback(callable $callback): void {}

takesStringCallback(static fn (int $value) => $value);
takesStringCallback(static function (int $value) { return $value; });
takesStringCallback(strlen(...));
takesStringCallback('strlen');
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A `static` return type needs the class context the compatibility layer
/// deliberately does not guess at, and a covariant one is not a mismatch.
#[test]
fn callable_spec_accepts_a_covariant_or_relative_closure_return_type() {
    let php = r#"<?php
namespace App;

class Animal {}
class Cat extends Animal {}

class Shelter
{
    /** @param callable(int): Animal $factory */
    public function register(callable $factory): void {}

    /** @param callable(int): static $factory */
    public function registerSelf(callable $factory): void {}

    public function go(): void {
        $this->register(static fn (int $i): Cat => new Cat());
        $this->registerSelf(fn (int $i): static => $this);
    }
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

// ─── Required array-shape keys ──────────────────────────────────────────────

/// An array written out at the call site lists every key it has, so a
/// required shape key that is not among them is genuinely absent.
#[test]
fn array_literal_missing_a_required_shape_key_is_reported() {
    let php = r#"<?php
/** @param array{host: string, port: int} $config */
function takesConfig(array $config): void {}

takesConfig(['host' => 'localhost']);
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("missing required key 'port'"),
        "got {messages:?}"
    );
}

/// An empty array literal enumerates all (zero) of its keys just as
/// completely as a non-empty one, so every required key is absent.
#[test]
fn empty_array_literal_missing_required_shape_keys_is_reported() {
    let php = r#"<?php
/** @param array{host: string, port: int} $config */
function takesConfig(array $config): void {}

takesConfig([]);
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("missing required keys 'host', 'port'"),
        "got {messages:?}"
    );
}

#[test]
fn array_literal_missing_several_required_shape_keys_names_all_of_them() {
    let php = r#"<?php
/** @param array{host: string, port: int, user: string} $config */
function takesConfig(array $config): void {}

takesConfig(['port' => 3306]);
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("missing required keys 'host', 'user'"),
        "got {messages:?}"
    );
}

/// Key order is irrelevant, extra keys are harmless, and an optional key
/// is by definition not required.
#[test]
fn array_literal_satisfying_a_shape_is_not_reported() {
    let php = r#"<?php
/** @param array{host: string, port: int} $config */
function takesConfig(array $config): void {}

/** @param array{host: string, port?: int} $config */
function takesOptional(array $config): void {}

takesConfig(['host' => 'localhost', 'port' => 3306]);
takesConfig(['port' => 3306, 'host' => 'localhost']);
takesConfig(['host' => 'localhost', 'port' => 3306, 'debug' => true]);
takesOptional(['host' => 'localhost']);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A shape written positionally holds the same keys as one written with
/// the indices spelled out.
#[test]
fn positional_shape_entries_count_as_their_index() {
    let php = r#"<?php
/** @param array{0: string, 1: string} $pair */
function takesPair(array $pair): void {}

takesPair(['a', 'b']);
takesPair([0 => 'a', 1 => 'b']);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A shape inferred from a variable records the keys we saw assigned,
/// which is a lower bound on the keys the value has — a branch we could
/// not follow may add more. Only a literal at the call site is proof.
#[test]
fn a_shape_that_did_not_come_from_a_literal_argument_stays_silent() {
    let php = r#"<?php
/** @param array{host: string, port: int} $config */
function takesConfig(array $config): void {}

function build(bool $withPort): array {
    $config = ['host' => 'localhost'];
    if ($withPort) {
        $config['port'] = 3306;
    }
    return $config;
}

$partial = ['host' => 'localhost'];
takesConfig($partial);
takesConfig(build(true));
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A key spelled as anything but a literal, or spread in from another
/// array, leaves keys we cannot read — so the literal is no longer a
/// complete account of what it holds.
#[test]
fn a_literal_with_keys_we_cannot_read_stays_silent() {
    let php = r#"<?php
const PORT_KEY = 'port';

/** @param array{host: string, port: int} $config */
function takesConfig(array $config): void {}

takesConfig(['host' => 'localhost', PORT_KEY => 3306]);
takesConfig(['host' => 'localhost', ...['port' => 3306]]);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

// ─── List order ─────────────────────────────────────────────────────────────

/// `array_is_list()` is `false` for a literal whose keys are written out
/// of order, so it does not hold a `list`, however well its values fit.
#[test]
fn a_literal_with_reversed_keys_does_not_satisfy_a_list() {
    let php = r#"<?php
/** @param list{string, string} $pair */
function takesPair(array $pair): void {}

/** @param list<string> $items */
function takesItems(array $items): void {}

takesPair([1 => 'x', 0 => 'y']);
takesItems([1 => 'x', 0 => 'y']);
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 2, "got {messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("keys are not in list order")),
        "got {messages:?}"
    );
    assert!(
        messages[0].contains("expects list{string, string}"),
        "the parameter is reported as the list shape it was written as, got {messages:?}"
    );
}

/// A literal whose keys are gapped or named is no more a list than a
/// reversed one is.
#[test]
fn a_literal_with_gapped_or_named_keys_does_not_satisfy_a_list() {
    let php = r#"<?php
/** @param list<string> $items */
function takesItems(array $items): void {}

takesItems([0 => 'x', 2 => 'y']);
takesItems(['first' => 'x', 'second' => 'y']);
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 2, "got {messages:?}");
}

/// Keys written out in list order hold a list, and so does a literal
/// written without keys at all.
#[test]
fn a_literal_whose_keys_are_in_list_order_satisfies_a_list() {
    let php = r#"<?php
/** @param list{string, string} $pair */
function takesPair(array $pair): void {}

/** @param list<string> $items */
function takesItems(array $items): void {}

takesPair([0 => 'x', 1 => 'y']);
takesPair(['x', 'y']);
takesItems([0 => 'x', 1 => 'y']);
takesItems(['x', 'y']);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// An `array{…}` shape makes no promise about the order of its keys, so
/// the same literal satisfies it.
#[test]
fn reversed_keys_still_satisfy_an_array_shape() {
    let php = r#"<?php
/** @param array{string, string} $pair */
function takesPair(array $pair): void {}

takesPair([1 => 'x', 0 => 'y']);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A shape inferred from a variable lists the keys we saw assigned in the
/// order we saw them, which is not the order the value's keys are in.
#[test]
fn list_order_stays_silent_for_a_shape_that_did_not_come_from_a_literal() {
    let php = r#"<?php
/** @param list<string> $items */
function takesItems(array $items): void {}

$items = [];
$items[1] = 'x';
$items[0] = 'y';
takesItems($items);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

// ─── Short ternary ──────────────────────────────────────────────────────────

/// `$x ?: $default` yields `$x` only where `$x` was truthy, so the falsy
/// members of its type are not part of the result.
#[test]
fn a_short_ternary_drops_the_conditions_falsy_members() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function content() { return ''; }

$body = content() ?: '';
useString($body);
useString(content() ?: '');
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Only the short form yields the condition's own value: a full ternary
/// naming the same call in its then branch still contributes every member
/// of that call's type.
#[test]
fn a_full_ternary_keeps_its_then_branch_whole() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function content() { return ''; }

useString(content() ? content() : '');
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

/// The condition can be truthy in more than one way, and all of those
/// ways stay in the result.
#[test]
fn a_short_ternary_keeps_every_truthy_member_of_the_condition() {
    let php = r#"<?php
class A { public function a(): void {} }
class B { public function b(): void {} }

function useAOrB(A|B $value): void {}
function useA(A $value): void {}

/** @return A|B|null */
function pick() { return null; }

useAOrB(pick() ?: new A());
useA(pick() ?: new A());
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

/// A full ternary whose condition is the same bare variable as its then
/// branch narrows that branch the same way an `if ($x) { … }` body would:
/// reaching the then branch proves `$x` was truthy, so its falsy members
/// are not part of the result.
#[test]
fn a_full_ternarys_bare_variable_condition_narrows_its_own_then_branch() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function content() { return ''; }

$x = content();
useString($x ? $x : '');
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

// ─── Docblock refinement of a native union ──────────────────────────────────

/// A docblock may narrow one member of an all-scalar native union: the
/// value reads as `false|string` rather than the declared `bool|string`.
#[test]
fn a_docblock_refines_one_member_of_a_native_scalar_union() {
    let php = r#"<?php
function useInt(int $value): void {}

/**
 * @return false|string
 */
function parseImage(array $fragments): bool|string { return false; }

useInt(parseImage([]));
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("false|string"),
        "the docblock should refine `bool` to `false`, got {messages:?}"
    );
}

/// A docblock describing something the native union does not mention is
/// still ignored: the native hint is the more trustworthy of the two.
#[test]
fn a_docblock_unrelated_to_the_native_union_is_ignored() {
    let php = r#"<?php
function useString(string $value): void {}

/**
 * @return array<int>
 */
function widen(): bool|string { return ''; }

useString(widen());
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("bool|string"),
        "the native union should stand, got {messages:?}"
    );
}

// ─── `!== false` in a truthy branch ─────────────────────────────────────────

/// `if ($x !== false)` rules out `false` for the body, the same way
/// `if ($x !== null)` already rules out `null`.
#[test]
fn a_not_false_check_narrows_inside_the_if_body() {
    let php = r#"<?php
function takesNonEmptyString(string $value): void {}

/** @param non-empty-string|false $value */
function inspect($value): void {
    if ($value !== false) {
        takesNonEmptyString($value);
    }
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The whole idiom, end to end: a docblock refines the native union to
/// `false|string` and `!== false` narrows that `false` away.
#[test]
fn a_refined_union_narrows_through_a_not_false_check() {
    let php = r#"<?php
function useString(string $value): void {}

/**
 * @return false|string
 */
function parseImage(array $fragments): bool|string { return false; }

$image = parseImage([]);
if ($image !== false) {
    useString($image);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// `while ($row !== false)` narrows its body the same way, which is the
/// shape every `fgets()`/`fgetcsv()` read loop is written in.
#[test]
fn a_not_false_check_narrows_inside_a_while_body() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function readLine() { return false; }

$line = readLine();
while ($line !== false) {
    useString($line);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Only `false` is ruled out: a `null` member of the same union survives
/// the check, since `null !== false`.
#[test]
fn a_not_false_check_leaves_null_in_place() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false|null */
function readLine() { return null; }

$line = readLine();
if ($line !== false) {
    useString($line);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("null"),
        "only false should be narrowed away, got {messages:?}"
    );
}

// ─── `assert()` on an array element ─────────────────────────────────────────

/// `assert($items[0] instanceof Foo)` narrows the element it names, the
/// same way the check narrows it inside an `if`.
#[test]
fn an_assert_narrows_an_array_access_subject() {
    let php = r#"<?php
class Foo { }
class Bar { }
function takesFoo(Foo $f): void {}

/** @param array<int, Foo|Bar> $items */
function run(array $items): void {
    assert($items[0] instanceof Foo);
    takesFoo($items[0]);
}
"#;
    let messages = type_error_messages(&collect_slow(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A string key is keyed and narrowed the same way a numeric index is.
#[test]
fn an_assert_narrows_a_string_keyed_array_subject() {
    let php = r#"<?php
class Foo { }
class Bar { }
function takesFoo(Foo $f): void {}

/** @param array{main: Foo|Bar} $items */
function run(array $items): void {
    assert($items['main'] instanceof Foo);
    takesFoo($items['main']);
}
"#;
    let messages = type_error_messages(&collect_slow(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Only the element the assert names is narrowed: a sibling key keeps
/// the type it was declared with.
#[test]
fn an_assert_on_one_element_leaves_its_siblings_alone() {
    let php = r#"<?php
class Foo { }
class Bar { }
function takesFoo(Foo $f): void {}

/** @param array{a: Foo|Bar, b: Foo|Bar} $items */
function run(array $items): void {
    assert($items['a'] instanceof Foo);
    takesFoo($items['b']);
}
"#;
    let messages = type_error_messages(&collect_slow(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

// ─── Benevolent builtins ────────────────────────────────────────────────────

/// `tempnam()` declares `string|false`, but the failure branch fires only
/// when the temp directory itself is broken, so idiomatic PHP passes the
/// result straight on. PHPStan tags these builtins `__benevolent<>` and
/// stops enforcing the branch; PHPantom borrows the same list.
#[test]
fn a_builtin_whose_failure_branch_nobody_checks_is_not_enforced() {
    let php = r#"<?php
function takesString(string $path): void {}

function test(): void {
    $tmp = tempnam(sys_get_temp_dir(), 'x');
    takesString($tmp);
    takesString(tempnam(sys_get_temp_dir(), 'y'));
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The leniency is tied to the specific builtins on the list, not to
/// `|false` in general: an ordinary function that can fail is still a
/// function whose failure the caller has to deal with.
#[test]
fn an_ordinary_failure_branch_is_still_enforced() {
    let php = r#"<?php
function takesString(string $path): void {}

/** @return string|false */
function mightFail() { return false; }

function test(): void {
    $value = mightFail();
    takesString($value);
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(
        messages.iter().any(|m| m.contains("false")),
        "expected the `false` branch to still be reported, got {messages:?}"
    );
}

/// `strpos()` is deliberately *not* on the list: its `false` means "not
/// found", which is an answer the caller is expected to read.
#[test]
fn a_failure_branch_that_carries_meaning_is_still_enforced() {
    let php = r#"<?php
function takesInt(int $offset): void {}

function test(): void {
    $pos = strpos('haystack', 'needle');
    takesInt($pos);
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(
        messages.iter().any(|m| m.contains("false")),
        "expected strpos's `false` to still be reported, got {messages:?}"
    );
}

/// Leniency is a diagnostic policy, not a claim about the value: the union
/// stays intact, so a caller that *does* check the failure branch still
/// narrows through it.
#[test]
fn a_benevolent_result_still_narrows_on_an_identity_check() {
    let php = r#"<?php
function takesString(string $path): void {}

function test(): void {
    $tmp = tempnam(sys_get_temp_dir(), 'x');
    if ($tmp === false) {
        return;
    }
    takesString($tmp);
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}
/// Rejoining after a branch says nothing about a value neither side
/// touched, so the leniency has to survive the merge. It used to be
/// dropped by the literal collapse the join runs every local through,
/// which meant one unrelated `if` was enough to bring the `|false` back.
#[test]
fn a_benevolent_result_survives_a_branch_merge() {
    let php = r#"<?php
function takesString(string $path): void {}

function afterIf(bool $flag): void {
    $tmp = tempnam(sys_get_temp_dir(), 'x');
    if ($flag) {
        echo 'y';
    }
    takesString($tmp);
}

function afterIfElse(bool $flag): void {
    $tmp = tempnam(sys_get_temp_dir(), 'x');
    if ($flag) {
        echo 'y';
    } else {
        echo 'z';
    }
    takesString($tmp);
}

function afterWhile(bool $flag): void {
    $tmp = tempnam(sys_get_temp_dir(), 'x');
    while ($flag) {
        echo 'y';
    }
    takesString($tmp);
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The merge carries the marker only while every side had it: a union the
/// code spelled out itself is still enforced on the other side of an `if`.
#[test]
fn a_spelled_out_failure_branch_survives_a_branch_merge_too() {
    let php = r#"<?php
function takesString(string $path): void {}

/** @return string|false */
function mightFail() { return false; }

function test(bool $flag): void {
    $value = mightFail();
    if ($flag) {
        echo 'y';
    }
    takesString($value);
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(
        messages.iter().any(|m| m.contains("false")),
        "expected the `false` branch to still be reported, got {messages:?}"
    );
}

/// `DOMNode::appendChild()` is benevolent *and* templated
/// (`@return TNode|false`), so the marker has to survive both the template
/// substitution and the union simplification that follow it.
#[test]
fn a_benevolent_return_survives_template_substitution() {
    let php = r#"<?php
function takesNode(DOMNode $n): void {}

class Holder {
    private DOMNode $node;
    public function fill(DOMDocument $dom): void {
        $this->node = $dom->appendChild($dom->createElement('x'));
    }
}

function test(DOMDocument $dom): void {
    takesNode($dom->createElement('y'));
    takesNode($dom->appendChild($dom->createElement('z')));
}
"#;
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_argument_type_diagnostics(uri, php, &mut out);
    backend.collect_property_type_diagnostics(uri, php, &mut out);
    let messages: Vec<String> = out.iter().map(|d| d.message.clone()).collect();
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A read loop advances its own cursor inside the body, so the checked
/// variable is written to below the read.  The read still sees what the
/// loop condition established, not the reassigned type.
#[test]
fn a_while_body_that_reassigns_the_subject_keeps_the_narrowing() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function readLine() { return false; }

$line = readLine();
while ($line !== false) {
    useString($line);
    $line = readLine();
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The same shape with `null` as the sentinel, which is what an iterator
/// walked by hand (`$node = $node->next()`) looks like.
#[test]
fn a_while_body_that_reassigns_the_subject_keeps_the_null_narrowing() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|null */
function readLine() { return null; }

$line = readLine();
while ($line !== null) {
    useString($line);
    $line = readLine();
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The reassignment is still in force for the statements written *below*
/// it: the widened type comes back once the loop body has advanced past
/// the write.
#[test]
fn a_while_body_reassignment_widens_below_itself() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function readLine() { return false; }

$line = readLine();
while ($line !== false) {
    $line = readLine();
    useString($line);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("string|false"),
        "the read below the reassignment sees the widened type, got {messages:?}"
    );
}

// ─── Assign-and-check in one condition ──────────────────────────────────────

/// `while (($line = fgets($h)) !== false)` assigns and checks in one
/// expression, so the assignment is the subject the check narrows.
#[test]
fn a_condition_assignment_is_narrowed_by_its_own_false_check() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function readLine() { return false; }

while (($line = readLine()) !== false) {
    useString($line);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The bare truthy form of the same loop rules out every falsy value,
/// `false` among them.
#[test]
fn a_bare_truthy_condition_assignment_is_narrowed() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function readLine() { return false; }

while ($line = readLine()) {
    useString($line);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The `null` sentinel is the shape a hand-walked iterator
/// (`while (($node = $node->next()) !== null)`) is written in.
#[test]
fn a_condition_assignment_is_narrowed_by_its_own_null_check() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|null */
function readLine() { return null; }

while (($line = readLine()) !== null) {
    useString($line);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// An `if` with the same shape narrows its body the same way.
#[test]
fn an_if_condition_assignment_is_narrowed_by_its_own_false_check() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function readLine() { return false; }

if (($line = readLine()) !== false) {
    useString($line);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Only the sentinel the check names is ruled out: `null` survives a
/// `!== false` check on an assignment that could produce it.
#[test]
fn a_condition_assignment_keeps_the_members_its_check_leaves() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false|null */
function readLine() { return null; }

if (($line = readLine()) !== false) {
    useString($line);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("null"),
        "only false should be narrowed away, got {messages:?}"
    );
}

// ─── A `for` condition narrows like `while`'s does ──────────────────────────

/// A `for` loop's condition narrows its body the same way `while`'s does:
/// the sentinel a sentinel check rules out doesn't survive into the body.
#[test]
fn a_for_condition_narrows_inside_the_body() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function readLine() { return false; }

$line = readLine();
for (; $line !== false; $line = readLine()) {
    useString($line);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The assign-and-check shape (`for (; ($row = fgetcsv($h)) !== false; )`)
/// narrows the same way a `while` with the same condition does.
#[test]
fn a_for_condition_assignment_is_narrowed_by_its_own_false_check() {
    let php = r#"<?php
function useCsvRow(array $row): void {}

/** @return array|false */
function readRow() { return false; }

for (; ($row = readRow()) !== false; ) {
    useCsvRow($row);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Only the sentinel the check names is ruled out: `null` survives a
/// `!== false` check in a `for` condition the same way it does in a `while`.
#[test]
fn a_for_condition_leaves_null_in_place() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false|null */
function readLine() { return null; }

$line = readLine();
for (; $line !== false; $line = readLine()) {
    useString($line);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("null"),
        "only false should be narrowed away, got {messages:?}"
    );
}

// ─── `assert()` narrows the way its `if` equivalent does ────────────────────

/// The defensive idiom guarding a `T|false` return: `assert()` proves its
/// argument for everything that follows, so the sentinel is gone by the
/// time the value is used.
#[test]
fn an_assert_rules_out_the_false_it_names() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function readLine() { return false; }

$line = readLine();
assert($line !== false);
useString($line);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The `null` sentinel is ruled out by the same shape.
#[test]
fn an_assert_rules_out_the_null_it_names() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|null */
function readLine() { return null; }

$line = readLine();
assert($line !== null);
useString($line);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A type-guard call inside the assert narrows just as it would inside an
/// `if`.
#[test]
fn an_assert_applies_a_type_guard_call() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|int */
function readLine() { return 1; }

$line = readLine();
assert(is_string($line));
useString($line);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// An `&&` chain proves each of its operands, so both subjects narrow.
#[test]
fn an_assert_applies_every_operand_of_an_and_chain() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false */
function readLine() { return false; }

$first = readLine();
$second = readLine();
assert($first !== false && $second !== false);
useString($first);
useString($second);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The fully-qualified spelling a namespaced file uses reaches the same
/// narrowing.
#[test]
fn a_fully_qualified_assert_narrows_too() {
    let php = r#"<?php
namespace App;

function useString(string $value): void {}

/** @return string|false */
function readLine() { return false; }

$line = readLine();
\assert($line !== false);
useString($line);
"#;
    let messages = type_error_messages(&collect(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Only the sentinel the assert names is ruled out: `null` survives an
/// `assert($x !== false)`.
#[test]
fn an_assert_keeps_the_members_it_does_not_name() {
    let php = r#"<?php
function useString(string $value): void {}

/** @return string|false|null */
function readLine() { return null; }

$line = readLine();
assert($line !== false);
useString($line);
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("null"),
        "only false should be narrowed away, got {messages:?}"
    );
}

// ─── Builtins whose return type depends on an argument ──────────────────────

/// `preg_replace`/`str_replace` are declared with the flat union of both of
/// their overloads, but a string subject can only come back as a string, so
/// the result satisfies a `string` parameter.
#[test]
fn a_replace_on_a_string_subject_is_a_string() {
    let php = r#"<?php
function useString(string $value): void {}

function test(string $text): void {
    useString(preg_replace('/a/', 'b', $text) ?? '');
    useString(str_replace('a', 'b', $text));
    useString(substr_replace($text, 'b', 0, 1));
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The same conditional decides a direct `return`, not just an argument:
/// a `string`-typed parameter fed straight into `str_replace()` and
/// returned still satisfies a `string` return type.
#[test]
fn a_replace_on_a_string_subject_satisfies_a_string_return_type() {
    let php = r#"<?php
function relative(string $filename): string
{
    return str_replace('\\', '/', $filename);
}
"#;
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_return_type_diagnostics(uri, php, &mut out);
    assert!(out.is_empty(), "got {out:?}");
}

/// The array branch is still enforced: an array subject returns an array,
/// which no `string` parameter accepts.
#[test]
fn a_replace_on_an_array_subject_is_not_a_string() {
    let php = r#"<?php
function useString(string $value): void {}

/** @param list<string> $lines */
function test(array $lines): void {
    useString(str_replace('a', 'b', $lines));
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
}

/// `json_encode()` is declared `string|false`, but `JSON_THROW_ON_ERROR`
/// raises a `JsonException` instead of returning `false`, so the result
/// satisfies a `string` parameter.
#[test]
fn json_encode_with_throw_on_error_is_a_string() {
    let php = r#"<?php
function useString(string $value): void {}

function test(mixed $value): void {
    useString(json_encode($value, JSON_THROW_ON_ERROR));
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The flag is still the flag when it is reached through a constant: one that
/// aliases it, one that ORs it with another, and a local variable holding such
/// a mask.
#[test]
fn json_encode_reads_throw_on_error_through_a_constant() {
    let php = r#"<?php
function useString(string $value): void {}

const ENCODE_FLAGS = JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR;

class Encoder {
    const FLAGS = JSON_THROW_ON_ERROR;

    public function test(mixed $value, int $options): void {
        useString(json_encode($value, self::FLAGS));
        useString(json_encode($value, ENCODE_FLAGS));
        useString(json_encode($value, $options | self::FLAGS));
        $mask = JSON_UNESCAPED_SLASHES | self::FLAGS;
        useString(json_encode($value, $mask));
    }
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A declared type on the constant (PHP 8.3 typed class constants) does not
/// hide the value behind it: `const int FLAGS = …` still folds to the mask its
/// initialiser computes.
#[test]
fn json_encode_reads_throw_on_error_through_a_typed_constant() {
    let php = r#"<?php
function useString(string $value): void {}

class Encoder {
    private const int DEFAULT_OPTIONS = JSON_HEX_TAG | JSON_THROW_ON_ERROR;
    private const int ALIAS = self::DEFAULT_OPTIONS;

    public function test(mixed $value, int $options): void {
        useString(json_encode($value, self::DEFAULT_OPTIONS));
        useString(json_encode($value, self::ALIAS));
        useString(json_encode($value, $options | self::DEFAULT_OPTIONS));
        $mask = JSON_UNESCAPED_SLASHES | self::DEFAULT_OPTIONS;
        useString(json_encode($value, $mask));
    }
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The shape the typed-constant report was filed against: a declared `string`
/// says the constant may hold any string, while the initialiser says which one
/// it does hold. A parameter that accepts only some of them tells the two
/// apart, and the one that is not among them is what gets reported.
#[test]
fn a_typed_string_constant_keeps_the_literal_it_holds() {
    let php = r#"<?php
/** @param 'ZeroSSL'|'LetsEncrypt' $provider */
function useProvider(string $provider): void {}

class SiteCertificate {
    final public const string ZERO_SSL = 'ZeroSSL';
    final public const string SELF_SIGNED = 'SelfSigned';
}

function test(): void {
    useProvider(SiteCertificate::ZERO_SSL);
    useProvider(SiteCertificate::SELF_SIGNED);
}
"#;
    let messages = type_error_messages(&collect(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("'SelfSigned'"),
        "the value the constant holds is what the message names, got {messages:?}"
    );
}

/// A constant defined in terms of itself has no value to fold, and folding it
/// must terminate rather than chase the cycle.
#[test]
fn a_cyclic_constant_leaves_the_declared_union() {
    let php = r#"<?php
function useString(string $value): void {}

class Encoder {
    const A = self::B;
    const B = self::A;

    public function test(mixed $value): void {
        useString(json_encode($value, self::A));
    }
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("false"),
        "the failure branch stands when the flag cannot be read, got {messages:?}"
    );
}

/// Without the flag the failure branch is real and still reported.
#[test]
fn json_encode_without_throw_on_error_may_be_false() {
    let php = r#"<?php
function useString(string $value): void {}

function test(mixed $value): void {
    useString(json_encode($value));
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("false"),
        "the failure branch is what is reported, got {messages:?}"
    );
}

// ─── instanceof narrowing drops the non-class union members ─────────────────

/// A conditional return type resolves to a single entry whose class is
/// the object member and whose type string is the whole union, so an
/// `instanceof` guard has to narrow the type string too — otherwise the
/// array member is still judged against the parameter at the call site.
#[test]
fn instanceof_guard_clause_drops_the_array_member_of_a_union_return_type() {
    let php = r#"<?php
class UploadedFile {}

class Request {
    /** @return UploadedFile|array<UploadedFile>|null */
    public function file(string $key) {}
}

class ImageService {
    public function store(UploadedFile $file): void {}
}

function upload(Request $request, ImageService $images): void {
    $file = $request->file('image');
    if (!$file instanceof UploadedFile) {
        throw new RuntimeException('missing');
    }
    $images->store($file);
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// The same union, narrowed by a plain then-branch rather than a guard
/// clause.  This one reaches the scope through a different narrowing
/// path, so it needs its own coverage.
#[test]
fn instanceof_then_branch_drops_the_array_member_of_a_union_return_type() {
    let php = r#"<?php
class UploadedFile {}

class Request {
    /** @return UploadedFile|array<UploadedFile> */
    public function file(string $key) {}
}

class ImageService {
    public function store(UploadedFile $file): void {}
}

function upload(Request $request, ImageService $images): void {
    $file = $request->file('image');
    if ($file instanceof UploadedFile) {
        $images->store($file);
    }
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// A declared union splits into one entry per member, which the same
/// narrowing has to reduce to the checked class.
#[test]
fn instanceof_guard_clause_drops_the_array_member_of_a_declared_union() {
    let php = r#"<?php
class UploadedFile {}

class ImageService {
    public function store(UploadedFile $file): void {}
}

function upload(UploadedFile|array|null $file, ImageService $images): void {
    if (!$file instanceof UploadedFile) {
        throw new RuntimeException('missing');
    }
    $images->store($file);
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

#[test]
fn instanceof_then_branch_drops_the_array_member_of_a_declared_union() {
    let php = r#"<?php
class UploadedFile {}

class ImageService {
    public function store(UploadedFile $file): void {}
}

function upload(UploadedFile|array|null $file, ImageService $images): void {
    if ($file instanceof UploadedFile) {
        $images->store($file);
    }
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert!(messages.is_empty(), "got {messages:?}");
}

/// Narrowing must not overreach: an exclusion (`!$x instanceof Y` in
/// the branch it guards) rules out only that class, so the array member
/// of the union has to survive.
#[test]
fn negated_instanceof_keeps_the_array_member_of_a_union_return_type() {
    let php = r#"<?php
class UploadedFile {}

class Request {
    /** @return UploadedFile|array<UploadedFile> */
    public function file(string $key) {}
}

class ImageService {
    public function store(UploadedFile $file): void {}
}

function upload(Request $request, ImageService $images): void {
    $file = $request->file('image');
    if (!$file instanceof UploadedFile) {
        $images->store($file);
    }
}
"#;
    let messages = type_error_messages(&collect_with_full_stubs(php));
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("array<UploadedFile>"),
        "the array member is what remains, got {messages:?}"
    );
}

// ─── Names reached through a bare namespace import ──────────────────────────

/// A class named through a bare `use App\Support;` (rather than a
/// per-class import) is the same class as the one a `new` expression
/// resolves to, so a template bound from both spellings must collapse to
/// one type rather than union `Support\Pen` with `App\Support\Pen`.
#[test]
fn a_qualified_name_via_a_namespace_import_binds_the_same_template_as_its_fqcn() {
    let backend = create_test_backend();

    let support_uri = "file:///support.php";
    let support = r#"<?php
namespace App\Support;

class Pen {}
class Pencil {}
"#;
    backend.update_ast(support_uri, support);

    let uri = "file:///test.php";
    let php = r#"<?php
namespace App;

use App\Support;

/** @template TValue */
class Reducible
{
    /**
     * @template TInitial
     * @template TReturn
     *
     * @param callable(TInitial|TReturn, TValue): TReturn $callback
     * @param TInitial $initial
     * @return TReturn
     */
    public function reduce(callable $callback, mixed $initial): mixed
    {
        return $initial;
    }
}

/** @var Reducible<Support\Pencil> $reducible */
$reducible = new Reducible();
$reducible->reduce(
    fn (Support\Pen $carry, Support\Pencil $item): Support\Pen => $carry,
    new Support\Pen()
);
"#;
    backend.update_ast(uri, php);
    let mut diags = Vec::new();
    backend.collect_argument_type_diagnostics(uri, php, &mut diags);
    let messages = type_error_messages(&diags);
    assert!(messages.is_empty(), "got {messages:?}");
}

// ─── By-reference out-parameters are written by the callee ──────────────────

#[test]
fn no_diagnostic_for_by_ref_out_param_with_null_default() {
    // A by-reference parameter that defaults to `null` is an out-parameter:
    // the null says the caller may omit the argument, not that the callee
    // may leave it null. This is the shape every PCRE out-parameter uses
    // (`preg_match(string $p, string $s, ?array &$m = null)`), so reading an
    // offset off `$matches` after the call must not be reported as possibly
    // null.
    let php = r#"<?php
/** @param null|string[] &$matches */
function match_it(string $pattern, string $subject, ?array &$matches = null): int|false
{
    return 0;
}

function consume(string $s): void {
    if (match_it('/(?<unit>\w+)/', $s, $match)) {
        strtolower($match['unit']);
        strtolower($match[0]);
    }
}
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        !has_type_error(&diags),
        "by-ref out-param $match should be a non-null array after the call: {diags:?}"
    );
}

#[test]
fn diagnostic_kept_for_nullable_by_ref_param_without_default() {
    // Without a default there is nothing to say the callee writes the
    // argument, so a nullable by-reference parameter stays nullable and the
    // null dereference is still reported.
    let php = r#"<?php
function fill(?string &$out): void
{
}

function consume(): void {
    $value = null;
    fill($value);
    strtolower($value);
}
"#;
    let diags = collect_with_full_stubs(php);
    assert!(
        has_type_error(&diags),
        "nullable by-ref param without a default should stay nullable: {diags:?}"
    );
}

/// A `key-of<CONSTANT>` operand is resolved against the file's import
/// table and read as an absolute name, not only as a bare one.
#[test]
fn key_of_over_an_imported_constant_types_the_parameter() {
    for operand in ["TABLE", "\\App\\ID_TABLE"] {
        let php = format!(
            r#"<?php
namespace App;

const ID_TABLE = ['immutable' => 1, 'mutable' => 'two'];

use const App\ID_TABLE as TABLE;

function takesInt(int $x): void {{}}

/** @param key-of<{operand}> $key */
function acceptsKey(string $key): void {{
    takesInt($key);
}}
"#
        );
        let messages = type_error_messages(&collect(&php));
        assert_eq!(messages.len(), 1, "`{operand}`: got {messages:?}");
        assert!(
            messages[0].contains("'immutable'|'mutable'"),
            "`{operand}`: {messages:?}"
        );
    }
}

/// A file may declare several `namespace` blocks. A call in the second
/// block must resolve against that block's namespace, not the first
/// block's, or the callee is never found and the check goes silent.
#[test]
fn second_namespace_block_still_checks_argument_types() {
    let php = r#"<?php
namespace App\Other;

class Marker {}

namespace App;

function takesInt(int $x): void {}

function plain(string $key): void {
    takesInt($key);
}
"#;
    assert_eq!(
        type_error_messages(&collect(php)),
        vec!["Argument 1 ($x) expects int, got string"],
        "a call in the second namespace block should still be checked"
    );
}

/// Same file shape, but the callee is a method on a class declared in the
/// second block and the mismatched value comes from a nested call.
#[test]
fn second_namespace_block_checks_method_and_nested_call_arguments() {
    let php = r#"<?php
namespace App\Other;

class Marker {}

namespace App;

function giveString(): string { return "x"; }

class Widget {
    public function takesInt(int $x): void {}
}

function run(): void {
    $w = new Widget();
    $w->takesInt(giveString());
}
"#;
    assert_eq!(
        type_error_messages(&collect(php)),
        vec!["Argument 1 ($x) expects int, got string"]
    );
}

// ─── Template bound from an inline call argument ────────────────────────────

/// A template bound from a call argument keeps the `null` arm the callee's
/// return type declares.  The class walk that resolves the argument reports
/// only the classes it can be, so the arm used to be dropped and the
/// substituted return claimed the value could never be null.
#[test]
fn template_bound_from_inline_call_keeps_null_arm() {
    let php = r#"<?php
class Carbon {
    public static function create(int $year): ?Carbon { return null; }
}

/**
 * @template T of Carbon|string|null
 * @param T $date
 * @return T
 */
function passthrough(mixed $date): mixed { return $date; }

function takesCarbon(Carbon $c): void {}

function run(): void {
    takesCarbon(passthrough(Carbon::create(2024)));
}
"#;
    assert_eq!(
        type_error_messages(&collect(php)),
        vec!["Argument 1 ($c) expects Carbon, got ?Carbon"],
        "the inline call form should bind `?Carbon`, like the variable form does"
    );
}

/// The same for a non-null alternative: a `Carbon|string` return binds both
/// arms, not just the one backed by a class.
#[test]
fn template_bound_from_inline_call_keeps_scalar_arm() {
    let php = r#"<?php
class Carbon {
    public static function create(int $year): Carbon|string { return "x"; }
}

/**
 * @template T of Carbon|string|null
 * @param T $date
 * @return T
 */
function passthrough(mixed $date): mixed { return $date; }

function takesCarbon(Carbon $c): void {}

function run(): void {
    takesCarbon(passthrough(Carbon::create(2024)));
}
"#;
    assert_eq!(
        type_error_messages(&collect(php)),
        vec!["Argument 1 ($c) expects Carbon, got Carbon|string (string does not satisfy Carbon)"]
    );
}

/// A call whose return type names one class and nothing else binds that
/// class alone, with no spurious alternative attached.
#[test]
fn template_bound_from_inline_call_keeps_plain_class() {
    let php = r#"<?php
class Carbon {
    public static function create(int $year): Carbon { return new Carbon(); }
}

/**
 * @template T of Carbon|string|null
 * @param T $date
 * @return T
 */
function passthrough(mixed $date): mixed { return $date; }

function takesCarbon(Carbon $c): void {}

function run(): void {
    takesCarbon(passthrough(Carbon::create(2024)));
}
"#;
    assert_eq!(type_error_messages(&collect(php)), Vec::<String>::new());
}

// ─── Narrowing inside an echoed expression ──────────────────────────────────

#[test]
fn an_echoed_ternary_narrows_its_arms() {
    // Blade compiles every `{{ … }}` to an `echo`, so a template's guards
    // are only honoured if an echoed expression narrows the way an
    // assigned or returned one does.
    let php = r#"<?php
class Text { public static function shout(string $v): string { return $v; } }

function render(?string $c): void {
    echo $c ? Text::shout($c) : '';
}
"#;
    assert_eq!(
        type_error_messages(&collect_slow(php)),
        Vec::<String>::new()
    );
}

#[test]
fn an_echoed_short_circuit_chain_narrows_its_right_hand_side() {
    let php = r#"<?php
class Text { public static function shout(string $v): string { return $v; } }

function render(?string $c): void {
    echo $c && Text::shout($c) ? 'yes' : 'no';
}
"#;
    assert_eq!(
        type_error_messages(&collect_slow(php)),
        Vec::<String>::new()
    );
}

#[test]
fn an_echoed_type_guard_narrows_the_else_arm() {
    let php = r#"<?php
interface Rule { public function getDescription(): string; }
function out(string $v): string { return $v; }

function render(Rule|string $rule): void {
    echo out(is_string($rule) ? $rule : $rule->getDescription());
}
"#;
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///echoed_type_guard.php";
    backend.update_ast(uri, php);
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri, php, &mut diags);
    assert_eq!(type_error_messages(&diags), Vec::<String>::new());
}

/// A `@return static` method called on an intersection-typed receiver
/// returns the whole intersection: late static binding names the runtime
/// class, which satisfies every member of the intersection, not only the
/// interface that happened to declare the method.
#[test]
fn static_return_keeps_the_receivers_whole_intersection() {
    let php = r#"<?php
interface IfaceA
{
    /** @return static */
    public function filter(): self;
}

interface IfaceB
{
    public function ifaceBMethod(): void;
}

function needsBoth(IfaceA&IfaceB $x): void {}

function test(IfaceA&IfaceB $scope): void
{
    $filtered = $scope->filter();
    needsBoth($filtered);
}
"#;
    let msgs = type_error_messages(&collect(php));
    assert!(
        msgs.is_empty(),
        "static on an IfaceA&IfaceB receiver should stay IfaceA&IfaceB, got: {msgs:?}"
    );
}

/// `new self(…)` names the enclosing class, and has to keep naming it when
/// its short name is also a global class's: the constructor whose arguments
/// get checked must be the namespaced class's, not `\Error`'s.
#[test]
fn new_self_checks_the_namespaced_classs_own_constructor() {
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
    let msgs = type_error_messages(&collect(php));
    assert!(
        msgs.is_empty(),
        "new self() must check App\\Error::__construct, not \\Error's, got: {msgs:?}"
    );
}

/// An unqualified class name in an inline `/** @var */` resolves against the
/// current namespace first, exactly like the same name in a `@param` tag.  A
/// name that also exists globally (`Error`, `Exception`, …) must not silently
/// switch meaning between the two spellings.
#[test]
fn inline_var_resolves_an_unqualified_name_against_the_current_namespace() {
    let php = r#"<?php
namespace {
    class Error {}
}

namespace App\Sub {
    class Error {}

    class Take
    {
        /** @param list<Error> $e */
        public function take(array $e): void {}
    }

    class Probe
    {
        public function run(Take $t): void
        {
            /** @var list<Error> $errors */
            $errors = [];
            $t->take($errors);
        }
    }
}
"#;
    let msgs = type_error_messages(&collect(php));
    assert!(
        msgs.is_empty(),
        "inline @var list<Error> must mean App\\Sub\\Error, got: {msgs:?}"
    );
}

/// The cross-file shape of the same thing, from `phpstan-src`: the
/// namespace's own `Error` lives in another file and the colliding one is a
/// stub, so neither is among the classes parsed out of the file being
/// analysed.
#[test]
fn inline_var_prefers_the_namespaces_own_class_over_a_stub_of_that_name() {
    use phpantom_lsp::Backend;
    use std::collections::HashMap;

    let files = [
        (
            "src/Analyser/Error.php",
            r#"<?php
namespace PHPStan\Analyser;

class Error {}
"#,
        ),
        (
            "src/Analyser/Analyser.php",
            r#"<?php
namespace PHPStan\Analyser;

class Analyser
{
    /** @param list<Error> $errors */
    public function report(array $errors): void {}

    public function run(): void
    {
        /** @var list<Error> $errors */
        $errors = [];
        $this->report($errors);
    }
}
"#,
        ),
    ];

    let dir = tempfile::tempdir().expect("failed to create temp dir");
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"PHPStan\\":"src/"}}}"#,
    )
    .expect("failed to write composer.json");
    for (rel_path, content) in &files {
        let full = dir.path().join(rel_path);
        std::fs::create_dir_all(full.parent().unwrap()).expect("failed to create dirs");
        std::fs::write(&full, content).expect("failed to write PHP file");
    }

    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert("Error", "<?php\nclass Error {}\n");
    let backend = Backend::new_test_with_stubs(stubs);
    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());
    *backend.psr4_mappings().write() = mappings;

    let uri = format!("file://{}/src/Analyser/Analyser.php", dir.path().display());
    let content = files[1].1;
    backend.update_ast(&uri, content);
    let mut diags = Vec::new();
    backend.collect_argument_type_diagnostics(&uri, content, &mut diags);
    let msgs = type_error_messages(&diags);
    assert!(
        msgs.is_empty(),
        "inline @var list<Error> must mean PHPStan\\Analyser\\Error, got: {msgs:?}"
    );
}

// ─── PHP's implicit widenings and imprecise types ───────────────────────────

#[test]
fn a_bounded_int_satisfies_a_float_parameter() {
    let php = r#"<?php
function wantsFloat(float $n): void {}

/** @param int<0, max> $count */
function f(int $count): void {
    wantsFloat($count);
}
"#;
    assert!(
        !has_type_error(&collect(php)),
        "PHP widens an int to a float on the way in, bounds and all: {:?}",
        type_error_messages(&collect(php))
    );
}

#[test]
fn a_class_string_satisfies_a_non_empty_string_parameter() {
    let php = r#"<?php
/** @param non-empty-string $s */
function wantsNonEmpty(string $s): void {}

/** @param class-string $c */
function f(string $c): void {
    wantsNonEmpty($c);
}
"#;
    assert!(
        !has_type_error(&collect(php)),
        "A string that names a class always has content: {:?}",
        type_error_messages(&collect(php))
    );
}

#[test]
fn an_array_key_satisfies_either_half_of_itself() {
    let php = r#"<?php
function wantsInt(int $i): void {}
function wantsString(string $s): void {}

/** @param array-key $k */
function f($k): void {
    wantsInt($k);
    wantsString($k);
}
"#;
    assert!(
        !has_type_error(&collect(php)),
        "`array-key` is the key type of an array nobody described, so one \
         half fitting is the whole bargain: {:?}",
        type_error_messages(&collect(php))
    );
}

#[test]
fn a_bitwise_op_on_untyped_operands_satisfies_a_string_return() {
    let php = r#"<?php
/** @param callable(mixed, mixed): string $cb */
function wantsStringCallback(callable $cb): void {}

function f(): void {
    wantsStringCallback(static fn ($a, $b) => $a & $b);
}
"#;
    assert!(
        !has_type_error(&collect(php)),
        "`&` over two strings produces a string, and neither operand rules \
         that out: {:?}",
        type_error_messages(&collect(php))
    );
}

// ─── Arithmetic over a value nobody typed ───────────────────────────────────

#[test]
fn arithmetic_on_an_undescribed_array_element_satisfies_an_int_parameter() {
    let php = r#"<?php declare(strict_types = 1);
function wantsInt(int $i): void {}

function f(array $placeholder): void {
    wantsInt($placeholder['position'] - 1);
}
"#;
    assert!(
        !has_type_error(&collect(php)),
        "`mixed - 1` is `int` or `float` depending on the value, and nothing \
         about the element rules either out: {:?}",
        type_error_messages(&collect(php))
    );
}

#[test]
fn arithmetic_chained_through_an_untyped_operand_stays_unenforced() {
    let php = r#"<?php declare(strict_types = 1);
/** @return mixed */
function tagLine() { return null; }

function wantsInt(int $i): void {}

function f(string $s): void {
    $line = tagLine();
    if ($line !== null) {
        wantsInt(strlen($s) + $line - 1);
    }
}
"#;
    assert!(
        !has_type_error(&collect_with_full_stubs(php)),
        "The addition already answered `int|float` for want of a typed \
         operand; the subtraction must not turn that into a promise: {:?}",
        type_error_messages(&collect_with_full_stubs(php))
    );
}

#[test]
fn arithmetic_on_a_string_operand_is_still_enforced() {
    let php = r#"<?php declare(strict_types = 1);
function wantsInt(int $i): void {}

function f(string $s): void {
    wantsInt($s * 2);
}
"#;
    assert!(
        has_type_error(&collect(php)),
        "A `string` operand is something the code did say, and PHP's answer \
         for it is not an int"
    );
}

#[test]
fn array_sum_over_an_undescribed_array_satisfies_an_int_parameter() {
    let php = r#"<?php declare(strict_types = 1);
function wantsInt(int $i): void {}

function f(array $json): void {
    $peaks = [];
    foreach ($json as $entry) {
        $peaks[] = $entry['memoryUsage'];
    }
    wantsInt(array_sum($peaks));
}
"#;
    assert!(
        !has_type_error(&collect_with_full_stubs(php)),
        "Summing values nobody typed is `int` or `float` by the same rule \
         adding two of them is: {:?}",
        type_error_messages(&collect_with_full_stubs(php))
    );
}

#[test]
fn array_sum_over_a_float_array_is_still_enforced() {
    let php = r#"<?php declare(strict_types = 1);
function wantsInt(int $i): void {}

/** @param list<float> $prices */
function f(array $prices): void {
    wantsInt(array_sum($prices));
}
"#;
    assert!(
        has_type_error(&collect_with_full_stubs(php)),
        "Every element is a float, so the sum is one too"
    );
}

// ─── A declared `mixed` against a body that disagrees with itself ───────────

#[test]
fn a_declared_mixed_stands_where_the_bodys_returns_disagree() {
    let php = r#"<?php
class Statement {}
interface Schema {}

class Factory
{
    public function build(Statement $statement): void
    {
        $this->process($this->processArgument($statement));
    }

    public function process(Schema $schema): void {}

    /**
     * @param mixed $argument
     * @return mixed
     */
    private function processArgument($argument)
    {
        if ($argument instanceof Statement) {
            return $this->makeSchema();
        } elseif (is_array($argument)) {
            return $argument;
        }

        return $argument;
    }

    private function makeSchema(): Schema
    {
        throw new \RuntimeException();
    }
}
"#;
    assert!(
        !has_type_error(&collect(php)),
        "The body can also hand back an array or its own `mixed` argument, \
         so reading it as one of two classes states a narrower answer than \
         the truth: {:?}",
        type_error_messages(&collect(php))
    );
}

// ─── By-reference out-parameters ────────────────────────────────────────────

/// A file the by-reference passes can read a body out of.
fn collect_with_body(php: &str) -> Vec<Diagnostic> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(php.to_string()));
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_argument_type_diagnostics(uri, php, &mut out);
    out
}

#[test]
fn a_by_reference_parameter_the_callee_always_assigns_loses_the_null_it_declares() {
    let php = r#"<?php
class Ops
{
    public static function keyFor(object $node, ?string &$key): void
    {
        $key = self::nodeKey($node);
    }

    public static function nodeKey(object $node): string
    {
        return 'k';
    }
}

class Caller
{
    public function run(object $node): string
    {
        Ops::keyFor($node, $key);

        return $this->takesString($key);
    }

    private function takesString(string $s): string
    {
        return $s;
    }
}
"#;
    assert!(
        !has_type_error(&collect_with_body(php)),
        "`keyFor` assigns `$key` on every path, so the `?string` it declares \
         is what may go in, not what comes back out: {:?}",
        type_error_messages(&collect_with_body(php))
    );
}

#[test]
fn a_by_reference_parameter_only_one_branch_assigns_keeps_its_declared_null() {
    let php = r#"<?php
class Ops
{
    public static function keyFor(bool $flag, ?string &$key): void
    {
        if ($flag) {
            $key = 'k';
        }
    }
}

class Caller
{
    public function run(bool $flag): string
    {
        Ops::keyFor($flag, $key);

        return $this->takesString($key);
    }

    private function takesString(string $s): string
    {
        return $s;
    }
}
"#;
    assert!(
        has_type_error(&collect_with_body(php)),
        "The other branch leaves `$key` unwritten, so null still reaches the caller"
    );
}

#[test]
fn a_body_that_contradicts_the_declared_out_type_does_not_replace_it() {
    let php = r#"<?php
class Ops
{
    /** @param int &$count */
    public static function count(&$count): void
    {
        $count = 'not an int';
    }
}

class Caller
{
    public function run(): int
    {
        Ops::count($count);

        return $this->takesInt($count);
    }

    private function takesInt(int $i): int
    {
        return $i;
    }
}
"#;
    assert!(
        !has_type_error(&collect_with_body(php)),
        "A reading of the body may sharpen the declaration, never overrule it: {:?}",
        type_error_messages(&collect_with_body(php))
    );
}

#[test]
fn the_value_a_by_reference_out_parameter_already_holds_is_not_checked() {
    let php = r#"<?php declare(strict_types = 1);
function collectAll(string $text): void
{
    foreach ([1, 2] as $_) {
        preg_match_all('~(\w+)~', $text, $matches, PREG_OFFSET_CAPTURE);
    }
}
"#;
    assert!(
        !has_type_error(&collect_with_full_stubs(php)),
        "On the second pass `$matches` still holds the offset-capture shape \
         the first left behind, and the declared `?array` describes what \
         `preg_match_all` writes, not what it accepts: {:?}",
        type_error_messages(&collect_with_full_stubs(php))
    );
}
